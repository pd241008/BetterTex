# ADR-003: Go Heuristic CTAN and Drop-in Resolver (4-Tier)

- **Status:** Accepted
- **Date:** 2026-10-19
- **Phase:** 1 — Monorepo Scaffolding, ADRs & Rust Control Plane Core

## Context

LaTeX documents declare dependencies through free-form macros (`\documentclass`, `\usepackage`, `\bibliography`, `\addbibresource`, `\input`, `\include`) for which there is no machine-readable package manifest. A local-first IDE must still resolve unknown dependencies (e.g., `\usepackage{algpseudocode}` maps to the CTAN `algorithmicx` package) and must do so without a second copy of all classes sitting inside a container image.

An added constraint: in workshops and pair-programming sessions, the fastest legal path to a fix is for a user to drop a corrected `.sty`/`.cls`/`.bib` file into the workspace, and the resolver must honor it *before* any repository- or network-level variant.

## Decision

The Go scanner (`worker/go-scanner`) is the sole owner of dependency resolution and enforces a strict 4-tier priority order. Every required file/package is resolved against the tiers in order and stops at the first match:

1. **Tier 1 — Workspace Drop-In Directory** (`<project_root>/dropin/` and `<project_root>/`): user-dropped `.tex`, `.sty`, `.cls`, `.bst`, `.bib`. Highest priority, because the user put it there deliberately.
2. **Tier 2 — User Custom Source Registries**: per-user local template directories or Git/HTTP registries registered through `SourceRegistryModal` (Phase 5), which records order-preserving absolute paths that the control plane passes as `user_source_dirs[]` in every `ScanRequest`.
3. **Tier 3 — Local Content-Addressed CTAN Cache** (`~/.texcraft/cache/ctan/`): keyed by package digest, so repeated builds never re-download.
4. **Tier 4 — Heuristic CTAN Remote Resolver**: an embedded inverted index (`ctan_heuristic.go`) mapping individual `.sty`/`.cls` filenames to their parent CTAN archive (e.g., `algpseudocode.sty` → `algorithmicx`). Downloads happen concurrently, are verified against SHA-256 checksums, and are extracted into Tier 3 for future probes.

The index is maintained by `infra/scripts/generate_ctan_index.go`, refreshed as a contract artifact so offline Tier-4 lookups (index-only) still resolve correctly without touching the network.

To hit Tier-3 degradation cost in the same pass as the scan, Go lexes the `\input`/`\include` tree with **concurrent goroutines**, each goroutine handling exactly one recursive document file; per-file extraction covers packages, the document class, bibliographies, and embedded figure blocks (`\begin{pyplot}`/`\begin{mermaid}`) with their start/end line spans and raw `BLAKE3` hashes, so the downstream `B2` stage can cache figures and the `B3` C++ daemon can compute preamble hashes from the same pass.

## Rejected Alternatives

- **Ship a full vendored TeX tree:** Rejected. It would relieve the resolver briefly but bloats the installer by hundreds of MB, which violates the zero-install-bloat goal the product is built around.
- **Sandboxed container with full TeX Live (`tectonic` full-bundle style downloads):** Rejected as the default path because it needs the network on first build and hides the 4-tier layering; the same bundle logic replaces our hash-keyed cache with a single opaque blob we can't incrementally update.
- **Parse-only resolver (no network tier):** Rejected. Without Tier 4, `\usepackage{algpseudocode}` fails on a clean machine before ever showing the user a helpful UI hint.
- **Rust-resolver-in-backend:** Rejected. It would force hand-rolled concurrency/high-priority network pools into the control-plane binary and mix boundary responsibility (Design-Dungeons §1.1); Go's standard library and goroutine scheduler already solve this exactly this problem.

## Consequences

- **Positive:** User intent (drop-in files) always wins — no "reached into a repository cache and overwrote my fix" behavior is possible by design.
- **Positive:** Deterministic tier precedence means the same graph input always produces the same `DependencyManifest`, which is what makes content-addressed figure caching in `B2` and warm preamble caching in `B3` valid to build on.
- **Positive:** Concurrent goroutine scanning keeps walk-and-resolve in the tens of milliseconds even on large trees; `go_scan_ms` telemetry verifies it.
- **Negative:** Registry misconfiguration (bad Tier-2 path) or a stale Tier-3 cache leaves a dependency unresolvable; failure surfaces as an envelope error with a remediation hint rather than a frame swap.
- **Negative:** The CTAN index must be regenerated on TeX Live package additions or the Tier-4 fallback misses newly added archives; the generate script is idempotent and cheap to rerun.
