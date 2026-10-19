# TexCraft: Polyglot Local-First LaTeX IDE — Master Agent Specification

> **Agent Role & Mission:** You are an autonomous Staff Systems Architect and Polyglot Engineer. Your objective is to build **TexCraft**, a local-first, high-performance Overleaf alternative utilizing a **Rust Control Plane Orchestrator**, a **Go Heuristic Dependency & AST Scanner**, a **C++ Warm TeX Compilation Daemon**, and **Lazy-Loaded Pyplot/Mermaid Figure Sidecars**, backed by **GitHub Auto-Sync with Priority Protection**.
>
> You must strictly follow all architectural boundaries, state machine invariants, and **`Design-Dungeons`** engineering constraints defined in this document. Do not skip phases or violate module boundaries.

---

## 1. Non-Negotiable `Design-Dungeons` Engineering Constraints

Every line of code, directory, and inter-process call in this repository must obey the following five foundational rules:

### 1.1 Strict Boundary Decoupling

The monorepo is partitioned into five strictly isolated top-level domains:

* **`frontend/`**: UI presentation, split-pane editor, SyncTeX viewport, and drag-and-drop ingestion. Never accesses Git credentials, filesystem caches, or worker sockets directly.
* **`auth/`**: Isolated authentication service managing GitHub OAuth Device Flow, Personal Access Tokens (PATs), and OS native keyring storage.
* **`backend/`**: Rust core orchestrator and control plane. Owns the Git Priority Lock Finite State Machine (FSM), `BLAKE3` build DAG, shared memory broker, and worker lifecycle orchestration.
* **`worker/`**: Isolated execution engines (`go-scanner`, `cpp-compiler`, `lazy-modules`). Workers are stateless or warm-cache slaves; they never mutate Git state or communicate directly with the frontend.
* **`infra/`**: Protobuf/IPC schemas, cross-platform build scripts, and reproducible toolchains.

### 1.2 Standardized Internal Layering

Inside `auth/`, `backend/`, and `worker/go-scanner/`, enforce the strict unidirectional request flow:

$$\text{Routes} \rightarrow \text{Middleware} \rightarrow \text{Controllers} \rightarrow \text{Services} \rightarrow \text{Models}$$

* **Routes**: Bind IPC/WebSocket/Unix socket endpoints and parse wire payloads.
* **Middleware**: Inject `trace_id`, record stage latency telemetry, and enforce the **Git Priority Lock Gatekeeper**.
* **Controllers**: Validate inputs, orchestrate service calls, and format the Unified JSON Response.
* **Services**: Pure domain logic, FSM transitions, shared-memory ring buffer allocation, and worker IPC dispatch.
* **Models**: Strongly-typed data structures, state enums, and serialization schemas.

### 1.3 Instrumentation Before Optimization

No caching or warm-daemon optimization may be merged without structured telemetry. Every request entering `backend/` must generate a UUIDv7 `trace_id` and record stage-by-stage latency metrics (`git_probe_ms`, `go_scan_ms`, `lazy_sidecar_ms`, `cpp_compile_ms`).

### 1.4 Unified JSON Response Envelope (Control Plane)

Every API and IPC control-plane response across Rust, Go, and C++ must conform to this exact JSON envelope structure:

```json
{
  "status": "success",
  "code": "BUILD_STAGE_COMPLETE",
  "data": {
    "stage": "B3_CPP_WARM_COMPILE",
    "shm_descriptor": "/texcraft_shm_buf_01",
    "pdf_byte_len": 418920,
    "synctex_path": ".texcraft_cache/main.synctex.gz"
  },
  "meta": {
    "trace_id": "0192a4b8-7c10-7000-8000-000000000001",
    "sync_state": "S2_SYNCED_READY",
    "push_locked": false,
    "timings_ms": {
      "git_probe": 38,
      "go_heuristic_scan": 5,
      "lazy_sidecar": 0,
      "cpp_compile": 162
    }
  },
  "error": null
}
```

