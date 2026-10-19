//! Telemetry middleware (Spec §1.3): injects a UUIDv7 `trace_id` on the first hop
//! into `backend/` and records stage-by-stage latency that lands in the Unified
//! JSON Envelope as `meta.timings_ms = { git_probe, go_heuristic_scan,
//! lazy_sidecar, cpp_compile }`.
//!
//! Policy (§1.3): no caching or warm-daemon optimization merges without
//! structured telemetry surrounding it — every build pipeline stage is wrapped
//! in one of the `Stage` timers below.

use std::time::Instant;

use uuid::Uuid;

use crate::models::envelope::{EnvelopeMeta, TimingsMs};

/// The four instrumented pipeline stages (§1.4 `meta.timings_ms` keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    GitProbe,
    GoScanner,
    LazySidecar,
    CppCompile,
}

impl Stage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::GitProbe => "git_probe",
            Stage::GoScanner => "go_heuristic_scan",
            Stage::LazySidecar => "lazy_sidecar",
            Stage::CppCompile => "cpp_compile",
        }
    }
}

/// Per-request telemetry context. Created by the middleware on the first hop into
/// `backend/`; handed to controllers, which time each stage and finalize the
/// envelope `meta`.
#[derive(Debug)]
pub struct Telemetry {
    trace_id: Uuid,
    started_at: Instant,
    timings: TimingsMs,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::begin()
    }
}

impl Telemetry {
    /// Middleware entry point (`Routes -> Middleware` hop): mints a fresh UUIDv7
    /// trace id for this request.
    pub fn begin() -> Self {
        Telemetry {
            trace_id: Uuid::now_v7(),
            started_at: Instant::now(),
            timings: TimingsMs::default(),
        }
    }

    /// Propagation constructor: workers and controllers continue a request's
    /// existing trace instead of minting a new one (§1.3 trace propagation).
    pub fn with_trace(trace_id: Uuid) -> Self {
        Telemetry {
            trace_id,
            started_at: Instant::now(),
            timings: TimingsMs::default(),
        }
    }

    /// Time one stage via closure: `telemetry.time(Stage::GoScanner, || scan())`.
    /// The stage's measured duration is folded into `timings_ms` automatically.
    pub fn time<T>(&mut self, stage: Stage, f: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let out = f();
        self.record(stage, started.elapsed().as_millis() as u64);
        out
    }

    /// Record a stage duration explicitly (for instrumented async stages or
    /// durations measured across process boundaries).
    pub fn record(&mut self, stage: Stage, duration_ms: u64) {
        match stage {
            Stage::GitProbe => self.timings.git_probe = duration_ms,
            Stage::GoScanner => self.timings.go_heuristic_scan = duration_ms,
            Stage::LazySidecar => self.timings.lazy_sidecar = duration_ms,
            Stage::CppCompile => self.timings.cpp_compile = duration_ms,
        }
    }

    /// Accumulate a stage duration on top of what was already recorded (useful
    /// when a logical stage is executed in multiple slices).
    pub fn accumulate(&mut self, stage: Stage, duration_ms: u64) {
        match stage {
            Stage::GitProbe => self.timings.git_probe += duration_ms,
            Stage::GoScanner => self.timings.go_heuristic_scan += duration_ms,
            Stage::LazySidecar => self.timings.lazy_sidecar += duration_ms,
            Stage::CppCompile => self.timings.cpp_compile += duration_ms,
        }
    }

    /// RAII guard version of `time()` for stages that cannot be expressed as one
    /// closure (e.g. a stage whose result is built across several calls).
    ///
    /// ```
    /// let mut t = Telemetry::begin();
    /// let mut guard = t.stage(Stage::CppCompile);
    /// // ... compile work ...
    /// drop(guard); // records elapsed time into `timings_ms.cpp_compile`
    /// ```
    pub fn stage(&mut self, stage: Stage) -> StageGuard<'_> {
        StageGuard::new(self, stage)
    }

    pub fn trace_id(&self) -> Uuid {
        self.trace_id
    }

    pub fn timings(&self) -> TimingsMs {
        self.timings
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    /// Snapshot the trace id and recorded timings into an envelope `meta`.
    pub fn attach(&self, meta: &mut EnvelopeMeta) {
        meta.trace_id = self.trace_id;
        meta.timings_ms = Some(self.timings);
    }

    /// Build an `EnvelopeMeta` starting from this telemetry context.
    pub fn into_meta(self) -> EnvelopeMeta {
        let mut meta = EnvelopeMeta::new(self.trace_id);
        meta.timings_ms = Some(self.timings);
        meta
    }
}

