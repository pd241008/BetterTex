// Package routes — ipc_router.go: binds the scanner's Unix-socket endpoint and
// parses wire payloads (Spec §1.2 route layer, §1.5 control plane). Windows
// builds fall back to a TCP loopback pipe handled identically.
package routes

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"

	"texcraft/worker/go-scanner/internal/controllers"
	"texcraft/worker/go-scanner/internal/models"
)

// DefaultSocketPath mirrors AGENT_HANDOFF_SPEC.md §5 Phase 2 step 4.
const DefaultSocketPath = "/tmp/texcraft_go_scanner.sock"

// SocketPath resolves the endpoint: $TEXCRAFT_SCANNER_SOCK overrides tests/CIs.
func SocketPath() string {
	if p := os.Getenv("TEXCRAFT_SCANNER_SOCK"); p != "" {
		return p
	}
	return DefaultSocketPath
}

// Serve binds the socket and handles framed ScanRequest/Envelope exchanges
// (newline-delimited JSON) until accept fails. Always expose exactly one
// request per line over the connection for simple interleaved clients.
func Serve(ln net.Listener) {
	for {
		conn, err := ln.Accept()
		if err != nil {
			return
		}
		go handleConn(conn)
	}
}

func handleConn(conn net.Conn) {
	defer conn.Close()
	scanner := bufio.NewScanner(conn)
	scanner.Buffer(make([]byte, 0, 64*1024), 16*1024*1024)
	for scanner.Scan() {
		line := scanner.Bytes()
		var req models.ScanRequest
		if err := json.Unmarshal(line, &req); err != nil {
			resp := models.Error(
				models.CodeBadPayload,
				fmt.Sprintf("malformed ScanRequest JSON: %v", err),
				middleware_base_meta(),
				"send a single-line JSON ScanRequest",
			)
			writeEnvelope(conn, resp)
			continue
		}
		writeEnvelope(conn, controllers.Scan(req))
	}
}

// middleware_base_meta exists for the parse-error path (no trace available yet).
func middleware_base_meta() models.EnvelopeMeta {
	return models.EnvelopeMeta{TraceID: "00000000-0000-7000-8000-000000000000", PushLocked: false}
}

func writeEnvelope(conn net.Conn, env models.Envelope) {
	b, err := json.Marshal(env)
	if err != nil {
		b = []byte(`{"status":"error","code":"ERR_SCAN_FAILED","data":null,"meta":{"push_locked":false},"error":{"message":"marshal failure"}}`)
	}
	conn.Write(append(b, '\n'))
}

// Listen cleans a stale socket file, binds, and returns the live listener.
func Listen(path string) (net.Listener, error) {
	if path != "" {
		os.Remove(path) // stale socket from a previous daemon run
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil && filepath.Dir(path) != "/" {
			return nil, err
		}
	}
	return net.Listen("unix", path)
}