When an error or Priority Lock rejection occurs:

```json
{
  "status": "error",
  "code": "ERR_PRIORITY_LOCK_REMOTE_AHEAD",
  "data": null,
  "meta": {
    "trace_id": "0192a4b8-7c10-7000-8000-000000000002",
    "sync_state": "S3_PRIORITY_LOCKED",
    "push_locked": true,
    "remote_head_sha": "a1b2c3d4e5f6",
    "local_base_sha": "998877665544"
  },
  "error": {
    "message": "Push blocked by Priority Protection: remote repository has newer commits. Run Pull & Reconcile first.",
    "remediation": "TRIGGER_S3_RECONCILE"
  }
}
```

### 1.5 Control Plane vs. Data Plane Separation

* **Control Plane (Unified JSON over Unix Domain Sockets / Named Pipes)**: Used strictly for commands, state transitions, dependency manifests, and telemetry metadata.
* **Data Plane (OS Shared Memory `mmap`)**: Raw `.tex` buffers, precompiled `.fmt` memory dumps, generated vector `.pdf` figures, and final compiled PDF page buffers must be transferred via POSIX (`shm_open` / `mmap`) or Win32 (`CreateFileMapping` / `MapViewOfFile`) shared memory. Never serialize megabytes of PDF or TeX buffers into JSON strings.

---

## 2. Target Repository Directory Structure

Scaffold and maintain this exact directory hierarchy:

