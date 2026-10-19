# ADR-004: Lazy-Loaded Pyplot / Mermaid Figure Sidecars

- **Status:** Accepted
- **Date:** 2026-10-19
- **Phase:** 1 — Monorepo Scaffolding, ADRs & Rust Control Plane Core

## Context

Scientific LaTeX users keep wanting to inline figures two ways:

1. `\begin{pyplot}…\end{pyplot}` — tiny matplotlib scripts to render one-off plots, avoiding a manual "run notebook, export PNG, rename, link" loop.
2. `\begin{mermaid}…\end{mermaid}` — diagrams authored directly in text.

Shipping both toolchains up front (a standalone Python distribution, `matplotlib`, `numpy`, a headless browser, a Mermaid renderer, and their transitive runtime files) would push the installer past 1 GB and require signed update channels for consumers who never touch a single figure — the exact opposite of the local-first, small-footprint design story TexCraft is built on.

## Decision

Figure toolchains are **lazy-loaded sidecars** that only ever provision on their first actual use:

1. **Detection only, no provisioning.** Stage `B1` (`worker/go-scanner`) reports `has_pyplot` / `has_mermaid` flags only; the control plane reads flags and does not provision anything.
2. **Fast bypass.** If both flags are false, `B2` completes in 0 ms and zero external runtimes are touched, spawned, or downloaded.
3. **Content-addressed cache.** Each figure block computes `H_block = BLAKE3(block_source ∥ preamble_font_config ∥ target_width)`. On a cache hit against `.texcraft_cache/figures/<H_block>.pdf`, **no worker process is spawned at all** — the AST rewriter substitutes `\includegraphics` and moves on. Slightly stale files in the cache are semantically invisible because the hash is content-derived.
4. **On first cache miss, provision once** via `bootstrap_manager.rs`:
   - Pyplot: standalone portable Python + `matplotlib` + `numpy` under `~/.texcraft/runtimes/python/`.
   - Mermaid: a lightweight renderer under `~/.texcraft/runtimes/mermaid/`.
5. **Warm persistent workers.** `pyplot_bridge.rs` keeps a Python process alive; `mermaid_bridge.rs` keeps a headless renderer alive. Configuration guarantees `pdf.fonttype = 42` (TrueType/Type 1 vector fonts only; **never Type 3 bitmap fonts**) plus LaTeX-style serif typography, so figures blend into the typeset page as real vector geometry.
6. **Line-preserving AST rewrite.** `ast_rewriter.rs` replaces the entire environment with one `\includegraphics{…}` line and pads the rest of the block's original lines with `% TEXCRAFT_LINE_PAD` comment lines so the rewritten buffer has exactly the same line count as the user's editor buffer. SyncTeX inverse mappings `(page, x_pt, y_pt) → file line` therefore stay 100 % accurate against what the user sees.

## Rejected Alternatives

- **Bundling all figure toolchains in the installer:** Rejected. +1 GB for a default userscripted-truth is exactly the bloat the product set out to avoid.
- **Subprocess-per-figure (cold spawn):** Rejected. Python interpreter + matplotlib import cold-start latency is multi-second; in a "<150 ms per figure" compile budget it's a non-starter for active editing.
- **Rendering figures as serverless cloud calls:** Rejected on local-first grounds (documents must not leave the machine) and offline reliability grounds.
- **Raster PNG output:** Rejected. Vector PDF output preserves infinitely sharp fonts and geometry in both print and on-screen zoom, which is essential for a product whose goal is published-book-quality output.
- **Lossy code rewriting without padding:** Rejected. Rewriting the TeX buffer without keeping every original line in place would desynchronize SyncTeX line mappings from the editor's line numbers, breaking click-to-jump.

## Consequences

- **Positive:** Figure-using authors pay a one-time provisioning cost per sidecar runtime; editors who never produce figures pay nothing.
- **Positive:** Second and subsequent figure renderings hit a warm persistent child process, meeting the <150 ms budget; cache hits are <1 ms.
- **Positive:** SyncTeX fidelity is guaranteed because rewrite output is line-count-preserving; click-to-jump and hover coordinates never drift.
- **Negative:** First-ever figure render on a cold machine incurs runtime-provisioning latency; this is surfaced as an explicit progress state, not a build failure.
- **Negative:** The `~/.texcraft/runtimes/` directory must be versioned per runtime upgrade; `bootstrap_manager.rs` pins toolchain versions and discards old dirs on major-version change.
