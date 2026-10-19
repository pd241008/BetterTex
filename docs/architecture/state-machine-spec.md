# TexCraft Architecture — State Machine Specification

- **Status:** Accepted
- **Date:** 2026-10-19
- **Source:** Master Agent Spec §3 (`AGENT_HANDOFF_SPEC.md`)
- **Implementation targets:** `backend/src/services/git_priority_fsm.rs` (Plane A), `backend/src/controllers/build_controller.rs` (Plane B), `backend/src/models/fsm_state.rs` (enums)

TexCraft runs **two coupled state machines** inside the Rust control plane:

- **Plane A — GitHub Synchronization & Priority Protection FSM (`SyncState`)** — who owns the working tree right now.
- **Plane B — Polyglot Build Pipeline Stages (`BuildStage`)** — the B1 → B2 → B3 pipeline that translates a working-tree buffer into a compiled PDF.

Plane B is triggered by Plane A transitions (notably `S1` auto-pull), by filesystem events (file save, file drop-in), or by explicit user action.

---

## Plane A: `SyncState` (S0–S4 + S3R)

### State definitions

| State | Enum variant | Entry invariant | Behavior | Push permission |
| :--- | :--- | :--- | :--- | :--- |
| **S0** | `S0_BOOT_PROBE` | Workspace opened / app started. | Fetch credential from `auth/`; `git ls-remote origin HEAD` with **1500 ms timeout**; compare `remote_head_sha` vs `local_base_sha`. | LOCKED |
| **S1** | `S1_AUTO_PULL_FF` | `remote_head_sha != local_base_sha` AND `working_tree_dirty == false` AND `local_ahead_count == 0`. | `git merge --ff-only FETCH_HEAD`; unlock editor buffer; trigger Build Pipeline (`B1`). | LOCKED |
| **S2** | `S2_SYNCED_READY` | `remote_head_sha == local_base_sha` (tree may be clean or dirty). | Normal authoring. Background fiber polls `git ls-remote` every **30 s**. | **UNLOCKED** |
| **S3** | `S3_PRIORITY_LOCKED` | `remote_head_sha != local_base_sha` AND (`working_tree_dirty == true` OR `local_ahead_count > 0`). | Priority Protection triggered: `priority_lock_mw.rs` rejects all pushes with `ERR_PRIORITY_LOCK_REMOTE_AHEAD`; UI replaces Push with **Pull & Reconcile**. | **HARD_LOCKED** |
| **S3R** | `S3_RECONCILE` | User invokes Pull & Reconcile from `S3`. | 1. `git stash push -u` 2. fetch & rebase onto `origin/MAIN` 3. `git stash pop` 4. conflict-free → `local_base_sha = remote_head_sha`; conflicting markers → open 3-way conflict view. | LOCKED |
| **S4** | `S4_ATOMIC_PUSH` | User clicks Manual Sync/Push from `S2`. | **Pre-Push Gate:** re-run `git ls-remote origin HEAD`; if it moved since last poll → abort immediately to `S3`. Otherwise commit + `git push --force-with-lease`. | IN_FLIGHT (briefly) |

### Transition table (`S*` → `S*`)

```
S0_BOOT_PROBE   ─┬─ remote ahead, tree clean & local_ahead_count == 0 ──► S1_AUTO_PULL_FF
                 ├─ remote_head_sha == local_base_sha ─────────────────► S2_SYNCED_READY
                 └─ remote ahead && (dirty || ahead > 0) ──────────────► S3_PRIORITY_LOCKED

S1_AUTO_PULL_FF ──► ff-only merge succeeds ───────────────────────────► S2_SYNCED_READY
                    (failure falls back to S3 logic)

S2_SYNCED_READY ─┬─ poll detects remote advance ──────────────────────► S3_PRIORITY_LOCKED
                 └─ user Push ────────────────────────────────────────► S4_ATOMIC_PUSH

S3_PRIORITY_LOCKED ── user invokes Pull & Reconcile ─────────────────► S3_RECONCILE

S3_RECONCILE ────┬─ clean rebase, no markers ─────────────────────────► S2_SYNCED_READY
                 └─ unresolved conflict markers ──────────────────────► S3_RECONCILE (stay)

S4_ATOMIC_PUSH ──┬─ pre-push gate clean & push succeeded ─────────────► S2_SYNCED_READY
                 └─ pre-push gate detects remote movement ────────────► S3_PRIORITY_LOCKED
```

### Derived helpers

- `push_locked(state) -> bool` — true for every state except `S2_SYNCED_READY` (and `S4`, where push is already in flight and further pushes are rejected).
- `sync_state` is echoed in every Unified JSON Envelope `meta` (ADR-005).

---

## Plane B: `BuildStage` (B1–B3)

| Stage | Variant | Owner | Effect |
| :--- | :--- | :--- | :--- |
| **B1** | `B1_GO_HEURISTIC_SCAN` | `worker/go-scanner` | Concurrent Go lex of `\documentclass`/`\usepackage`/`\input`/`\include`/`\bibliography`/`\addbibresource` tree + figure blocks; 4-Tier resolution → `DependencyManifest`. |
| **B2** | `B2_LAZY_MODULE_TRANSPILATION` | `worker/lazy-modules` | Fast bypass (0 ms) when no figure blocks; otherwise BLAKE3 cache lookup → optional lazy runtime bootstrap → warm sidecars render vector PDF figures; line-preserving AST rewrite → SHM arena. |
| **B3** | `B3_CPP_WARM_COMPILE` | `worker/cpp-compiler` | Warm TeX daemon: preamble-hash keyed `.fmt` cache, conditional BibTeX/cross-ref passes, PDF written to SHM slot, SyncTeX R-tree updated; envelope returns `shm_descriptor`, `pdf_byte_len`, `synctex_path`. |

- **In-flight semantics:** a new build invalidates any in-flight generation with the same `trace_id`; `dag_cache_service.rs` keys every stage artifact by BLAKE3 content digests so unchanged stages short-circuit.
- **Telemetry:** stage durations land in `meta.timings_ms` — `git_probe` / `go_heuristic_scan` / `lazy_sidecar` / `cpp_compile` (§1.4).

---

## Cross-plane rules

1. Every plane transition is observable through the Unified JSON Envelope (`meta.sync_state`, `data.stage`).
2. `S3_PRIORITY_LOCKED` is the only state that **hard-blocks a control-plane request class** (push) at the middleware layer — this is enforced in code, not by UI convention.
3. Builds and pushes never overlap: `S4` performs its pre-push gate with builds idle, and an active build defers a user push until the current generation finishes.
