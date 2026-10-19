# ADR-005: Shared Memory Data Plane and Unified JSON Control Plane

- **Status:** Accepted
- **Date:** 2026-10-19
- **Phase:** 1 — Monorepo Scaffolding, ADRs & Rust Control Plane Core

## Context

TexCraft passes two qualitatively different kinds of message between producer and consumer:

1. **Small, structured control messages** — commands, FSM transitions, dependency manifests, SyncTeX queries, telemetry timestamps. Kilobytes at most, need framing, versioning, and human debugging in one file, and change shape frequently and repeatedly as phases land.
2. **Raw bulk payloads** — raw `.tex` buffer snapshots, generated vector-PDF figures, precompiled `.fmt` memory dumps, and finished compiled PDF page buffers. Multi-hundred-KB to multi-MB frames, immutable per build pass, and must cross the `backend → bp_fork → cpp` process boundary over and over while keeping zero-copy gutters clean.

A single transport can't serve both well. JSON is autodisable for control but the *data* path needs pointer-zero work); byte-stream framing is needed when messages arrive off process boundaries.

## Decision

TexCraft **hard-separates** the planes (Design-Dungeons §1.5):

### Control Plane — Unified JSON over Unix Domain Socket / Named Pipe

- One single transport
- Rust IPC server (`backend/src/routes/`), Go's `scanner` listener, and the C++ daemon's `ipc_server.hpp` all bind a Unix Domain Socket (Windows: Named Pipe) and accept one request per frame
- Every response across Rust ↔ Go ↔ C++ matches the Unified JSON Envelope exactly (§1.4 / `infra/schemas/ipc_envelope.schema.json`): `status`, `code`, `data`, `meta.trace_id`, `meta.timings_ms`, `error.remediation[a]`
- `trace_id` (UUIDv7) is created by `backend/src/middleware/telemetry_mw.rs` on the first hop into `backend/` and propagated unchanged at every hop, so one build can be traced end-to-end across all three languages

### Data Plane — Named Shared Memory (`shm_open` / `mmap` or Win32 `CreateFileMapping` / `MapViewOfFile`)

- `backend/src/services/shm_broker.rs` owns allocation; a single 64 MB default arena (`member`-based slot ring in the mmap) readable and writable by all cooperating processes; workers that attach to a page attach by named region
- Frames are transferred via slot descriptors in the meta envelope (`shm_descriptor: "/texcraft_shm_buf_01"` + `pdf_byte_len`) — the buffer contents stay in OS pages and are never base64'd into a JSON string
- No envelope ever serializes megabytes of PDF, TeX, `.fmt`, figure bytes, or font bits
- Payload lifetime is tied to the broker's generation counter, so a crashed worker cannot leak a still-referenced frame

### Why not one protocol

An all-JSON transport can technically carry megabytes of base64, but the price is per-build ring kills and worker-build tracing; an all-shared-memory-covered floor (no JSON control path) would be error-prone for error & transition routing. The split stays and prices each plane with what it does best.

## Rejected Alternatives

- **All-JSON (bulk as base64 strings):** Rejected. 1.33× size inflation on binary payloads, per-frame UTF-8 escaping costs, and — worse — pretending a 4 MB compiled PDF is a metadata field couples lifetime, schema, and GC in every language instead of letting the mmap own it.
- **gRPC for workers:** Rejected as the primary plane. Adds a codegen + runtime stack per language and baking blobs inside protobuf is the same lifetime/zero-copy problem as all-JSON; its streaming mode is a good optional fit later only for *telemetry firehose*, never for data-plane bytes.
- **Per-stage temp files on disk as the medium:** Rejected. Works but churns the page cache and costs an explicit unlink-on-consume dance that shared memory pages give us for free (and the mtime/timestamp ledger for cache invalidation is more fragile than a slot mutex).
- **Ptr-arithmetic single flat byte frame per page:** Rejected; ring-buffer slots with per-slot state machine (`EMPTY`/`WRITING`/`FULL`) let multiple producers hand off the arena without a coordinator, which matters when the Go scanner, lazy-modules, and cpp daemon are all attaching concurrently.

## Consequences

- **Positive:** Sub-millisecond dispatches for big-frame handoffs; base64 never existed.
- **Positive:** One JSON schema (already versioned in `infra/schemas/ipc_envelope.schema.json`) means contract drift is caught at unit-test time in all three languages against the same golden fixture.
- **Positive:** A worker crash trashes only its own data-plane generations; the control plane FSM state is never lost to a renderer problem.
- **Negative:** Deviating from the schema requires a cross-language coordinated change; the JSON schema file is the source of truth any language's envelope serializer must match, and Phase-1 tests pin the Rust side first.
- **Negative:** Shared memory arena sizing (64 MB default, tunable) briefly pins OS pages; the fallback for arenas that fail to attach is a unified JSON error with remediation, not a size explosion.