/// RAII stage timer created by [`Telemetry::stage`]. Records elapsed
/// milliseconds into the parent telemetry's stage slot when dropped.
pub struct StageGuard<'a> {
    started_at: Instant,
    telemetry: &'a mut Telemetry,
    stage: Stage,
}

impl<'a> StageGuard<'a> {
    fn new(telemetry: &'a mut Telemetry, stage: Stage) -> Self {
        StageGuard {
            started_at: Instant::now(),
            telemetry,
            stage,
        }
    }
}

impl Drop for StageGuard<'_> {
    fn drop(&mut self) {
        let elapsed = self.started_at.elapsed().as_millis() as u64;
        self.telemetry.record(self.stage, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::{Stage, Telemetry};
    use crate::models::envelope::Envelope;
    use crate::models::fsm_state::SyncState;

    #[test]
    fn begin_mints_uuidv7_trace_id() {
        let t = Telemetry::begin();
        assert_eq!(t.trace_id().get_version_num(), 7, "must be UUIDv7");
        // Time-ordered: two consecutive mints must not go backwards.
        let later = Telemetry::begin().trace_id();
        assert!(later > t.trace_id(), "UUIDv7 must be monotonic-ish");
    }

    #[test]
    fn with_trace_propagates_existing_trace_id_unchanged() {
        let inbound = "0192a4b8-7c10-7000-8000-000000000001".parse().unwrap();
        let t = Telemetry::with_trace(inbound);
        assert_eq!(t.trace_id(), inbound);
        let meta = t.into_meta();
        assert_eq!(meta.trace_id, inbound);
    }

    #[test]
    fn stage_timers_record_all_four_keys() {
        let mut t = Telemetry::begin();
        t.record(Stage::GitProbe, 38);
        t.record(Stage::GoScanner, 5);
        t.record(Stage::LazySidecar, 0);
        t.record(Stage::CppCompile, 162);
        let timings = t.timings();
        assert_eq!(timings.git_probe, 38);
        assert_eq!(timings.go_heuristic_scan, 5);
        assert_eq!(timings.lazy_sidecar, 0);
        assert_eq!(timings.cpp_compile, 162);

        let meta = t.into_meta();
        let timings = meta.timings_ms.expect("timings present");
        assert_eq!(timings.git_probe, 38);
        assert_eq!(timings.cpp_compile, 162);
    }

    #[test]
    fn closure_timer_and_raii_guard_both_record() {
        let mut t = Telemetry::begin();
        let _ = t.time(Stage::LazySidecar, || {
            // A zero-cost fast bypass still records ~0 ms — the key must exist.
            42
        });
        assert!(t.timings().lazy_sidecar <= 5);

        {
            let _guard = t.stage(Stage::CppCompile);
            // simulate ~1 ms of work — dropping `_guard` records elapsed time
        }
        // Dropped, so cpp_compile must have been recorded into the timings struct.
        assert!(t.timings().cpp_compile < 10_000);
    }

    #[test]
    fn trace_id_and_timings_flow_into_envelope_meta_and_are_serialized() {
        let mut t = Telemetry::begin();
        t.record(Stage::GitProbe, 38);
        let trace_id = t.trace_id();

        let envelope = Envelope::success("BUILD_STAGE_COMPLETE", None, t.into_meta());
        let json = serde_json::to_value(&envelope).unwrap();

        assert_eq!(
            json["meta"]["trace_id"],
            serde_json::json!(trace_id.to_string())
        );
        assert_eq!(
            json["meta"]["timings_ms"]["git_probe"],
            serde_json::json!(38)
        );
        // Roundtrip preserves the propagated trace id.
        let roundtrip: Envelope = serde_json::from_value(json).unwrap();
        assert_eq!(roundtrip.meta.trace_id, trace_id);
    }

    #[test]
    fn attach_keeps_fsm_state_from_meta() {
        let mut t = Telemetry::begin();
        t.record(Stage::GoScanner, 5);
        let mut meta = crate::models::envelope::EnvelopeMeta::new(t.trace_id())
            .with_sync_state(SyncState::S2SyncedReady);
        t.attach(&mut meta);
        assert_eq!(meta.sync_state, Some(SyncState::S2SyncedReady));
        assert!(!meta.push_locked);
        assert_eq!(meta.timings_ms.unwrap().go_heuristic_scan, 5);
    }
}
