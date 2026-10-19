// Package services — tier_resolver.go: 4-Tier Heuristic Dependency Resolution
// (Spec §3.2 B1.4, ADR-003). Strict priority order:
//
//	Tier 1 Drop-in  ->  Tier 2 User registries  ->  Tier 3 Local CTAN cache
//	->  Tier 4 Heuristic CTAN remote resolver (fetch + extract into Tier 3).
package services

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

// TierResolver evaluates each required package/file against the 4 tiers in
// strict order and reports the first tier that satisfied it.
type TierResolver struct {
	dropinDir      string   // Tier 1
	projectRoot    string   // Tier 1b: root of the project itself
	userSourceDirs []string // Tier 2
	cacheDir       string   // Tier 3

	ctanLock  sync.Mutex
	ctanIndex CtanIndex              // Tier 4 embedded inverted index (ctan_heuristic.go)
	ctanFetch func(pkg string) error // injectable remote fetch; nil = real fetch
}

// NewTierResolver wires a resolver for one scan. userSourceDirs are absolute
// Tier-2 paths registered by the (Phase 5) SourceRegistryModal and passed here
// by the control plane on every ScanRequest.
func NewTierResolver(dropinDir string, userSourceDirs []string, cacheDir string) *TierResolver {
	return &TierResolver{
		dropinDir:      dropinDir,
		userSourceDirs: userSourceDirs,
		cacheDir:       cacheDir,
		ctanIndex:      EmbeddedCtanIndex(),
	}
}

// exts lists the TeX-side extensions probed per dependency name.
var exts = []string{".sty", ".cls", ".tex", ".bst", ".bib"}

// ResolvePackage returns the tier name ("drop_in", "user_registry",
// "local_ctan_cache", "ctan_remote") that satisfied pkg, checking tiers in
// strict order and stopping at the first hit.
func (r *TierResolver) ResolvePackage(pkg string) (string, error) {
	if _, tier, ok := r.tier1DropIn(pkg); ok {
		return tier, nil
	}
	if _, tier, ok := r.tier2UserDirs(pkg, exts); ok {
		return tier, nil
	}
	if _, tier, ok := r.tier3Cache(pkg); ok {
		return tier, nil
	}
	// Tier 4: embedded inverted index, then fetch + SHA-verified extract.
	if _, ok := r.ctanIndex.Lookup(pkg); ok {
		fetch := r.ctanFetch
		if fetch == nil {
			fetch = r.fetchFromCtan
		}
		if err := fetch(pkg); err != nil {
			return "", fmt.Errorf("tier 4 CTAN fetch for %s: %w", pkg, err)
		}
		return "ctan_remote", nil
	}
	return "", fmt.Errorf("no file for %q in any tier", pkg)
}

// ResolveInputPath resolves a \input{}/\include target in Tier-1/2 order and
// returns the absolute path to scan next, or "" when unresolvable.
func (r *TierResolver) ResolveInputPath(callerDir, target string) (string, error) {
	candidates := inputCandidates(target)
	// Relative first (caller's directory = same-dir inheritance), then Tier 1
	// drop-in, then Tier 2 registries.
	for _, cand := range candidates {
		p := filepath.Join(callerDir, cand)
		if fileExists(p) {
			return p, nil
		}
	}
	if dir, tier, ok := r.tier1DropInDirs(candidates); ok {
		_ = tier
		return dir, nil
	}
	for _, base := range r.userSourceDirs {
		for _, cand := range candidates {
			p := filepath.Join(base, cand)
			if fileExists(p) {
				return p, nil
			}
		}
	}
	return "", fmt.Errorf("input target %q not found in drop-in, project root, or user registries", target)
}

// inputCandidates expands an input target against TeX filename semantics.
func inputCandidates(target string) []string {
	name := strings.TrimSpace(target)
	if name == "" {
		return nil
	}
	out := []string{name}
	if filepath.Ext(name) == "" {
		out = append(out, name+".tex", name+".cls")
	}
	return out
}

// ---- Tier 1 -----------------------------------------------------------------

// tier1DropIn checks <root>/dropin/ AND the project root itself.
func (r *TierResolver) tier1DropIn(pkg string) (string, string, bool) {
	return r.probeDirs([]string{r.dropinDir, projectRootOf(r.dropinDir)}, pkg, exts)
}

func (r *TierResolver) tier1DropInDirs(candidates []string) (string, string, bool) {
	searchDirs := []string{r.dropinDir, projectRootOf(r.dropinDir)}
	for _, base := range searchDirs {
		for _, cand := range candidates {
			p := filepath.Join(base, cand)
			if fileExists(p) {
				return p, "drop_in", true
			}
		}
	}
	return "", "", false
}

// projectRootOf derives the project root from a drop-in dir (<root>/dropin).
func projectRootOf(dropinDir string) string {
	parent := filepath.Dir(filepath.Clean(dropinDir))
	if parent == filepath.Clean(dropinDir) { // dropin was given at root level
		parent = filepath.Clean(dropinDir)
	}
	return parent
}

// ---- Tier 2 -----------------------------------------------------------------

func (r *TierResolver) tier2UserDirs(pkg string, extsToProbe []string) (string, string, bool) {
	return r.probeDirs(r.userSourceDirs, pkg, extsToProbe)
}

// ---- generic probing ----------------------------------------------------------

func (r *TierResolver) probeDirs(dirs []string, pkg string, extsToProbe []string) (string, string, bool) {
	for _, base := range dirs {
		if base == "" {
			continue
		}
		for _, ext := range extsToProbe {
			p := filepath.Join(base, pkg+ext)
			if fileExists(p) {
				return p, tierOfBase(base, r), true
			}
		}
	}
	return "", "", false
}

func tierOfBase(base string, r *TierResolver) string {
	clean := filepath.Clean(base)
	switch clean {
	case filepath.Clean(r.dropinDir), projectRootOf(r.dropinDir):
		return "drop_in"
	case filepath.Clean(r.cacheDir):
		return "local_ctan_cache"
	default:
		return "user_registry"
	}
}

// ---- Tier 3 -----------------------------------------------------------------

func (r *TierResolver) tier3Cache(pkg string) (string, string, bool) {
	return r.probeDirs([]string{r.cacheDir}, pkg, exts)
}

// ---- Tier 4 (real fetch lives in ctan_heuristic.go) ---------------------------

func (r *TierResolver) fetchFromCtan(pkg string) error {
	return fetchCtanArchive(pkg, r.cacheDir)
}

func fileExists(p string) bool {
	fi, err := os.Stat(p)
	return err == nil && (!fi.IsDir() || true)
}
