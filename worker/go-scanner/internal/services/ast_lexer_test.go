// Package services — Phase 2 Verification Gate (b)+(c): nested \input{}/
// \include{} files are scanned concurrently, and pyplot/mermaid blocks emit
// accurate BLAKE3 hashes and exact line spans.
package services

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"lukechampine.com/blake3"

	"texcraft/worker/go-scanner/internal/models"
)

// (b) Nested \input tree: main -> chapter1 -> chapter2 (+ sibling appendix).
// Scanning must visit all leaves; -race in the gate run exercises the
// one-goroutine-per-file concurrency model.
func TestNestedInputsScannedConcurrently(t *testing.T) {
	root := t.TempDir()
	chapters := filepath.Join(root, "chapters")
	if err := os.MkdirAll(chapters, 0o755); err != nil {
		t.Fatal(err)
	}
	files := map[string]string{
		"main.tex":              "\\documentclass{article}\n\\usepackage{algpseudocode}\n\\begin{document}\n\\input{chapters/chapter1}\n\\input{chapters/appendix}\n\\end{document}\n",
		"chapters/chapter1.tex": "\\section{Chapter 1}\n\\input{chapter2}\n\\endinput",
		"chapters/chapter2.tex": "\\subsection{Deep nested}\n\\bibliography{refs}\n",
		"chapters/appendix.tex": "\\appendix\n\\addbibresource{refs.bib}\n",
	}
	for rel, body := range files {
		if err := os.WriteFile(filepath.Join(root, rel), []byte(body), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	manifest, err := ScanTree(
		filepath.Join(root, "main.tex"),
		filepath.Join(root, "dropin"),
		nil,
		ScanOptions{CacheDir: t.TempDir(), CTANFetch: func(pkg string) error { return nil }},
	)
	if err != nil {
		t.Fatalf("ScanTree: %v", err)
	}

	seen := map[string]bool{}
	for _, f := range manifest.ScannedFiles {
		seen[f.RelPath] = true
	}
	if len(manifest.ScannedFiles) < 4 {
		t.Logf("DEBUG scanned=%d: %+v", len(manifest.ScannedFiles), manifest.ScannedFiles)
	}
	for _, want := range []string{"main.tex", "chapters/chapter1.tex", "chapters/chapter2.tex", "chapters/appendix.tex"} {
		if !seen[want] {
			t.Fatalf("nested file %q missing from scan (got %v)", want, keys(seen))
		}
	}
	// Include line numbers recorded on the direct parents.	checkIncludeLines(t, manifest, "main.tex", []uint64{4, 5}) // lines 4 and 5
	checkIncludeLines(t, manifest, "chapters/chapter1.tex", []uint64{2})
}

func checkIncludeLines(t *testing.T, m *models.DependencyManifest, rel string, want []uint64) {
	t.Helper()
	for _, f := range m.ScannedFiles {
		if f.RelPath == rel {
			if len(f.Includes) != len(want) {
				t.Fatalf("%s include lines: want %v got %v", rel, want, f.Includes)
			}
			for i := range want {
				if f.Includes[i] != want[i] {
					t.Fatalf("%s include line %d: want %d got %d", rel, i, want[i], f.Includes[i])
				}
			}
			return
		}
	}
	t.Fatalf("file %q not in manifest", rel)
}

// (c) Figure blocks: exact spans (1-based, inclusive) + BLAKE3 of verbatim source.
func TestFigureBlocksSpansAndHashes(t *testing.T) {
	root := t.TempDir()
	rootTex := filepath.Join(root, "figs.tex")
	body := strings.Join([]string{
		"\\usepackage{graphicx}",
		"\n",
		"\\begin{pyplot}",
		"  plt.plot([1,2],[3,4])",
		"\\end{pyplot}",
		"tail text",
		"\\begin{mermaid}",
		"  graph TD; A-->B;",
		"\\end{mermaid}",
		"",
	}, "\n")
	if err := os.WriteFile(rootTex, []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}

	manifest, err := ScanTree(rootTex, filepath.Join(root, "dropin"), nil, ScanOptions{
		CacheDir: t.TempDir(), CTANFetch: func(pkg string) error { return nil },
	})
	if err != nil {
		t.Fatalf("ScanTree: %v", err)
	}
	if len(manifest.FigureBlocks) != 2 {
		t.Fatalf("want 2 figure blocks, got %d", len(manifest.FigureBlocks))
	}
	if !manifest.HasPyplot || !manifest.HasMermaid {
		t.Fatalf("flags: has_pyplot=%v has_mermaid=%v", manifest.HasPyplot, manifest.HasMermaid)
	}

	lines := strings.Split(body, "\n")
	// Scene: file lines (1-based); body lines array is 0-based there.
	// pyplot: starts line 3 ("\begin{pyplot}") with col of the backslash: lines[2][0].
	expectSpan(t, manifest, "pyplot", 4, 6, blockHashForLines(lines, 3, 5))
	// mermaid: lines 8..10 (lines[7..9]).
	expectSpan(t, manifest, "mermaid", 8, 10, blockHashForLines(lines, 7, 9))
}

func expectSpan(t *testing.T, m *models.DependencyManifest, kind string, start, end uint64, hash string) {
	t.Helper()
	for _, b := range m.FigureBlocks {
		if b.Kind == kind {
			if b.StartLine != start || b.EndLine != end {
				t.Fatalf("%s span: want [%d..%d] got [%d..%d]", kind, start, end, b.StartLine, b.EndLine)
			}
			if len(hash) != 64 {
				t.Fatalf("%s: BLAKE3 hex must be 64 chars, got %d", kind, len(hash))
			}
			if b.BlockHash != hash {
				t.Fatalf("%s block hash mismatch:\n want %s\n  got %s", kind, hash, b.BlockHash)
			}
			return
		}
	}
	t.Fatalf("%s block missing", kind)
}

// blockHashForLines hashes the verbatim block source the lexer accumulates:
// lines lo..hi (0-based slice indexes) each with their '\n' terminator.
func blockHashForLines(lines []string, lo, hi int) string {
	var src string
	for i := lo; i <= hi; i++ {
		src += lines[i] + "\n"
	}
	sum := blake3.Sum256([]byte(src))
	return hexOf(sum[:])
}

func keys(m map[string]bool) []string {
	out := []string{}
	for k := range m {
		out = append(out, k)
	}
	return out
}

func hexOf(b []byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, 0, len(b)*2)
	for _, v := range b {
		out = append(out, digits[v>>4], digits[v&0x0f])
	}
	return string(out)
}
