#!/usr/bin/env bash
# TexCraft reproducible three-toolchain build (ADR-001).
# Usage: ./infra/scripts/build_all.sh [go] [cpp]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

stage() { printf '\n== %s ==\n' "$*"; }

# 1) Rust control plane + auth + lazy-modules (Phase 1 verification gate)
stage "rust: cargo build + test (workspace)"
(cd "$ROOT" && cargo build --workspace && cargo test --workspace)

# 2) Go heuristic scanner (Phase 2)
if [ -f "$ROOT/worker/go-scanner/go.mod" ]; then
  stage "go: build + test (go-scanner)"
  (cd "$ROOT/worker/go-scanner" && go build ./... && go test ./...)
else
  stage "go: skipped (Phase 2 has not initialized go.mod)"
fi

# 3) C++ warm TeX daemon (Phase 4)
if [ -f "$ROOT/worker/cpp-compiler/CMakeLists.txt" ]; then
  stage "cpp: cmake build (cpp-compiler)"
  cmake -S "$ROOT/worker/cpp-compiler" -B "$ROOT/worker/cpp-compiler/build" \
        -DCMAKE_BUILD_TYPE=RelWithDebInfo
  cmake --build "$ROOT/worker/cpp-compiler/build" -j
else
  stage "cpp: skipped (Phase 4 has not created CMakeLists.txt)"
fi

stage "build_all complete"
