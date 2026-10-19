// Package services — ast_lexer.go: concurrent lexing of the \input/\include tree
// (Spec §3.2 B1). Each recursive document file is scanned in its own goroutine;
// directive extraction covers \documentclass, \usepackage, \input, \include,
// \bibliography, \addbibresource, and pyplot/mermaid block coordinate capture
// (1-based line spans + BLAKE3 hashes of the block source).
package services

import (
	"bufio"
	"bytes"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"

	"lukechampine.com/blake3"

	"texcraft/worker/go-scanner/internal/models"
)

var (
	reDocumentClass  = regexp.MustCompile(`\\documentclass(?:\[[^\]]*\])?\{([^{}]+)\}`)
	reUsepackage     = regexp.MustCompile(`\\usepackage(?:\[[^\]]*\])?\{([^{}]+)\}`)
	reInput          = regexp.MustCompile(`\\input\{([^{}]+)\}`)
	reInclude        = regexp.MustCompile(`\\include\{([^{}]+)\}`)
	reBibliography   = regexp.MustCompile(`\\bibliography\{([^{}]+)\}`)
	reAddBibResource = regexp.MustCompile(`\\addbibresource(?:\[[^\]]*\])?\{([^{}]+)\}`)
	reBeginBlock     = regexp.MustCompile(`\\begin\{(pyplot|mermaid)\}`)
	reEndBlock       = regexp.MustCompile(`\\end\{(pyplot|mermaid)\}`)

	// Figure flags + preamble split are used by B2/B3 later; exported via helpers.
	reBeginDocument = regexp.MustCompile(`\\begin\{document\}`)
)

// ScanOptions carries resolver wiring for a specific scan.
type ScanOptions struct {
	CacheDir string
	// CTANFetch replaces the remote Tier-4 downloader (tests inject fakes).
	CTANFetch func(pkg string) error
}

// ScanTree walks the \input/\include tree concurrently and returns the B1
// DependencyManifest. Every discovered file scans in its own goroutine; the
// only lock is a short manifest-mutation critical section.
func ScanTree(rootTexPath, dropinDir string, userSourceDirs []string, opts ScanOptions) (*models.DependencyManifest, error) {
	if opts.CacheDir == "" {
		opts.CacheDir = DefaultCacheDir()
	}

	manifest := &models.DependencyManifest{
		RootTex:      rootTexPath,
		Resolutions:  map[string]string{},
		ScannedFiles: []models.ScannedFile{},
		FigureBlocks: []models.FigureBlock{},
	}

	resolver := NewTierResolver(dropinDir, userSourceDirs, opts.CacheDir)
	resolver.ctanFetch = opts.CTANFetch

	type result struct {
		scanned  models.ScannedFile
		blocks   []models.FigureBlock
		pkgTiers map[string]string
	}
	results := make(chan result, 64)
	errMu := sync.Mutex{}
	var firstErr error
	setErr := func(e error) {
		errMu.Lock()
		if firstErr == nil {
			firstErr = e
		}
		errMu.Unlock()
	}
	var wg sync.WaitGroup
	visited := map[string]bool{}
	var visitedMu sync.Mutex

	var scanFile func(absPath string)
	scanFile = func(absPath string) {
		defer wg.Done()

		content, err := os.ReadFile(absPath)
		if err != nil {
			setErr(fmt.Errorf("scan %s: %w", absPath, err))
			return
		}
		scanned, childTargets, blocks := lexBuffer(absPath, content)
		depTiers := collectPackageRequirements(content, resolver)
		if scanned.RelPath, err = relToRoot(rootTexPath, absPath); err != nil {
			setErr(err)
			return
		}
		results <- result{scanned: *scanned, blocks: blocks, pkgTiers: depTiers}

		for _, target := range childTargets {
			resolved, rerr := resolver.ResolveInputPath(filepath.Dir(absPath), target)
			if rerr != nil {
				setErr(fmt.Errorf("include %q from %s: %w", target, absPath, rerr))
				continue
			}
			if resolved == "" {
				continue // unresolvable input: B3 will fail with a clear error
			}
			visitedMu.Lock()
			if seen := visited[resolved]; seen {
				visitedMu.Unlock()
				continue
			}
			visited[resolved] = true
			visitedMu.Unlock()
			wg.Add(1)
			go scanFile(resolved)
		}
	}

	wg.Add(1)
	rootAbs, _ := filepath.Abs(rootTexPath)
	visited[rootAbs] = true
	go scanFile(rootAbs)

	go func() {
		wg.Wait()
		close(results)
	}()

	for r := range results {
		manifest.ScannedFiles = append(manifest.ScannedFiles, r.scanned)
		manifest.FigureBlocks = append(manifest.FigureBlocks, r.blocks...)
		for pkg, tier := range r.pkgTiers {
			manifest.Resolutions[pkg] = tier
		}
	}

	if firstErr != nil {
		return nil, firstErr
	}

	manifest.HasPyplot, manifest.HasMermaid = FigureFlags(manifest.FigureBlocks)
	if preamble, ok := PreambleBytes(rootTexPath); ok {
		sum := blake3.Sum256(preamble)
		manifest.PreambleHash = hex.EncodeToString(sum[:])
	}
	return manifest, nil
}

