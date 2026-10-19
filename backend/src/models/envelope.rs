//! Unified JSON Response Envelope (Spec §1.4) — the single control-plane wire shape
//! shared by Rust, Go, and C++. The authoritative JSON Schema lives at
//! `infra/schemas/ipc_envelope.schema.json`; the Rust shape here is pinned first
//! (Phase 1) and the Go/C++ serializers must match it (DR-005 / ADR-005).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::models::fsm_state::SyncState;

/// Canonical control-plane codes (§1.4).
pub mod codes {
    pub const BUILD_STAGE_COMPLETE: &str = "BUILD_STAGE_COMPLETE";
    pub const ERR_PRIORITY_LOCK_REMOTE_AHEAD: &str = "ERR_PRIORITY_LOCK_REMOTE_AHEAD";
    /// Standard remediation hint telling the client to drive `S3_RECONCILE`.
    pub const REMEDIATION_TRIGGER_S3_RECONCILE: &str = "TRIGGER_S3_RECONCILE";
    pub const MSG_PRIORITY_LOCK_REMOTE_AHEAD: &str =
        "Push blocked by Priority Protection: remote repository has newer commits. Run Pull & Reconcile first.";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Error,
}

/// Stage latencies in milliseconds (§1.4 `meta.timings_ms`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingsMs {
    #[serde(default)]
    pub git_probe: u64,
    #[serde(default)]
    pub go_heuristic_scan: u64,
    #[serde(default)]
    pub lazy_sidecar: u64,
    #[serde(default)]
    pub cpp_compile: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// `error` object: human-readable message plus a machine-usable remediation hint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

/// `meta` object. Fixed keys are strongly typed; any worker-specific key
/// (e.g. `remote_head_sha`, `local_base_sha`) flows through `extra` via
/// `#[serde(flatten)]` so the envelope stays forward-compatible across languages.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeMeta {
    /// UUIDv7, minted by `middleware::telemetry_mw` on the first hop into `backend/`
    /// and propagated unchanged across every worker boundary (§1.3).
    pub trace_id: uuid::Uuid,
    /// Mirrors the Plane A FSM state; omitted while outside a sync context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_state: Option<SyncState>,
    /// True whenever pushes are not usable right now (§3.1 push permissions).
    #[serde(default)]
    pub push_locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timings_ms: Option<TimingsMs>,
    /// Catch-all for worker-contributed metadata keys.
    #[serde(flatten, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl EnvelopeMeta {
    pub fn new(trace_id: uuid::Uuid) -> Self {
        EnvelopeMeta {
            trace_id,
            ..Default::default()
        }
    }

    /// Attach stage timings collected by `middleware::telemetry_mw`.
    pub fn with_timings(mut self, timings: TimingsMs) -> Self {
        self.timings_ms = Some(timings);
        self
    }

    /// Mark the plane-A FSM state and derive `push_locked` from it — the FSM enum is
    /// the single source of truth; envelope writers never set `push_locked` by hand.
    pub fn with_sync_state(mut self, state: SyncState) -> Self {
        self.sync_state = Some(state);
        self.push_locked = state.push_locked();
        self
    }

    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

/// The Unified JSON Response Envelope itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub status: Status,
    pub code: String,
    /// Always serialized — `null` when no payload, matching the spec envelopes exactly.
    pub data: Option<Value>,
    pub meta: EnvelopeMeta,
    /// Always serialized — `null` on success, matching the spec envelopes exactly.
    pub error: Option<EnvelopeError>,
}

impl Envelope {
    pub fn success(code: impl Into<String>, data: Option<Value>, meta: EnvelopeMeta) -> Self {
        Envelope {
            status: Status::Success,
            code: code.into(),
            data,
            meta,
            error: None,
        }
    }

    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        remediation: Option<String>,
        meta: EnvelopeMeta,
    ) -> Self {
        Envelope {
            status: Status::Error,
            code: code.into(),
            data: None,
            meta,
            error: Some(EnvelopeError {
                message: message.into(),
                remediation,
            }),
        }
    }

    /// Exact Priority Protection rejection envelope from §1.4. Middleware
    /// (`priority_lock_mw.rs`, Phase 5) routes every blocked push through here.
    pub fn priority_lock_rejected(
        remote_head_sha: impl AsRef<str>,
        local_base_sha: impl AsRef<str>,
        meta: EnvelopeMeta,
    ) -> Self {
        let mut meta = meta
            .with_sync_state(SyncState::S3PriorityLocked)
            .with_extra("remote_head_sha", remote_head_sha.as_ref())
            .with_extra("local_base_sha", local_base_sha.as_ref());
        meta.push_locked = true;
        Envelope::error(
            codes::ERR_PRIORITY_LOCK_REMOTE_AHEAD,
            codes::MSG_PRIORITY_LOCK_REMOTE_AHEAD,
            Some(codes::REMEDIATION_TRIGGER_S3_RECONCILE.to_string()),
            meta,
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self.status, Status::Success)
    }
}

