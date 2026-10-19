// Package services — Phase 2 Verification Gate (a): a .sty dropped into
// /dropin overrides CTAN; plus full 4-tier ordering + input-path resolution.
package services

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func newRoot(t *testing.T) (projectRoot, dropin, cache string) {
	t.Helper()
	projectRoot = t.TempDir()
	dropin = filepath.Join(projectRoot, "dropin")
	if err := os.MkdirAll(dropin, 0o755); err != nil {
		t.Fatal(err)
	}
	cache = t.TempDir()
	return projectRoot, dropin, cache
}

// (a) Tier 1 (drop-in) wins over Tier 4 (CTAN): the fake network fetch must
// never be called when the file lives in /dropin.
func TestDropInOverridesCtan(t *testing.T) {
	_, dropin, cache := newRoot(t)
	if err := os.WriteFile(filepath.Join(dropin, "algpseudocode.sty"), []byte("\\ProvidesPackage{algpseudocode}"), 0o644); err != nil {
		t.Fatal(err)
	}
	resolver := NewTierResolver(dropin, nil, cache)

	fetchCalled := false
	resolver.ctanFetch = func(pkg string) error {
		fetchCalled = true
		return errors.New("network must not be touched when Tier 1 has the file")
	}

	tier, err := resolver.ResolvePackage("algpseudocode")
	if err != nil {
		t.Fatalf("ResolvePackage: %v", err)
	}
	if tier != "drop_in" {
		t.Fatalf("want drop_in tier, got %s", tier)
	}
	if fetchCalled {
		t.Fatal("Tier 4 fetch invoked although Tier 1 already resolved the package")
	}
}

func TestTier2AndTier3Ordering(t *testing.T) {
	projectRoot, dropin, cache := newRoot(t)
	_ = projectRoot
	userDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(userDir, "pkgB.sty"), []byte("b"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cache, "pkgC.sty"), []byte("c"), 0o644); err != nil {
		t.Fatal(err)
	}
	fakeFetch := func(pkg string) error {
		return errors.New("touched network: " + pkg)
	}

	resolver := NewTierResolver(dropin, []string{userDir}, cache)
	resolver.ctanFetch = fakeFetch

	if tier, err := resolver.ResolvePackage("pkgB"); err != nil || tier != "user_registry" {
		t.Fatalf("Tier 2 expectation violated: tier=%s err=%v", tier, err)
	}
	if tier, err := resolver.ResolvePackage("pkgC"); err != nil || tier != "local_ctan_cache" {
		t.Fatalf("Tier 3 expectation violated: tier=%s err=%v", tier, err)
	}
}

// Tier 4 resolves via the embedded inverted index and the injected fetcher.
func TestTier4HeuristicIndexResolution(t *testing.T) {
	_, dropin, cache := newRoot(t)
	resolver := NewTierResolver(dropin, nil, cache)
	var fetched []string
	resolver.ctanFetch = func(pkg string) error {
		fetched = append(fetched, pkg)
		return nil
	}

	tier, err := resolver.ResolvePackage("algpseudocode")
	if err != nil {
		t.Fatalf("ResolvePackage tier 4: %v", err)
	}
	if tier != "ctan_remote" {
		t.Fatalf("want ctan_remote tier, got %s", tier)
	}
	if len(fetched) != 1 || fetched[0] != "algpseudocode" {
		t.Fatalf("expected exactly one Tier-4 fetch for algpseudocode, got %v", fetched)
	}
}

func TestResolveInputPathPrefersCallerDirThenDropIn(t *testing.T) {
	projectRoot, dropin, cache := newRoot(t)
	chapters := filepath.Join(projectRoot, "chapters")
	if err := os.MkdirAll(chapters, 0o755); err != nil {
		t.Fatal(err)
	}
	chapter1 := filepath.Join(chapters, "chapter1.tex")
	dropin2 := filepath.Join(dropin, "shared.tex")
	if err := os.WriteFile(chapter1, []byte("\\section{One}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dropin2, []byte("\\newcommand{\\shared}{ok}"), 0o644); err != nil {
		t.Fatal(err)
	}
	resolver := NewTierResolver(dropin, nil, cache)

	got, err := resolver.ResolveInputPath(chapters, "chapter1")
	if err != nil || got != chapter1 {
		t.Fatalf("caller-dir inheritance failed: got=%s err=%v", got, err)
	}
	got, err = resolver.ResolveInputPath(chapters, "shared")
	if err != nil || got != dropin2 {
		t.Fatalf("Tier-1 drop-in input resolution failed: got=%s err=%v", got, err)
	}
	// Unresolvable input targets come back with an explicit error.
	if _, err := resolver.ResolveInputPath(chapters, "ghost"); err == nil {
		t.Fatal("expected error for unresolvable input target")
	}
}