```text
texcraft/
├── docs/
│   ├── adr/
│   │   ├── ADR-001-polyglot-rust-go-cpp-split.md
│   │   ├── ADR-002-github-priority-lock-state-machine.md
│   │   ├── ADR-003-go-heuristic-ctan-and-dropin-resolver.md
│   │   ├── ADR-004-lazy-loaded-pyplot-mermaid-sidecars.md
│   │   └── ADR-005-shared-memory-ipc-and-unified-json.md
│   └── architecture/
│       ├── state-machine-spec.md
│       └── ipc-wire-protocol.md
│
├── frontend/                          # Split-pane UI (Editor + GPU PDF Viewport)
│   ├── src/
│   │   ├── components/
│   │   │   ├── EditorPane/            # CodeMirror/Monaco + AST diagnostics & hover
│   │   │   ├── PdfViewport/           # SyncTeX click-to-jump + double-buffered canvas
│   │   │   ├── DropZone/              # Drag-and-drop .tex/.sty/.cls/.bib ingest
│   │   │   ├── SourceRegistryModal/   # User custom local/remote LaTeX source paths
│   │   │   └── SyncStatusBar/         # Priority Lock indicator, Auto-Sync & Reconcile UI
│   │   ├── state/                     # Client store mirroring S0-S4 & B1-B3 states
│   │   └── ipc/                       # Unified JSON Envelope client & SHM reader
│   └── package.json
│
├── auth/                              # Isolated GitHub Credential & Token Service (Rust)
│   ├── src/
│   │   ├── routes/                    # auth_routes.rs (OAuth device flow, PAT bind)
│   │   ├── middleware/                # scope_guard.rs (repo permission verifier)
│   │   ├── controllers/               # auth_controller.rs
│   │   ├── services/                  # keyring_service.rs, github_oauth_service.rs
│   │   ├── models/                    # token_model.rs
│   │   └── lib.rs
│   └── Cargo.toml
│
├── backend/                           # Rust Core Orchestrator (Control Plane)
│   ├── src/
│   │   ├── routes/                    # command_routes.rs, ws_events.rs
│   │   ├── middleware/
│   │   │   ├── telemetry_mw.rs        # Trace ID injection & latency timers
│   │   │   └── priority_lock_mw.rs    # Blocks push requests when state == S3
│   │   ├── controllers/
│   │   │   ├── build_controller.rs    # Drives B1 (Go) -> B2 (Lazy) -> B3 (C++)
│   │   │   ├── sync_controller.rs     # Drives S0-S4 GitHub sync & reconcile
│   │   │   └── source_controller.rs   # Manages /dropin files & custom source dirs
│   │   ├── services/
│   │   │   ├── git_priority_fsm.rs    # Deterministic S0-S4 state machine (gix/git2)
│   │   │   ├── dag_cache_service.rs   # BLAKE3 content-addressed figure & pass cache
│   │   │   ├── shm_broker.rs          # Cross-platform shared memory arena manager
│   │   │   └── worker_supervisor.rs   # Spawns & health-checks Go, C++, & Lazy workers
│   │   ├── models/
│   │   │   ├── envelope.rs            # Unified JSON Response & Error structs
│   │   │   ├── fsm_state.rs           # SyncState (S0-S4) & BuildStage (B1-B3) enums
│   │   │   └── manifest.rs            # AST Dependency Manifest schema
│   │   └── main.rs
│   └── Cargo.toml
│
├── worker/                            # Isolated Execution Engines & Sidecars
│   ├── go-scanner/                    # Go Heuristic Dependency & AST Scanner
│   │   ├── cmd/scanner/main.go        # Unix socket / Named Pipe server entrypoint
│   │   ├── internal/
│   │   │   ├── routes/                # ipc_router.go
│   │   │   ├── middleware/            # trace_mw.go
│   │   │   ├── controllers/           # scan_controller.go
│   │   │   ├── services/
│   │   │   │   ├── ast_lexer.go       # Fast concurrent .tex, pyplot, mermaid scanner
│   │   │   │   ├── tier_resolver.go   # 4-Tier resolution (Drop-in -> User -> Cache -> CTAN)
│   │   │   │   └── ctan_heuristic.go  # Inverted index mapping .sty/.cls -> CTAN tarball
│   │   │   └── models/                # scan_manifest.go
│   │   └── go.mod
│   │
│   ├── cpp-compiler/                  # C++ Warm TeX Daemon & SyncTeX Engine
│   │   ├── include/
│   │   │   ├── ipc_server.hpp         # Socket control listener + Unified JSON emitter
│   │   │   ├── shm_arena.hpp          # Zero-copy POSIX/Win32 shared memory attachment
│   │   │   ├── warm_tex_engine.hpp    # Resident .fmt preamble & Web2C/XeTeX/pdfTeX core
│   │   │   └── synctex_index.hpp      # Bidirectional source line <-> PDF (page, x, y)
│   │   ├── src/
│   │   │   ├── main.cpp
│   │   │   ├── ipc_server.cpp
│   │   │   ├── warm_tex_engine.cpp
│   │   │   └── synctex_index.cpp
│   │   └── CMakeLists.txt
│   │
│   └── lazy-modules/                  # On-Demand Figure & Diagram Transpilers
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs
│       │   ├── bootstrap_manager.rs   # Downloads standalone Python/Mermaid on 1st use
│       │   ├── ast_rewriter.rs        # Replaces pyplot/mermaid blocks with \includegraphics
│       │   ├── pyplot_bridge.rs       # Persistent warm Python stdio/SHM worker bridge
│       │   └── mermaid_bridge.rs      # Persistent headless Mermaid -> Vector PDF bridge
│       └── runtimes/                  # Populated lazily at runtime (empty at install)
│           └── .gitkeep
│
└── infra/
    ├── schemas/
    │   └── ipc_envelope.schema.json
    └── scripts/
        ├── build_all.sh
        └── generate_ctan_index.go
```

---

## 3. State Machines & Pipeline Stages

The backend orchestrator (`backend/src/services/git_priority_fsm.rs` and `build_controller.rs`) must implement two coupled state machines.

### 3.1 Plane A: GitHub Synchronization & Priority Protection FSM (`SyncState`)

 Collaboration uses a user-owned GitHub repository with automatic startup sync, manual sync support, and strict **Priority Protection** (optimistic locking that blocks pushing whenever the remote repository contains commits not yet integrated locally).

