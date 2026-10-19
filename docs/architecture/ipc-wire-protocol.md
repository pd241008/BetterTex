# TexCraft Architecture — IPC Wire Protocol

- **Status:** Accepted
- **Date:** 2026-10-19
- **Source:** Master Agent Spec §1.4 / §1.5 (`AGENT_HANDOFF_SPEC.md`)
- **Authoritative schema:** `infra/schemas/ipc_envelope.schema.json`
- **Decision rationale:** `docs/adr/ADR-005-shared-memory-ipc-and-unified-json.md`

## Control plane — Unified JSON over Unix socket / Named pipe

- **Transports:** Unix Domain Sockets on POSIX; Named Pipes on Win32.
- **Endpoints:**
  - Go scanner: `/tmp/texcraft_go_scanner.sock` (`$TEXCRAFT_SCANNER_SOCK` overrides; `worker/go-scanner/internal/routes/ipc_router.go`)
  - Rust control plane: control-plane listeners under `backend/src/routes/` (Phases 2–5)
  - C++ daemon: `worker/cpp-compiler/include/ipc_server.hpp` (Phase 4)
- **Framing:** newline-delimited JSON — one `ScanRequest`/`CompileCommand` per line, one `Envelope` per line. Single connection can multiplex sequential requests simply; each request carries its own `meta.trace_id`.

### Envelope shape (§1.4)

```json
{
  "status": "success" | "error",
  "code": "<MACHINE_CODE>",
  "data": { ... } | null,
  "meta": {
    "trace_id": "<uuidv7>",
    "sync_state": "<S0..S4+R>",   // optional outside sync contexts
    "push_locked": <bool>,
    "timings_ms": { "git_probe": 0, "go_heuristic_scan": 0, "lazy_sidecar": 0, "cpp_compile": 0 },
    "...": "worker-specific keys flattened into the same object"
  },
  "error": { "message": "...", "remediation": "..." } | null
}
```

Rules enforced by unit tests on all three language implementations:

1. `data` and `error` are **always serialized** — literal `null`, never omitted.
2. `meta.trace_id` is always present in worker responses (UUIDv7, §1.3).
3. `timings_ms` keys exist for every stage with a non-negative integer (zero allowed).
4. `sync_state`/`push_locked` echo the Plane A FSM; `push_locked == true` in every state except `S2_SYNCED_READY` (ADR-002).
5. Worker-specific metadata keys (`remote_head_sha`, `shm_descriptor`, …) ride in the same `meta` object, never in separate top-level fields.

### Wire names pinned by Phase 1/2 tests

- FSM states: `S0_BOOT_PROBE`, `S1_AUTO_PULL_FF`, `S2_SYNCED_READY`, `S3_PRIORITY_LOCKED`, `S3_RECONCILE`, `S4_ATOMIC_PUSH`.
- Build stages: `B1_GO_HEURISTIC_SCAN`, `B2_LAZY_MODULE_TRANSPILATION`, `B3_CPP_WARM_COMPILE`.
- Timing keys: `git_probe`, `go_heuristic_scan`, `lazy_sidecar`, `cpp_compile`.
- Canonical codes: `BUILD_STAGE_COMPLETE`, `SCAN_COMPLETE`, `ERR_PRIORITY_LOCK_REMOTE_AHEAD`, `ERR_SCAN_FAILED`, `ERR_BAD_PAYLOAD`; remediation `TRIGGER_S3_RECONCILE`.

## Data plane — named shared memory (never bytes-in-JSON)

- **Backing:** `shm_open`/`mmap` on POSIX (`/dev/shm`, name prefix `/texcraft_shm_buf_NN`), `CreateFileMapping`/`MapViewOfFile` on Win32. The Rust `backend/src/services/shm_broker.rs` is the single allocator; workers attach to existing named regions (`shm_arena.hpp`, Phase 4).
- **Layout ABI** (little-endian, pinned by `shm_broker.rs` and its tests; C++ must compile against it in Phase 4):

```text
off  size  field
  0     8  magic "TXCSHM\x01\x00"
  8     8  layout version = 1
 16     8  slot_size (payload bytes per slot; 1 MiB in the 64 MB default arena)
 24     8  slot_count
 32     8  push_pos  (monotonic; slot index = push_pos mod slot_count)
 40     8  pop_pos   (monotonic)
 48     4  cross-process spinlock (0 = free, 1 = held)
        12  reserved (zero)
 64   ...  slots: [state u32: 0 EMPTY | 1 WRITING | 2 FULL][len u32][pad u64][slot_size bytes]
```

- **Invariant:** a frame never spans two slots; `slot state` transitions `EMPTY → WRITING → FULL → EMPTY` under the spinlock, so torn reads are impossible.
- **Handoff descriptor:** control-plane `data.shm_descriptor` names the region (e.g. `/texcraft_shm_buf_01`) and `data.pdf_byte_len` gives the frame size — the payload itself is fetched from the arena, not serialized (§1.5 "never megabytes in JSON").
- **Fail-closed sizing:** pushing into a full ring returns an error (`RingFull`) rather than overwriting an unconsumed frame.
- **Environment override:** `TEXCRAFT_SHM_DIR` relocates arena backing files (CI sandboxes).

## Trace propagation contract

1. `backend/src/middleware/telemetry_mw.rs` mints a UUIDv7 when a request first enters `backend/`.
2. The id travels inside each worker request (`ScanRequest.trace_id` etc.); a worker that receives no id mints its own (Go `middleware.InjectTrace`).
3. Every worker response echoes the id in `meta.trace_id`; timings accumulate per stage under `meta.timings_ms`.
