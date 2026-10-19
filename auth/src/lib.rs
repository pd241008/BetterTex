//! TexCraft Auth — isolated GitHub credential & token service (Design-Dungeons §1.1).
//! Strict layering: Routes -> Middleware -> Controllers -> Services -> Models.
//! Phase 5 implements the modules; skeletons live here per Spec §2 so the tree is exact.

pub mod routes {
    pub mod auth_routes;
}
pub mod middleware {
    pub mod scope_guard;
}
pub mod controllers {
    pub mod auth_controller;
}
pub mod services {
    pub mod github_oauth_service;
    pub mod keyring_service;
}
pub mod models {
    pub mod token_model;
}