| State ID | Enum Variant | Entry Invariant | System Behavior | Push Permission | Allowed Transitions |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **S0** | `S0_BOOT_PROBE` | Workspace opened or app started. | Fetches credential from `auth/`. Executes `git ls-remote origin HEAD` (timeout $1500\text{ ms}$) to read `remote_head_sha` and compares with `local_base_sha`. | `LOCKED` | $\rightarrow \text{S1}$, $\text{S2}$, or $\text{S3}$ |
| **S1** | `S1_AUTO_PULL_FF` | `remote_head_sha != local_base_sha` AND `working_tree_dirty == false` AND `local_ahead_count == 0`. | Automatically performs fast-forward pull (`git merge --ff-only FETCH_HEAD`) before unlocking the editor buffer. Triggers Build Pipeline (`B1`). | `LOCKED` | $\rightarrow \text{S2}$ |
| **S2** | `S2_SYNCED_READY` | `remote_head_sha == local_base_sha` (working tree may be clean or have local edits/commits). | Normal authoring state. Background fiber polls `git ls-remote` every $30\text{ s}$. | **`UNLOCKED`** | $\rightarrow \text{S3}$ (if remote changes) or $\rightarrow \text{S4}$ (on Push) |
| **S3** | `S3_PRIORITY_LOCKED` | `remote_head_sha != local_base_sha` AND (`working_tree_dirty == true` OR `local_ahead_count > 0`). | **Priority Protection Triggered:** `priority_lock_mw.rs` rejects all push requests with `ERR_PRIORITY_LOCK_REMOTE_AHEAD`. UI replaces Push button with **Pull & Reconcile**. | **`HARD_LOCKED`** | $\rightarrow \text{S3\_RECONCILE}$ |
| **S3R** | `S3_RECONCILE` | User invokes **Pull & Reconcile** from `S3`. | 1. Snapshots dirty files (`git stash push -u`).<br>2. Fetches & rebases onto `origin/MAIN`.<br>3. Re-applies stash (`git stash pop`).<br>4. If conflict-free, updates `local_base_sha = remote_head_sha`. If conflict markers exist, opens 3-way conflict view in editor. | `LOCKED` | $\rightarrow \text{S2}$ (clean) or stays in `S3R` until conflict resolved |
| **S4** | `S4_ATOMIC_PUSH` | User clicks Manual Sync/Push while in `S2`. | **Pre-Push Gate:** Re-runs `git ls-remote origin HEAD` immediately before push. If `remote_head_sha` changed since last poll, aborts immediately to `S3`. Otherwise stages, commits, and pushes using `--force-with-lease`. | `IN_FLIGHT` | $\rightarrow \text{S2}$ (success) or $\rightarrow \text{S3}$ (race detected) |

### 3.2 Plane B: Polyglot Build Pipeline Stages (`BuildStage`)

Every build triggered by file save, file drop-in, or `S1` auto-pull runs sequentially through three stages:

#### Stage `B1_GO_HEURISTIC_SCAN` (`worker/go-scanner`)

1. Rust dispatches `ScanRequest` containing `root_tex_path`, `dropin_dir_path`, and `user_source_dirs[]` to the Go scanner daemon.
2. Go spawns concurrent goroutines to lex the root `.tex` file and recursively follow all `\input{}` and `\include{}` trees.
3. Go extracts all required packages (`\usepackage{}`, `\documentclass{}`), bibliographies (`\bibliography{}`, `\addbibresource{}`), and embedded figure blocks (`\begin{pyplot}...\end{pyplot}`, `\begin{mermaid}...\end{mermaid}`).
4. **4-Tier Heuristic Dependency Resolution:** For every required file or `.sty`/`.cls` package, Go resolves paths in strict priority order:
   * **Tier 1 (Workspace Drop-In Directory):** Checks `<project_root>/dropin/` and `<project_root>/` for user-dropped `.tex`, `.sty`, `.cls`, `.bst`, or `.bib` files.
   * **Tier 2 (User Custom Source Registries):** Checks user-configured local template directories or custom Git/HTTP registries.
   * **Tier 3 (Local Content-Addressed CTAN Cache):** Checks `~/.texcraft/cache/ctan/` by package digest.
   * **Tier 4 (Heuristic CTAN Remote Resolver):** Uses an embedded inverted index mapping individual `.sty`/`.cls` filenames to their parent CTAN archive (e.g., resolving `algpseudocode.sty` $\rightarrow$ `algorithmicx`), downloads the tarball concurrently, verifies its SHA-256 checksum, and extracts it into Tier 3.
