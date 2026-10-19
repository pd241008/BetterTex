//! TexCraft lazy-modules — on-demand figure & diagram transpilers (ADR-004).
//! Phase 3 fills `bootstrap_manager`, `ast_rewriter`, `pyplot_bridge`, `mermaid_bridge`.

pub mod ast_rewriter;
pub mod bootstrap_manager;
pub mod mermaid_bridge;
pub mod pyplot_bridge;

/// Placeholder wiring assertion removed by Phase 3.
pub fn modules_registered() -> usize {
    4
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_compiles() {
        assert_eq!(super::modules_registered(), 4);
    }
}
