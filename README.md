# 🏭 TexCraft

### 🪶 Polyglot Local-First LaTeX IDE

A local-first Overleaf alternative: a **Rust control-plane orchestrator**, a **Go heuristic dependency scanner**, a **C++ warm TeX compilation daemon**, lazy-loaded **pyplot/mermaid figure sidecars**, and **GitHub auto-sync with Priority Protection** — built to the strict `Design-Dungeons` engineering constraints.
> [!IMPORTANT]
> **The hard problem: sync without a server.** Collaboration runs over the user's own GitHub repository, so `origin/HEAD` can move under you at any moment. TexCraft answers with a deterministic S0–S4 FSM whose `S3_PRIORITY_LOCKED` state hard-blocks pushes at the middleware layer — a race made impossible in code, not discouraged in UI copy.

---

## 🎯 What This Is

Scientific authors keep losing either fidelity or vibe to where their figures live. TexCraft keeps everything **local**: LaTeX bodies, generated vector-PDF figures, precompiled `.fmt` preamble dumps, and a bidirectional SyncTeX index all move through OS shared memory, not JSON serialization, so the compile loop stays sub-second and click-to-jump line coordinates stay 1:1 with the editor.

- **Plane A (sync)** — GitHub Priority-Lock state machine: automatic fast-forward pulls, 30 s remote polls, and a pre-push atomic gate (`S4`).
- **Plane B (build)** — Go scanner (B1) → lazy transpilers (B2) → warm C++ TeX daemon (B3), each stage BLAKE3-content-addressed so untouched work short-circuits.

| Property | Value |
| --- | --- |
| Status | `[Status: Building]` — Phases 1–2 gates green |
| Control plane | Unified JSON envelope (§1.4) over Unix sockets / named pipes |
| Data plane | POSIX `shm_open`/`mmap` or Win32 `CreateFileMapping`/`MapViewOfFile` |
| Stack | Rust · Go · C++20 · memmap2 · blake3 |

---

## ✨ Highlights

- **Priority Protection** — pushing is legally possible only from `S2_SYNCED_READY` after an atomic `git ls-remote` re-probe; every blocked push returns `ERR_PRIORITY_LOCK_REMOTE_AHEAD` with a one-click `TRIGGER_S3_RECONCILE` remediation.
- **4-Tier dependency resolution** — workspace `/dropin` → user registries → local BLAKE3 cache → heuristic CTAN inverted index (`algpseudocode.sty → algorithmicx`), with drop-in always superior and network always fetch-last.
- **Line-numbers-never-lie figure rewriting** — `\begin{pyplot}`/`\begin{mermaid}` blocks are replaced by `\includegraphics` with `% TEXCRAFT_LINE_PAD` filler so SyncTeX inverse maps stay accurate against every keystroke.
- **Zero-install bloat** — the portable Python / Mermaid toolchains are fetched only on the first cache miss, never for authors who don't use figures.

---

## 🧭 Navigation

| Section | Description |
| --- | --- |
| [Agent spec](./AGENT_HANDOFF_SPEC.md) | The authoritative build plan, state machines, and phase gates. |
| [ADRs](./docs/adr/ADR-001-polyglot-rust-go-cpp-split.md) | 5 decision records covering the polyglot split, FSM, resolver, sidecars, IPC planes. |
| [Architecture](./docs/architecture/) | State-machine spec + IPC wire protocol (schema: `infra/schemas/`). |
| [Backend](./backend/) | Rust control plane: FSM, telemetry, BLAKE3 DAG, shared-memory broker. |
| [Workers](./worker/) | Isolated `go-scanner`, `cpp-compiler`, `lazy-modules` engines. |
| [Auth](./auth/) | GitHub OAuth Device Flow + OS-keyring PAT service. |
| [Frontend](./frontend/) | Split-pane editor + SyncTeX GPU viewport + SyncStatusBar (Phase 5). |

---

## 🚀 Quickstart

```bash
# Build every toolchain and run all phase gates so far
./infra/scripts/build_all.sh

# Rust control plane (Phase 1 gate)
cargo test --workspace

# Go scanner (Phase 2 gate)
cd worker/go-scanner && go test -race ./...
go run ./cmd/scanner   # serves /tmp/texcraft_go_scanner.sock
```

---

## 🏗️ Layout

```text
frontend/   UI presentation layer — never touches Git credentials or worker sockets
auth/       isolated GitHub credential service (OAuth Device Flow, OS keyring)
backend/    Rust orchestrator — FSM, telemetry, BLAKE3 DAG, shm broker, worker supervision
worker/     isolated engines: go-scanner (B1), lazy-modules (B2), cpp-compiler (B3)
infra/      envelope JSON Schema, CTAN index generator, reproducible build scripts
docs/       ADRs + architecture specs (state machines, wire protocol)
```

Build phase order is enforced: Phase N+1 does not open until Phase N's gate passes (see spec §5).

---

## 📜 License

MIT — see [LICENSE](./LICENSE).

_Build log: [Design Dungeons](https://github.com/pd241008/Design-Dungeons) documents the playbook this repo follows; this repo is the evidence for it._