#[cfg(test)]
mod tests {
    use super::codes;
    use super::{Envelope, EnvelopeMeta, Status, TimingsMs};
    use crate::models::fsm_state::SyncState;
    use serde_json::json;

    fn trace() -> uuid::Uuid {
        // Deterministic fixture used by the golden envelopes below.
        "0192a4b8-7c10-7000-8000-000000000001".parse().unwrap()
    }

    #[test]
    fn success_envelope_matches_spec_example_byte_for_key() {
        let envelope = Envelope::success(
            codes::BUILD_STAGE_COMPLETE,
            Some(json!({
                "stage": "B3_CPP_WARM_COMPILE",
                "shm_descriptor": "/texcraft_shm_buf_01",
                "pdf_byte_len": 418920,
                "synctex_path": ".texcraft_cache/main.synctex.gz"
            })),
            EnvelopeMeta::new(trace())
                .with_sync_state(SyncState::S2SyncedReady)
                .with_timings(TimingsMs {
                    git_probe: 38,
                    go_heuristic_scan: 5,
                    lazy_sidecar: 0,
                    cpp_compile: 162,
                }),
        );

        let serialized = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            serialized,
            json!({
                "status": "success",
                "code": "BUILD_STAGE_COMPLETE",
                "data": {
                    "stage": "B3_CPP_WARM_COMPILE",
                    "shm_descriptor": "/texcraft_shm_buf_01",
                    "pdf_byte_len": 418920,
                    "synctex_path": ".texcraft_cache/main.synctex.gz"
                },
                "meta": {
                    "trace_id": "0192a4b8-7c10-7000-8000-000000000001",
                    "sync_state": "S2_SYNCED_READY",
                    "push_locked": false,
                    "timings_ms": {
                        "git_probe": 38,
                        "go_heuristic_scan": 5,
                        "lazy_sidecar": 0,
                        "cpp_compile": 162
                    }
                },
                "error": null
            })
        );
    }

    #[test]
    fn priority_lock_error_envelope_matches_spec_example() {
        let envelope = Envelope::priority_lock_rejected(
            "a1b2c3d4e5f6",
            "998877665544",
            EnvelopeMeta::new("0192a4b8-7c10-7000-8000-000000000002".parse().unwrap()),
        );

        let serialized = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            serialized,
            json!({
                "status": "error",
                "code": "ERR_PRIORITY_LOCK_REMOTE_AHEAD",
                "data": null,
                "meta": {
                    "trace_id": "0192a4b8-7c10-7000-8000-000000000002",
                    "sync_state": "S3_PRIORITY_LOCKED",
                    "push_locked": true,
                    "remote_head_sha": "a1b2c3d4e5f6",
                    "local_base_sha": "998877665544"
                },
                "error": {
                    "message": "Push blocked by Priority Protection: remote repository has newer commits. Run Pull & Reconcile first.",
                    "remediation": "TRIGGER_S3_RECONCILE"
                }
            })
        );
        assert_eq!(envelope.status, Status::Error);
        assert!(!envelope.is_success());
    }

    #[test]
    fn envelope_json_roundtrip_preserves_all_fields() {
        let envelope = Envelope::success(
            codes::BUILD_STAGE_COMPLETE,
            Some(json!({"stage": "B1_GO_HEURISTIC_SCAN"})),
            EnvelopeMeta::new(trace())
                .with_sync_state(SyncState::S3PriorityLocked)
                .with_timings(TimingsMs::default())
                .with_extra("worker_pid", 4242),
        );

        let text = serde_json::to_string(&envelope).unwrap();
        let back: Envelope = serde_json::from_str(&text).unwrap();
        assert_eq!(envelope, back);
    }

    #[test]
    fn sync_state_drives_push_locked_flag() {
        let meta = EnvelopeMeta::new(trace()).with_sync_state(SyncState::S3PriorityLocked);
        let envelope = Envelope::success(codes::BUILD_STAGE_COMPLETE, None, meta);
        let serialized = serde_json::to_value(&envelope).unwrap();
        assert_eq!(serialized["meta"]["push_locked"], json!(true));
        assert_eq!(
            serialized["meta"]["sync_state"],
            json!("S3_PRIORITY_LOCKED")
        );
    }
}
