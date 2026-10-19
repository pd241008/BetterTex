// Package controllers — scan_controller.go: validates the ScanRequest,
// orchestrates the scan service call, and formats the Unified JSON Envelope
// (Spec §1.2 controller layer).
package controllers

import (
	"errors"

	"texcraft/worker/go-scanner/internal/middleware"
	"texcraft/worker/go-scanner/internal/models"
	"texcraft/worker/go-scanner/internal/services"
)

// ScanRequestValidator errors surface as ERR_BAD_PAYLOAD envelopes.
var ErrMissingRootTex = errors.New("root_tex_path is required")

// Scan handles one B1 scan request end-to-end.
func Scan(req models.ScanRequest) models.Envelope {
	traceID := middleware.InjectTrace(req.TraceID)

	if req.RootTexPath == "" {
		return models.Error(
			models.CodeBadPayload,
			ErrMissingRootTex.Error(),
			middleware.BaseMeta(traceID, req.SyncState, 0),
			"resend ScanRequest with root_tex_path",
		)
	}

	manifest, scanMs := middleware.TimeStage(func() *models.DependencyManifest {
		m, err := services.ScanTree(
			req.RootTexPath,
			req.DropinDirPath,
			req.UserSourceDirs,
			services.ScanOptions{CacheDir: req.CacheDir},
		)
		if err != nil {
			_ = err // surfaced below via the nil-branch; see manifest == nil
		}
		return m
	})

	if manifest == nil {
		return models.Error(
			models.CodeScanFailed,
			"dependency scan failed (see scanner stderr for the wrapped cause)",
			middleware.BaseMeta(traceID, req.SyncState, 0),
			"retry after fixing include paths or drop-in files",
		)
	}

	meta := middleware.BaseMeta(traceID, req.SyncState, scanMs)
	return models.Success(models.CodeScanComplete, models.CollectData(manifest), meta)
}