// lexBuffer scans one .tex buffer: directive names, include line numbers, and
// pyplot/mermaid block spans with BLAKE3 hashes. All state is local (the
// function runs concurrently across goroutines).
func lexBuffer(absPath string, content []byte) (*models.ScannedFile, []string, []models.FigureBlock) {
	scanned := &models.ScannedFile{Packages: []string{}, BibFiles: []string{}, Includes: []uint64{}}
	var childTargets []string
	var blocks []models.FigureBlock

	var openKind string
	var openStartLine, openStartCol uint64
	var blockSource []byte

	line := uint64(1)
	reader := bufio.NewReader(bytes.NewReader(content))
	var buf bytes.Buffer
	for {
		chunk, err := reader.ReadBytes('\n')
		buf.Reset()
		buf.Write(chunk)
		hadNewline := bytes.HasSuffix(chunk, []byte("\n"))
		if hadNewline {
			buf.Truncate(buf.Len() - 1)
		}
		lineBytes := buf.Bytes()

		if m := reDocumentClass.FindAllSubmatch(lineBytes, -1); m != nil {
			for _, gr := range m {
				scanned.Packages = append(scanned.Packages, string(gr[1]))
			}
		}
		if m := reUsepackage.FindAllSubmatch(lineBytes, -1); m != nil {
			for _, gr := range m {
				for _, p := range strings.Split(string(gr[1]), ",") {
					if p = strings.TrimSpace(p); p != "" {
						scanned.Packages = append(scanned.Packages, p)
					}
				}
			}
		}

		var hitInclude bool
		for _, re := range []*regexp.Regexp{reInput, reInclude} {
			if m := re.FindAllSubmatch(lineBytes, -1); m != nil {
				hitInclude = true
				for _, gr := range m {
					childTargets = append(childTargets, string(gr[1]))
				}
			}
		}
		if hitInclude {
			scanned.Includes = append(scanned.Includes, line)
		}

		for _, re := range []*regexp.Regexp{reBibliography, reAddBibResource} {
			if m := re.FindAllSubmatch(lineBytes, -1); m != nil {
				for _, gr := range m {
					scanned.BibFiles = append(scanned.BibFiles, strings.TrimSpace(string(gr[1])))
				}
			}
		}

		// Figure block span capture (open → close), source preserved verbatim.
		if m := reBeginBlock.FindSubmatchIndex(lineBytes); m != nil && openKind == "" {
			openKind = string(lineBytes[m[2]:m[3]])
			openStartLine = line
			openStartCol = uint64(m[0])
			blockSource = blockSource[:0]
		}
		if openKind != "" {
			blockSource = append(blockSource, lineBytes...)
			if hadNewline {
				blockSource = append(blockSource, '\n')
			} else {
				blockSource = append(blockSource, '\n')
			}
		}
		if m := reEndBlock.FindSubmatchIndex(lineBytes); m != nil && openKind != "" &&
			string(lineBytes[m[2]:m[3]]) == openKind {
			sum := blake3.Sum256(blockSource)
			blocks = append(blocks, models.FigureBlock{
				Kind:      openKind,
				StartLine: openStartLine,
				EndLine:   line,
				StartCol:  openStartCol,
				BlockHash: hex.EncodeToString(sum[:]),
			})
			openKind = ""
		}

		if hadNewline {
			line++
		}
		if err != nil {
			break // EOF (err == io.EOF here)
		}
	}
	return scanned, childTargets, blocks
}

// collectPackageRequirements runs the 4-Tier resolution for every package this
// file requires and records which tier satisfied it (ADR-003 ledger).
func collectPackageRequirements(content []byte, r *TierResolver) map[string]string {
	out := map[string]string{}
	var pkgs []string
	if m := reDocumentClass.FindAllSubmatch(content, -1); m != nil {
		for _, gr := range m {
			pkgs = append(pkgs, string(gr[1]))
		}
	}
	if m := reUsepackage.FindAllSubmatch(content, -1); m != nil {
		for _, gr := range m {
			for _, p := range strings.Split(string(gr[1]), ",") {
				if p = strings.TrimSpace(p); p != "" {
					pkgs = append(pkgs, p)
				}
			}
		}
	}
	for _, re := range []*regexp.Regexp{reBibliography, reAddBibResource} {
		if m := re.FindAllSubmatch(content, -1); m != nil {
			for _, gr := range m {
				pkgs = append(pkgs, strings.TrimSpace(string(gr[1])))
			}
		}
	}
	for _, pkg := range pkgs {
		if pkg == "" {
			continue
		}
		if _, done := out[pkg]; done {
			continue
		}
		tier, err := r.ResolvePackage(pkg)
		if err != nil {
			continue // unresolved: B3 surfaces the missing-file error clearly
		}
		out[pkg] = tier
	}
	return out
}

// FigureFlags reports has_pyplot / has_mermaid from the detected blocks.
func FigureFlags(blocks []models.FigureBlock) (bool, bool) {
	var hasPy, hasMm bool
	for _, b := range blocks {
		switch b.Kind {
		case "pyplot":
			hasPy = true
		case "mermaid":
			hasMm = true
		}
	}
	return hasPy, hasMm
}

// PreambleBytes reads the root file and returns everything before
// \begin{document} (the B3 warm-cache key material).
func PreambleBytes(rootTexPath string) ([]byte, bool) {
	content, err := os.ReadFile(rootTexPath)
	if err != nil {
		return nil, false
	}
	if m := reBeginDocument.FindIndex(content); m != nil {
		return content[:m[0]], true
	}
	return content, true // no document body marker: whole file is preamble
}

// DefaultCacheDir — Tier 3 checkout location (ADR-003), env-overridable for tests.
func DefaultCacheDir() string {
	if dir := os.Getenv("TEXCRAFT_CACHE_DIR"); dir != "" {
		return dir
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(os.TempDir(), ".texcraft", "cache", "ctan")
	}
	return filepath.Join(home, ".texcraft", "cache", "ctan")
}

// relToRoot renders one file's workspace-relative path ("" = the scan root).
func relToRoot(root, p string) (string, error) {
	rel, err := filepath.Rel(filepath.Dir(root), p)
	if err != nil {
		return "", err
	}
	if rel == "." {
		return "", nil
	}
	return rel, nil
}
