// Package models — wire types for Stage B1 (Spec §3.2). Field names match
// backend/src/models/manifest.rs; keep both in lockstep.
package models

// ScanRequest is dispatched by the Rust control plane over the Unix socket.
type ScanRequest struct {
	RootTexPath    string
	DropinDirPath  string
	UserSourceDirs []string
	// Optional propagated context (§1.3): empty trace_id means mint UUIDv7.
	TraceID string
	// Optional FSM state to echo into meta.sync_state.
	SyncState string
	// CacheDir overrides Tier 3 during tests; empty = ~/.texcraft/cache/ctan/.
	CacheDir string
	// CTANFetcherOverride (service-level injection) stays out of the wire
	// contract; resolver wiring uses a function field via the controller.
}

// ResolvedTier — which tier satisfied a dependency (tier order per ADR-003).
type ResolvedTier int

const (
	TierDropIn       ResolvedTier = iota // Tier 1: <root>/dropin/ + <root>/
	TierUserRegistry                     // Tier 2: user custom source registries
	TierLocalCache                       // Tier 3: ~/.texcraft/cache/ctan/ digest cache
	TierCtanRemote                       // Tier 4: heuristic CTAN remote resolver
)

// Wire names mirror backend ResolvedTier snake_case values.
func (t ResolvedTier) String() string {
	switch t {
	case TierDropIn:
		return "drop_in"
	case TierUserRegistry:
		return "user_registry"
	case TierLocalCache:
		return "local_ctan_cache"
	case TierCtanRemote:
		return "ctan_remote"
	default:
		return "unknown"
	}
}

// FigureBlock — one detected pyplot/mermaid environment with exact line span.
type FigureBlock struct {
	Kind      string `json:"kind"` // "pyplot" | "mermaid"
	StartLine uint64 `json:"start_line"`
	EndLine   uint64 `json:"end_line"`
	StartCol  uint64 `json:"start_col"`
	BlockHash string `json:"block_hash"` // BLAKE3(block_source)
}

// ScannedFile — per-file directive findings across the concurrent input tree.
type ScannedFile struct {
	RelPath  string   `json:"rel_path"`
	Packages []string `json:"packages"`
	BibFiles []string `json:"bib_files"`
	Includes []uint64 `json:"includes"`
}

// DependencyManifest — the B1 artifact consumed by B2 (lazy-modules) and B3 (C++ daemon).
type DependencyManifest struct {
	RootTex      string            `json:"root_tex"`
	ScannedFiles []ScannedFile     `json:"scanned_files"`
	FigureBlocks []FigureBlock     `json:"figure_blocks"`
	HasPyplot    bool              `json:"has_pyplot"`
	HasMermaid   bool              `json:"has_mermaid"`
	Resolutions  map[string]string `json:"resolutions"` // package -> tier.String()
	PreambleHash string            `json:"preamble_hash,omitempty"`
}

// Data payload key used in the success envelope.
func CollectData(manifest *DependencyManifest) map[string]any {
	return map[string]any{
		"stage":         "B1_GO_HEURISTIC_SCAN",
		"manifest":      manifest,
		"scanned_files": len(manifest.ScannedFiles),
		"has_pyplot":    manifest.HasPyplot,
		"has_mermaid":   manifest.HasMermaid,
	}
}
