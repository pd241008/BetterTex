# ADR-002: GitHub Sync Priority Lock State Machine (S0–S4)

- **Status:** Accepted
- **Date:** 2026-10-19
- **Phase:** 1 — Monorepo Scaffolding, ADRs & Rust Control Plane Core

## Context

TexCraft collaboration is built on a plain user-owned GitHub repository — no server-side merge service, no real-time CRDT session, no privileged bot. That means multiple origin points (the user's own clones and any collaborator) can commit concurrently, and `origin/HEAD` can advance at any moment while a user is mid-edit.

Naïve approaches fail in well-defined ways:

- Optimistically pushing over a moved `origin/HEAD` (or force-pushing) silently destroys collaborator work.
- Pulling automatically whenever the remote moves will happily clobber a dirty working tree.
- Doing nothing and letting the user manually manage merge state produces stale-editor races that paint the app as untrustworthy.

## Decision

The Rust control plane owns a deterministic five-state machine plus one reconciliation sub-state, implemented in `backend/src/services/git_priority_fsm.rs`, with states and invariants modeled in `backend/src/models/fsm_state.rs`:

| State | Meaning | Push permission |
| :--- | :--- | :--- |
| `S0_BOOT_PROBE` | On workspace open: credential probe from `auth/`, then `git ls-remote origin HEAD` with a **1500 ms timeout**, comparing `remote_head_sha` vs `local_base_sha`. | LOCKED |
| `S1_AUTO_PULL_FF` | Remote moved and the working tree is clean with zero local commits: automatic `git merge --ff-only FETCH_HEAD`, then Build Pipeline `B1`. | LOCKED |
| `S2_SYNCED_READY` | `remote_head_sha == local_base_sha`. Normal authoring; background fiber polls `git ls-remote` every 30 s. | **UNLOCKED** |
| `S3_PRIORITY_LOCKED` | Remote moved while `(working_tree_dirty == true OR local_ahead_count > 0)`. **Priority Protection triggered**: `priority_lock_mw.rs` rejects every push with `ERR_PRIORITY_LOCK_REMOTE_AHEAD`, and the UI replaces Push with **Pull & Reconcile**. | HARD_LOCKED |
| `S3_RECONCILE` | User-invoked: `git stash push -u` → fetch & rebase onto `origin/MAIN` → `git stash pop`; on conflict markers, open a 3-way conflict view. Only after a clean rebase does `local_base_sha = remote_head_sha` and the state return to `S2`. | LOCKED |
| `S4_ATOMIC_PUSH` | Invoked only from `S2`. **Pre-Push Gate:** re-run `git ls-remote origin HEAD` immediately before pushing; if `remote_head_sha` moved since the last poll, abort to `S3`. Otherwise commit and `git push --force-with-lease`. | IN_FLIGHT (briefly) |

Key invariants:

1. **Pushing is possible only from `S2` through `S4`.** `S4` re-probes the remote atomically right before the push, so the window between "checked" and "pushed" is as small as the network allows, and `--force-with-lease` makes the push fail closed if a collaborator landed a commit in even that microsecond window.
2. **Dirty + remote-ahead ⇒ hard lock.** Funds of data that can be lost are guarded at the middleware layer, not by UI convention.
3. **Reconciliation is explicit, deterministic, and uses reversible primitives** (`stash push -u`, `rebase`, `stash pop`) so that a failed reconciliation leaves a restorable snapshot.
4. **Startup auto-pull is strictly fast-forward.** `git merge --ff-only` can never produce a merge commit or reorder the user's history; if it is impossible we fall to `S3` logic instead of improvising.

## Rejected Alternatives

- **Centralized CRDT server / hosted merge service:** Rejected. It creates an operational cost center, a privacy hotspot (user documents leaving the machine outside GitHub), and a second source of truth with no conflict-resolution semantics superior to Git itself.
- **Blind `git pull --rebase` on every remote change:** Rejected. Rebasing a dirty working tree silently, without a snapshot, can lose content on conflict mid-rebase.
- **Push with plain `--force`:** Forever rejected. This is exactly the silent-overwrite failure Priority Protection exists to prevent.
- **Lock-free warning toast (warn-but-allow push):** Rejected as the primary mechanism. Warnings are ignorable; state machines are not.

## Consequences

- **Positive:** No scenario exists in which a push can overwrite an unintegrated remote commit — the `S4` pre-push gate plus `--force-with-lease` make the race fail closed instead of silently succeeding.
- **Positive:** Reconciliation is always user-initiated and always snapshot-first, which satisfies the "never lose user content" guarantee without inventing a merge fabric.
- **Negative:** The 30 s background idle poll trades occasional staleness of the remote view for predictable network load; this is tunable.
- **Negative:** A moved `origin/HEAD` during a build can momentarily interrupt an in-flight pipeline; the FSM specifically resolves in favor of "block new pushes" pending the next reconcile rather than attempting interleaved merge-and-build.
- **Testing hook:** the Phase 5 verification gate will simulate two concurrent clones to prove: Clone A push → Clone B (with dirty working tree) lands in `S3_PRIORITY_LOCKED`, `POST /api/sync/push` is rejected by middleware with `ERR_PRIORITY_LOCK_REMOTE_AHEAD`, and `POST /api/sync/reconcile` restores `S2_SYNCED_READY`.