5. Go returns the resolved `DependencyManifest` (including boolean flags `has_pyplot` and `has_mermaid`, plus raw block coordinates and `BLAKE3` hashes) in a Unified JSON Envelope.

#### Stage `B2_LAZY_MODULE_TRANSPILATION` (`worker/lazy-modules`)

1. **Fast Bypass:** If `DependencyManifest.has_pyplot == false` and `has_mermaid == false`, Stage `B2` completes in $0\text{ ms}$ and loads zero external runtimes.
2. **Content-Addressed Cache Lookup:** For each detected `pyplot` or `mermaid` block, Rust computes:

   $$H_{\text{block}} = \text{BLAKE3}(\text{block\_source} \parallel \text{preamble\_font\_config} \parallel \text{target\_width})$$

   If `.texcraft_cache/figures/<H_block>.pdf` already exists on disk, the worker is not invoked; the AST rewriter immediately substitutes `\includegraphics{.texcraft_cache/figures/<H_block>.pdf}`.
3. **Lazy Runtime Bootstrap (On First Cache Miss Only):**
   * **Pyplot Module:** If `~/.texcraft/runtimes/python/` is absent, `bootstrap_manager.rs` downloads a minimal standalone portable Python build + `matplotlib` + `numpy` and starts a persistent warm Python worker process listening over stdio/shared-memory (`pyplot_bridge.rs`). It pre-imports `matplotlib.pyplot` and configures `pdf.fonttype = 42` (TrueType/Type 1 vector fonts only; never Type 3 bitmap fonts) and LaTeX serif styling.
   * **Mermaid Module:** If `~/.texcraft/runtimes/mermaid/` is absent, `bootstrap_manager.rs` downloads the lightweight Mermaid-to-SVG/PDF renderer and starts a warm worker (`mermaid_bridge.rs`).
4. **AST Shadow Rewrite:** `ast_rewriter.rs` writes the transformed `.tex` buffer (where all `\begin{pyplot}` and `\begin{mermaid}` environments are replaced by `\includegraphics` pointing to the generated vector PDFs, preserving exact line counts via `%` padding comments so **SyncTeX line numbers remain 100% accurate**) into the Shared Memory Arena.

#### Stage `B3_CPP_WARM_COMPILE` (`worker/cpp-compiler`)

1. Rust sends a `CompileCommand` over the Unix socket/named pipe to the C++ daemon specifying the Shared Memory Arena descriptor, the `BLAKE3` hash of the document preamble (everything before `\begin{document}`), and the bibliography hash.
2. **RAM-Resident Preamble Caching (`.fmt`):**
   * If the preamble hash matches the cached `.fmt` memory dump in the C++ daemon, the C++ engine skips preamble macro expansion entirely and compiles only the body starting at `\begin{document}`.
   * If the preamble hash changed, the C++ daemon dumps a fresh `.fmt` snapshot and caches it in memory.
3. **Minimal Pass Execution:** Using dependency hashes from Rust, the C++ engine runs BibTeX/Biber and cross-reference passes *only* when `.aux` or `.bib` citations actually mutate.
4. **Zero-Copy Output & SyncTeX Index:** The C++ daemon writes the resulting PDF byte stream into the output Shared Memory slot, updates the in-memory SyncTeX bidirectional R-tree index (mapping `(file_id, line, column)` $\leftrightarrow$ `(page, x_pt, y_pt)`), and returns the Unified JSON Envelope.

