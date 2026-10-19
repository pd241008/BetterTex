//! Strict layering (§1.2): Routes → Middleware → Controllers → Services → Models,
//! declared as inline `mod` blocks so the on-disk tree matches Spec §2 exactly.
//!
//! Phase 1 implements: `models::envelope`, `models::fsm_state`,
//! `middleware::telemetry_mw`, `services::shm_broker`. All other modules are
//! deliberate stubs to be filled by their owning phase (see AGENT_HANDOFF_SPEC.md §5).

// Phase-1 types are exercised by unit tests now and consumers (controllers,
// worker dispatch) arrive in Phases 2-5; silence dead_code until then.
#![allow(dead_code)]

mod models {
    pub mod envelope;
    pub mod fsm_state;
    pub mod manifest;
}

mod middleware {
    pub mod priority_lock_mw;
    pub mod telemetry_mw;
}

mod controllers {
    pub mod build_controller;
    pub mod source_controller;
    pub mod sync_controller;
}

mod services {
    pub mod dag_cache_service;
    pub mod git_priority_fsm;
    pub mod shm_broker;
    pub mod worker_supervisor;
}

mod routes {
    pub mod command_routes;
    pub mod ws_events;
}

fn main() {
    println!(
        "texcraft-backend {} — control plane wired (Phase 1); worker daemons arrive in Phases 2-4.",
        env!("CARGO_PKG_VERSION")
    );
}
