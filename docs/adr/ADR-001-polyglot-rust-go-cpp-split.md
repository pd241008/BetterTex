# ADR-001: Polyglot Rust / Go / C++ Split

- **Status:** Accepted
- **Date:** 2026-10-19
- **Phase:** 1 — Monorepo Scaffolding, ADRs & Rust Control Plane Core

## Context

TexCraft is a locally aggregated pipeline of four very different workloads that share a single control plane:

1. **FSM safety & orchestration.** GitHub synchronization must follow a deterministic, audit-friendly state machine (S0–S4) with optimistic Priority Protection. Mistakes here corrupt user repositories. The state transitions are I/O-bound on Git probes and must be cheap, memory-safe, and leak-free under long-running daemon conditions.
2. **High-concurrency heuristic resolution.** Dependency scanning fans out over hundreds of small unitful tasks: lexing `\input`/`\include` trees, interrogating Tier 1–3 filesystems, and issuing concurrent network HEAD/GET calls against CTAN mirrors with SHA-256 checksum verification. Each resolution unit is short, often network-bound, and blocks ambient — Go's goroutine scheduler and first-class HTTP/read-write kernel poller handles this at near-zero marginal cost and is a well-trodden pattern.
3. **Native TeX macro compatibility.** The compilation engine must compile all legacy `.cls` files — Web2C/pdfTeX/XeTeX semantics are specified in legacy C/C++ code and only compile-and-integrate accurately against its control fields. Cross-reference Synctex indexing is also specibly implemented in C.
4. **Cross-process narrow IPC.** All workers must speak exactly one wire format and precisely over one transport per OS.

## Decision

- **Rust (`backend/`, `auth/`, `worker/lazy-modules/`):** Owns the control plane. The Git Priority Lock FSM, `BLAKE3` build DAG cache, shared-memory broker, worker lifecycle supervision, and all dispatch to workers. Rust's memory safety without GC keeps the orchestrator stable across days of runtime; the `unsafe`-free (or locally audited) `mmap` funnel in `shm_broker.rs` centralizes all data-plane plumbing.
- **Go (`worker/go-scanner/`):** Owns the heuristic dependency & AST scanner. Goroutines warm-scan `.tex` trees concurrently and implement the 4-Tier resolution (Drop-in → User → Cache → CTAN) with concurrent downloads. The Go process is a stateless worker: it may read the repository and caches, but **never mutates Git state** (Design-Dungeons §1.1).
- **C++ (`worker/cpp-compiler/`):** Owns the warm TeX daemon. Precompiled `.fmt` preamble dumps, Web2C/pdfTeX/XeTeX invocation, and the bidirectional SyncTeX R-tree index live here, keeping macro-expanded TeX internals in a single isolated process that can crash/restart without losing the control plane.

Every worker communicates via a strict boundary: **Unified JSON over Unix Domain Sockets / Named Pipes for control** and **OS shared memory (`shm_open`/`mmap` or Win32 `CreateFileMapping`) for data** (ADR-005).

## Rejected Alternatives

- **Pure-Rust TeX port (e.g., `tectonic`-style engine as a library):** Rejected as the primary engine. Legacy `.cls`/`.sty` files depend on exact Web2C/pdfTeX integer- and font-expansion semantics that Rust re-implementations do not bit-for-bit honor; subtle differences produce layout drift that is unacceptable for a LaTeX IDE. Tectonic remains an optional engine plugged behind `warm_tex_engine` but is not the norm.
- **Single-language orchestrator (all Rust):** Rejected. Rewriting the CTAN resolver in Rust forces either a heavy async runtime with hand-rolled connection pools or blocking thread spawns; no ecosystem advantage, and Go's standard-library HTTP + tar/gzip handling removes a class of supply surface we'd otherwise hand-maintain.
- **Single-language orchestrator (all Go):** Rejected. Go's GC pauses and the absence of precise shared-memory arena mapping primitives (without cgo) make the FSM/data-plane combination less safe than Rust.
- **Node/TypeScript backend:** Rejected. GUI-adjacent tooling advantages do not compensate for the loss of memory-safe native `mmap` and the need to embed system daemons with hard process-lifetime guarantees.

## Consequences

- **Positive:** Each language does only what its ecosystem best supports; workers are independently crash-isolated; per-path performance ceilings are reachable (warm C++ daemon, warm Go scanner, warm Rust control plane).
- **Positive:** The Unified JSON Envelope (§1.4) is a common, simple contract the small surface area of each worker can be verified against with plain unit tests.
- **Negative:** CI must maintain three toolchains plus CMake. `infra/scripts/build_all.sh` exists precisely for this.
- **Negative:** Cross-process tracing requires the middleware layer (§1.2) to inject/propagate `trace_id` at every boundary; the `telemetry_mw` primitive and worker-side equivalents are mandatory, not optional.