---

## 4. Mandatory Architecture Decision Records (`docs/adr/`)

In Phase 1, you must generate the following five ADR markdown files in `docs/adr/`. Each ADR must contain: `Status`, `Context`, `Decision`, `Rejected Alternatives`, and `Consequences`.

1. **`ADR-001-polyglot-rust-go-cpp-split.md`**: Documents why Rust owns orchestration/FSM safety, Go owns high-concurrency heuristic file/network resolution, and C++ owns native Web2C/XeTeX/pdfTeX macro compatibility (rejecting pure-Rust TeX ports due to legacy `.cls` incompatibilities).
2. **`ADR-002-github-priority-lock-state-machine.md`**: Documents the S0–S4 state machine, automatic startup fast-forward pulls, and why Priority Protection (`S3_PRIORITY_LOCKED`) blocks pushes when `origin/HEAD` advances instead of risking silent overwrites or requiring a centralized CRDT server.
3. **`ADR-003-go-heuristic-ctan-and-dropin-resolver.md`**: Documents the 4-tier resolution order (`Drop-in` $\rightarrow$ `User Sources` $\rightarrow$ `Local Cache` $\rightarrow$ `CTAN Heuristic Index`) and concurrent goroutine AST scanning.
4. **`ADR-004-lazy-loaded-pyplot-mermaid-sidecars.md`**: Documents how zero-install-bloat is achieved via on-demand runtime bootstrapping, `BLAKE3` figure caching, and line-preserving AST substitution for SyncTeX fidelity.
5. **`ADR-005-shared-memory-ipc-and-unified-json.md`**: Documents the split between the Unified JSON Control Plane and the POSIX/Win32 Shared Memory Data Plane.

---

## 5. Step-by-Step Implementation Roadmap for the Agent

Execute this build in **5 sequential phases**. Do not proceed to Phase $N+1$ until the verification gate for Phase $N$ passes.

### Phase 1: Monorepo Scaffolding, ADRs & Rust Control Plane Core

1. Create the full directory structure defined in Section 2.
2. Write `ADR-001` through `ADR-005` in `docs/adr/`.
3. Implement `backend/src/models/envelope.rs` (Unified JSON Response), `backend/src/models/fsm_state.rs`, and `backend/src/middleware/telemetry_mw.rs`.
4. Implement `backend/src/services/shm_broker.rs` using `memmap2` to allocate named shared-memory ring buffers (`64 MB` default arena) readable and writable across processes.

* **Phase 1 Verification Gate:** `cargo test --workspace` passes unit tests verifying Unified JSON serialization, `trace_id` propagation, and shared memory write/read roundtrips.

### Phase 2: Go Heuristic Scanner & 4-Tier Source Resolver (`worker/go-scanner`)

1. Initialize `worker/go-scanner/go.mod`.
2. Implement `ast_lexer.go` to concurrently scan `.tex` files for `\documentclass`, `\usepackage`, `\input`, `\include`, `\bibliography`, `\begin{pyplot}`, and `\begin{mermaid}`, recording exact start/end line numbers.
3. Implement `tier_resolver.go` and `ctan_heuristic.go` enforcing the 4-Tier hierarchy (Drop-in folder $\rightarrow$ User-registered source paths $\rightarrow$ Local cache $\rightarrow$ Remote CTAN heuristic lookup).
4. Expose the scanner over a local Unix socket (`/tmp/texcraft_go_scanner.sock` or Windows Named Pipe) following the `Routes -> Middleware -> Controllers -> Services -> Models` pattern and returning the Unified JSON Envelope.

