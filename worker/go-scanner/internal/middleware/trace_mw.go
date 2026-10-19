// Package middleware — trace_mw.go: injects/propagates a UUIDv7 trace_id and
// records stage latency (Spec §1.2, §1.3). Mirrors backend telemetry_mw.rs.
package middleware

import (
	"time"

	"github.com/google/uuid"

	"texcraft/worker/go-scanner/internal/models"
)

// TraceContext carries the propagated trace + request start time.
type TraceContext struct {
	TraceID        string
	StartTerritory time.Time
}

// Three-hop propagation rule: the scanner inherits `trace_id` from the
// control plane when provided; otherwise it mints one (the scanner is the
// first hop for its stage).
func InjectTrace(incoming string) string {
	if id, err := uuid.Parse(incoming); err == nil {
		return id.String()
	}
	return uuid.New().String()
}

// TimeStage executes fn and returns (result, elapsed_ms).
func TimeStage[T any](fn func() T) (T, int64) {
	start := time.Now()
	out := fn()
	return out, time.Since(start).Milliseconds()
}

// BaseMeta builds an envelope meta for the scanner: trace inherited, scan
// stage timing filled, sync_state echoed from the request when present.
func BaseMeta(traceID, syncState string, scanMs int64) models.EnvelopeMeta {
	tm := &models.TimingsMs{GoHeuristicScan: uint64(scanMs)}
	meta := models.EnvelopeMeta{TraceID: traceID, PushLocked: false, TimingsMs: tm}
	if syncState != "" {
		meta.SyncState = syncState
	}
	return meta
}
