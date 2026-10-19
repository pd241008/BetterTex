// Package models — Unified JSON Response Envelope (Spec §1.4) and
// dependency-manifest types shared with the Rust control plane (§3.2 B1).
package models

import "encoding/json"

// Status values for Envelope.Status.
const (
	StatusSuccess = "success"
	StatusError   = "error"
)

// Canonical control-plane codes (§1.4).
const (
	CodeScanComplete = "SCAN_COMPLETE"
	CodeScanFailed   = "ERR_SCAN_FAILED"
	CodeBadPayload   = "ERR_BAD_PAYLOAD"
)

// EnvelopeError is the `error` object: human message + machine remediation hint.
type EnvelopeError struct {
	Message     string `json:"message"`
	Remediation string `json:"remediation,omitempty"`
}

// TimingsMs mirrors Spec §1.4 meta.timings_ms. Zero-cost stages are always
// serialized (0), matching the schema exactly.
type TimingsMs struct {
	GitProbe        uint64 `json:"git_probe"`
	GoHeuristicScan uint64 `json:"go_heuristic_scan"`
	LazySidecar     uint64 `json:"lazy_sidecar"`
	CppCompile      uint64 `json:"cpp_compile"`
}

// EnvelopeMeta mirrors the Schema `meta` object; worker-specific keys
// (remote_head_sha etc.) are added via Extra so the shape stays open (§1.4).
type EnvelopeMeta struct {
	TraceID    string     `json:"trace_id"`
	SyncState  string     `json:"sync_state,omitempty"`
	PushLocked bool       `json:"push_locked"`
	TimingsMs  *TimingsMs `json:"timings_ms,omitempty"`

	// Extra carries worker-contributed keys (flattened into the JSON object).
	Extra map[string]any `json:"-"`
}

func (m EnvelopeMeta) MarshalJSON() ([]byte, error) {
	type plain struct {
		TraceID    string     `json:"trace_id"`
		SyncState  string     `json:"sync_state,omitempty"`
		PushLocked bool       `json:"push_locked"`
		TimingsMs  *TimingsMs `json:"timings_ms,omitempty"`
	}
	out := plain{
		TraceID:    m.TraceID,
		SyncState:  m.SyncState,
		PushLocked: m.PushLocked,
		TimingsMs:  m.TimingsMs,
	}
	b, err := json.Marshal(out)
	if err != nil {
		return nil, err
	}
	if len(m.Extra) == 0 {
		return b, nil
	}
	var obj map[string]any
	if err := json.Unmarshal(b, &obj); err != nil {
		return nil, err
	}
	for k, v := range m.Extra {
		if _, builtin := obj[k]; !builtin {
			obj[k] = v
		}
	}
	return json.Marshal(obj)
}

// Envelope is the Unified JSON Response Envelope — every control-plane response
// across Rust/Go/C++ must conform (Spec §1.4, infra/schemas/ipc_envelope.schema.json).
type Envelope struct {
	Status string         `json:"status"`
	Code   string         `json:"code"`
	Data   map[string]any `json:"data"`
	Meta   EnvelopeMeta   `json:"meta"`
	Error  *EnvelopeError `json:"error"`
}

func Success(code string, data map[string]any, meta EnvelopeMeta) Envelope {
	return Envelope{Status: StatusSuccess, Code: code, Data: data, Meta: meta, Error: nil}
}

func Error(code, message string, meta EnvelopeMeta, remediation string) Envelope {
	return Envelope{
		Status: StatusError,
		Code:   code,
		Data:   nil,
		Meta:   meta,
		Error:  &EnvelopeError{Message: message, Remediation: remediation},
	}
}
