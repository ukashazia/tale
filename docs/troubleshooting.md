# Troubleshooting and recovery

Tale reports the smallest affected capability and preserves the last-good
snapshot where one exists. Recovery never requires `sudo`, a downloaded script,
or publishing a support bundle.

## Theme or color output is wrong

Run `tale config check` and inspect Settings for configured theme provenance and
the resolved capability reason. Valid themes are exactly `tailscale-dark`,
`tailscale-light`, and `terminal`; valid color policies are `auto`,
`truecolor`, `ansi256`, `ansi16`, and `none`. `NO_COLOR` intentionally forces
Reset-only cells. Use `terminal` when the emulator's own background should
remain unpainted, or force `ansi16`/`none` when a multiplexer advertises more
color support than it reliably renders. Tale does not probe background
appearance automatically.

## Executable and daemon

Use `tale config path` and verify the configured Tailscale socket or named pipe.
The LocalAPI endpoint is the source for status and preferences; a missing or
denied CLI is an independent capability error. A stopped or unavailable daemon
is different from a missing executable; start or repair Tailscale using
the operating system’s normal administrator-approved procedure, then refresh
Tale. Do not grant broad permissions or run Tale as root merely to hide the
error. Tale does not probe alternate endpoints or fall back to CLI status.

## Tale cannot find the tailscale command

The failure names the command, every location that was checked, and what to do:

```text
the tailscale command was not found. Looked in /usr/bin/tailscale,
/opt/homebrew/bin/tailscale and 3 more. Install Tailscale or pass --tailscale-path.
```

A command that exists but cannot be run reports that separately, with the path it
found, so a permissions problem is never mistaken for a missing install. Tale
searches an explicit `--tailscale-path` first, then `TAILSCALE_PATH`, then the
configured path, then every entry of `PATH`.

The daemon and the CLI are discovered separately, and each carries its own
generation, so a daemon status update can never discard a CLI discovery result.
The top line can read `connected locally` while CLI-backed actions are still
unavailable, but `CLI discovering` is transient: it resolves to `available` or to
a named failure within a command timeout. A `local` view that stays on
`CLI discovering` with `executable  not returned` is a defect, not a slow probe.

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

## Interaction and navigation recovery

`Esc` cancels an active `:`, `/`, `a`, `y`, or help interaction; it does not
navigate in normal mode. Use `[` and `]` for view back/forward. A boundary notice
means there is no older/newer frame. Navigating after going back intentionally
discards the forward branch. If a restored resource no longer exists, Tale
selects the first deterministic visible resource and reports that the previous
selection disappeared. Resize does not discard active editor text; below 60x18
the prompt and `Esc cancel` remain visible with the minimum-size message.

## The cursor blinks unevenly while a prompt is open

Tale never sets the cursor shape or blink, so the rhythm is the terminal's own.
Terminals restart that rhythm whenever the cursor moves, and every repaint has to
move the cursor back to the prompt after writing cells. A prompt over a view that
keeps receiving data will therefore blink to the beat of those updates.

To see what is repainting, run with `TALE_RENDER_TRACE` set to a file path. Each
repaint appends a line naming the event that caused it:

```text
TALE_RENDER_TRACE=/tmp/tale-render.log tale
```

```text
    6.850s repaint after input
    9.204s repaint after local
   11.560s repaint after local
```

Only `input` lines are your keystrokes. Regular non-`input` lines while you are
not typing are the source of an uneven blink.

## Doctor support bundle

Run `tale doctor` for a bounded report. To save one, choose an explicit new
path: `tale doctor --output /path/to/new-support-bundle.json`. Existing files
are rejected to prevent accidental overwrite. Inspect the JSON locally before
sharing it and remove any local context that makes a path identifying; never
upload it automatically or include credentials.
