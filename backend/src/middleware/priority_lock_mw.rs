//! Priority Lock gatekeeper placeholder (Phase 5): rejects pushes when state == S3 with ERR_PRIORITY_LOCK_REMOTE_AHEAD.
//!
//! Intentionally empty: this module is implemented in its owning phase
//! (see AGENT_HANDOFF_SPEC.md §5) to avoid reaching across phase gates.

/// Placeholder so the module is non-trivially compiled; Phase N replaces it.
pub fn placeholder() -> &'static str {
    "stub — implemented by a later phase"
}
