//! AST Dependency Manifest (Spec §3.2 Stage B1) — the strongly-typed schema the
//! `worker/go-scanner` (Phase 2) returns inside a Unified JSON Envelope. Field
//! names below are wire names consumed by `build_controller.rs` and Phase 3's
//! `lazy-modules`; keep them stable when `worker/go-scanner/internal/models/.go`
//! lands.

use serde::{Deserialize, Serialize};

/// One detected figure block (`\begin{pyplot}` / `\begin{mermaid}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FigureBlock {
    /// "pyplot" or "mermaid"
    pub kind: String,
    /// Inclusive 1-based line numbers of `\begin{...}` and `\end{...}`.
    pub start_line: u64,
    pub end_line: u64,
    /// Column of the `\begin` token on `start_line` (byte offset).
    pub start_col: u64,
    /// BLAKE3(block_source) — extended by Phase 3 to include font config & width.
    pub block_hash: String,
}

/// Input-tree file entry discovered during the concurrent scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannedFile {
    /// Workspace-relative path (empty = scan root itself).
    pub rel_path: String,
    /// Packages the file requires via `\documentclass` + `\usepackage{...}`.
    pub packages: Vec<String>,
    /// Bibliography files referenced via `\bibliography{}` / `\addbibresource{}`.
    pub bib_files: Vec<String>,
    /// Line numbers of `\input{}` / `\include{}` references into other files.
    pub includes: Vec<u64>,
}

/// Four-tier resolution record for a single required file/package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedTier {
    /// Tier 1 — `<project_root>/dropin/` or project root.
    DropIn,
    /// Tier 2 — user custom local/remote source registries.
    UserRegistry,
    /// Tier 3 — `~/.texcraft/cache/ctan/` content-addressed cache.
    LocalCtanCache,
    /// Tier 4 — heuristic CTAN remote resolver (downloaded this build).
    CtanRemote,
}

/// The final B1 artifact handed to B2/B3.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DependencyManifest {
    pub root_tex: String,
    pub scanned_files: Vec<ScannedFile>,
    pub figure_blocks: Vec<FigureBlock>,
    pub has_pyplot: bool,
    pub has_mermaid: bool,
    /// (package/file → tier that satisfied it): proves Tier-1 override behavior.
    pub resolutions: Vec<(String, ResolvedTier)>,
    /// BLAKE3 of the preamble (everything before `\begin{document}`) — feeds the
    /// CPP daemon's `.fmt` warm cache key in B3.
    pub preamble_hash: Option<String>,
}