* **Phase 2 Verification Gate:** `go test ./...` passes tests proving that: (a) a `.sty` dropped into `/dropin` overrides CTAN, (b) nested `\input{}` files are scanned concurrently, and (c) `pyplot`/`mermaid` blocks emit accurate `BLAKE3` hashes and line spans.

### Phase 3: Lazy-Loaded Pyplot & Mermaid Sidecar Engine (`worker/lazy-modules`)

1. Implement `bootstrap_manager.rs` so that Python and Mermaid runtimes are only provisioned when `ScanManifest` flags `has_pyplot == true` or `has_mermaid == true`.
2. Implement `ast_rewriter.rs`:
   * Replace `\begin{pyplot}...\end{pyplot}` and `\begin{mermaid}...\end{mermaid}` with `\includegraphics[width=\linewidth]{.texcraft_cache/figures/<blake3_hash>.pdf}`.
   * Pad the remaining lines of the replaced block with `% TEXCRAFT_LINE_PAD` comments so the line count of the `.tex` buffer sent to C++ matches the user's editor line numbers $1:1$.
3. Implement `pyplot_bridge.rs` and `mermaid_bridge.rs` with persistent warm processes so subsequent figure edits compile in $<150\text{ ms}$ and cache hits resolve in $<1\text{ ms}$.

* **Phase 3 Verification Gate:** `cargo test -p lazy-modules` verifies line-number preservation after AST rewriting and confirms `BLAKE3` cache hits skip worker invocation.

### Phase 4: C++ Warm TeX Daemon & SyncTeX Engine (`worker/cpp-compiler`)

1. Create `worker/cpp-compiler/CMakeLists.txt` (C++20).
2. Implement `shm_arena.hpp` to attach to the Rust `shm_broker` memory region without copying buffers.
3. Implement `warm_tex_engine.cpp` supporting precompiled `.fmt` preamble dumps keyed by preamble hash and invoking the underlying TeX engine (`xelatex`/`pdflatex` library or warm subprocess pool) plus `synctex_index.cpp` for forward/inverse source-to-PDF coordinate mapping.
4. Wire `backend/src/controllers/build_controller.rs` to execute the full `B1 -> B2 -> B3` pipeline end-to-end.

* **Phase 4 Verification Gate:** End-to-end integration test compiles a sample `.tex` document containing a custom dropped-in `.sty` file, a `\begin{pyplot}` block, and a `\begin{mermaid}` block into a valid PDF buffer in shared memory and resolves a SyncTeX forward/inverse coordinate query.

### Phase 5: GitHub Auth, Priority Lock FSM & Split-Pane Frontend (`auth` + `backend` + `frontend`)

1. Implement `auth/` supporting GitHub OAuth Device Flow and PAT storage via OS keychain (`keyring` crate).
2. Implement `backend/src/services/git_priority_fsm.rs` (`S0` through `S4` + `S3_RECONCILE`) and `backend/src/middleware/priority_lock_mw.rs`.
3. Build `frontend/` featuring:
   * Split-pane CodeMirror/Monaco LaTeX editor + double-buffered PDF canvas with SyncTeX click-to-jump.
   * Drag-and-drop `DropZone` that copies `.tex`, `.sty`, `.cls`, or `.bib` files directly into the workspace `/dropin` tier and triggers an immediate build.
   * `SourceRegistryModal` allowing users to add custom local template folders or remote registries.
   * `SyncStatusBar` displaying real-time Priority Lock status (`S2 Unlocked`, `S3 Priority Locked — Remote Ahead`, and one-click `Pull & Reconcile` / `Manual Sync`).

* **Phase 5 Verification Gate:** Simulate two concurrent clones of the same GitHub repository. Push a commit from Clone A; verify that Clone B (with uncommitted local edits) automatically transitions to `S3_PRIORITY_LOCKED`, blocks `POST /api/sync/push` in middleware with `ERR_PRIORITY_LOCK_REMOTE_AHEAD`, and cleanly recovers to `S2_SYNCED_READY` when `POST /api/sync/reconcile` is invoked.
