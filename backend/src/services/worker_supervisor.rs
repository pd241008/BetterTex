//! Worker supervisor placeholder: spawns & health-checks Go/C++/Lazy daemons (Phases 2-4).
//!
//! Intentionally empty: this module is implemented in its owning phase
//! (see AGENT_HANDOFF_SPEC.md §5) to avoid reaching across phase gates.

/// Placeholder so the module is non-trivially compiled; Phase N replaces it.
pub fn placeholder() -> &'static str {
    "stub — implemented by a later phase"
}
