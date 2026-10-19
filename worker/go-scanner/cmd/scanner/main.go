// Command scanner — the Go heuristic dependency & AST scanner daemon (B1).
// Binds /tmp/texcraft_go_scanner.sock ($TEXCRAFT_SCANNER_SOCK overrides) and
// serves Unified JSON Envelopes over newline-delimited JSON (Spec §1.2/§1.4).
package main

import (
	"fmt"
	"log"
	"os"

	"texcraft/worker/go-scanner/internal/routes"
)

func main() {
	path := routes.SocketPath()
	ln, err := routes.Listen(path)
	if err != nil {
		log.Fatalf("scanner: bind %s: %v", path, err)
	}
	defer ln.Close()

	fmt.Fprintf(os.Stderr, "scanner: listening on %v (B1_GO_HEURISTIC_SCAN ready)\n", ln.Addr())
	routes.Serve(ln)
}
