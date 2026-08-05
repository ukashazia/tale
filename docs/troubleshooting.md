# Troubleshooting and recovery

Tale reports the smallest affected capability and preserves the last-good
snapshot where one exists. Recovery never requires `sudo`, a downloaded script,
or publishing a support bundle.

## Executable and daemon

Use `tale config path` and verify the configured Tailscale socket or named pipe.
The LocalAPI endpoint is the source for status and preferences; a missing or
denied CLI is an independent capability error. A stopped or unavailable daemon
is different from a missing executable; start or repair Tailscale using
the operating system’s normal administrator-approved procedure, then refresh
Tale. Do not grant broad permissions or run Tale as root merely to hide the
error. Tale does not probe alternate endpoints or fall back to CLI status.

## Authentication, scopes, and plan restrictions

For local mode, distinguish logged-out state from a daemon failure. For admin
mode, verify the selected profile, credential kind, and least-privilege scopes.
`401` means the credential is not accepted; endpoint-specific `403` means the
credential or plan cannot use that resource; `429` and `5xx` remain retryable
transport classifications with bounded recovery. No token is printed while
diagnosing these cases.

## Unsupported output and damaged configuration

An unsupported Tailscale output identifies the client/platform and affected
operation while retaining the last-good resource state. It does not try a
legacy parser. A damaged configuration is reported by `tale config check`.
Repair it from a reviewed backup or recreate it explicitly; Tale does not
migrate, guess, or silently rewrite obsolete configuration.

## Secrets and unknown mutations

A one-time secret result cannot be reopened after close. If clipboard copy
fails, keep the result visible and retry or transcribe it through the intended
secure channel; do not place it in a task log or support bundle. If a mutation
times out after dispatch, its outcome is `unknown` until a fresh read or audit
correlation proves the result. Never press a retry that would automatically
repeat the mutation; use the read/inspect action first.

## Exports and temporary files

JSON and CSV exports are bounded, redacted as required, and written atomically.
If a write is short, denied, or cannot be renamed, the previous file remains
untouched and the partial temporary file is cleaned up. Check the destination
directory and permissions without deleting the entire state directory.

## Terminal recovery

If an interrupted handoff leaves the terminal in an unexpected mode, return to
the parent shell and use that shell’s normal terminal-reset operation. Reopen a
new terminal session if necessary. Tale restores terminal state on handled
errors, cancellation, and fatal render paths; the terminal matrix is still
evidence-gated and unsupported terminals are not claimed.

## Doctor support bundle

Run `tale doctor` for a bounded report. To save one, choose an explicit new
path: `tale doctor --output /path/to/new-support-bundle.json`. Existing files
are rejected to prevent accidental overwrite. Inspect the JSON locally before
sharing it and remove any local context that makes a path identifying; never
upload it automatically or include credentials.
