//! Strongly-typed `SyncState` (S0–S4 + S3R) and `BuildStage` (B1–B3) enums.
//!
//! Wire serializations match Spec §3 exactly, e.g. `"S2_SYNCED_READY"`, `"S3_RECONCILE"`.
//! Decision/rationale lives in `docs/adr/ADR-002-github-priority-lock-state-machine.md`.

use serde::{Deserialize, Serialize};

/// GitHub Synchronization & Priority Protection FSM (Plane A).
///
/// Push permission by state (`push_locked`):
/// - `LOCKED` for `S0` (probe) and `S1` (auto pull) — the app is syncing, not accepting pushes.
/// - **`UNLOCKED`** only for `S2_SYNCED_READY` — the sole push-legal entry state for `S4`.
/// - `HARD_LOCKED` for `S3_PRIORITY_LOCKED` (remote ahead + local dirt) — blocked in
///   `middleware/priority_lock_mw.rs`, not by UI convention.
/// - `S3_RECONCILE` stays locked until a clean reconcile lands back in `S2`.
/// - `S4_ATOMIC_PUSH` is internally "in flight"; concurrent pushes are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncState {
    #[serde(rename = "S0_BOOT_PROBE")]
    S0BootProbe,
    #[serde(rename = "S1_AUTO_PULL_FF")]
    S1AutoPullFf,
    #[serde(rename = "S2_SYNCED_READY")]
    S2SyncedReady,
    #[serde(rename = "S3_PRIORITY_LOCKED")]
    S3PriorityLocked,
    #[serde(rename = "S3_RECONCILE")]
    S3Reconcile,
    #[serde(rename = "S4_ATOMIC_PUSH")]
    S4AtomicPush,
}

impl SyncState {
    pub const fn as_str(self) -> &'static str {
        match self {
            SyncState::S0BootProbe => "S0_BOOT_PROBE",
            SyncState::S1AutoPullFf => "S1_AUTO_PULL_FF",
            SyncState::S2SyncedReady => "S2_SYNCED_READY",
            SyncState::S3PriorityLocked => "S3_PRIORITY_LOCKED",
            SyncState::S3Reconcile => "S3_RECONCILE",
            SyncState::S4AtomicPush => "S4_ATOMIC_PUSH",
        }
    }

    /// `true` unless the state is `S2_SYNCED_READY` (push UNLOCKED).
    /// `S4_ATOMIC_PUSH` locks concurrent pushes — one push at a time.
    pub const fn push_locked(self) -> bool {
        !matches!(self, SyncState::S2SyncedReady)
    }

    /// True while Priority Protection hard-blocks pushes.
    pub const fn priority_locked(self) -> bool {
        matches!(self, SyncState::S3PriorityLocked)
    }
}

/// Polyglot Build Pipeline stages (Plane B). Executed sequentially B1 → B2 → B3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildStage {
    #[serde(rename = "B1_GO_HEURISTIC_SCAN")]
    B1GoHeuristicScan,
    #[serde(rename = "B2_LAZY_MODULE_TRANSPILATION")]
    B2LazyModuleTranspilation,
    #[serde(rename = "B3_CPP_WARM_COMPILE")]
    B3CppWarmCompile,
}

impl BuildStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            BuildStage::B1GoHeuristicScan => "B1_GO_HEURISTIC_SCAN",
            BuildStage::B2LazyModuleTranspilation => "B2_LAZY_MODULE_TRANSPILATION",
            BuildStage::B3CppWarmCompile => "B3_CPP_WARM_COMPILE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildStage, SyncState};

    #[test]
    fn sync_state_wire_names_match_spec() {
        assert_eq!(SyncState::S0BootProbe.as_str(), "S0_BOOT_PROBE");
        assert_eq!(SyncState::S1AutoPullFf.as_str(), "S1_AUTO_PULL_FF");
        assert_eq!(SyncState::S2SyncedReady.as_str(), "S2_SYNCED_READY");
        assert_eq!(SyncState::S3PriorityLocked.as_str(), "S3_PRIORITY_LOCKED");
        assert_eq!(SyncState::S3Reconcile.as_str(), "S3_RECONCILE");
        assert_eq!(SyncState::S4AtomicPush.as_str(), "S4_ATOMIC_PUSH");
    }

    #[test]
    fn sync_state_serde_roundtrip() {
        for st in [
            SyncState::S0BootProbe,
            SyncState::S1AutoPullFf,
            SyncState::S2SyncedReady,
            SyncState::S3PriorityLocked,
            SyncState::S3Reconcile,
            SyncState::S4AtomicPush,
        ] {
            let json = serde_json::to_string(&st).expect("serialize SyncState");
            let back: SyncState = serde_json::from_str(&json).expect("deserialize SyncState");
            assert_eq!(st, back, "roundtrip for {}", st.as_str());
            assert!(json == format!("\"{}\"", st.as_str()));
        }
    }

    #[test]
    fn push_locking_matches_fsm_invariants() {
        assert!(SyncState::S0BootProbe.push_locked());
        assert!(SyncState::S1AutoPullFf.push_locked());
        assert!(!SyncState::S2SyncedReady.push_locked());
        assert!(SyncState::S3PriorityLocked.push_locked());
        assert!(SyncState::S3PriorityLocked.priority_locked());
        assert!(SyncState::S3Reconcile.push_locked());
        assert!(SyncState::S4AtomicPush.push_locked());
    }

    #[test]
    fn build_stage_wire_names_match_spec() {
        assert_eq!(
            BuildStage::B1GoHeuristicScan.as_str(),
            "B1_GO_HEURISTIC_SCAN"
        );
        assert_eq!(
            BuildStage::B2LazyModuleTranspilation.as_str(),
            "B2_LAZY_MODULE_TRANSPILATION"
        );
        assert_eq!(BuildStage::B3CppWarmCompile.as_str(), "B3_CPP_WARM_COMPILE");
    }
}
