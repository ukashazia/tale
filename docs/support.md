# Tale support matrix

This is the sole document that makes a support claim. A row is not Supported
unless its complete core-flow matrix has passed on the named target, client,
keyring, and terminal combination.

## Current claim

There are no Supported 1.0 platform rows yet. The release candidate must not be
advertised as cross-platform or as supporting a real Tailscale client family.

| Target | Status | Evidence | Limitations |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Experimental | Linux Tailscale 1.98.9 fixtures and fake-adapter tests | No release runner, real daemon, real keyring, or named terminal evidence |
| `aarch64-unknown-linux-gnu` | Experimental | Same parser/fixture family only | No ARM64 Linux runner or real-client evidence |
| `aarch64-apple-darwin` | Experimental | Local development host, Tailscale 1.98.9 version output, and mock/fake tests | No real LocalAPI session, mutation matrix, keyring, or named-terminal evidence |
| `x86_64-apple-darwin` | Omitted | No target evidence | Build and core-flow evidence required before inclusion |
| `x86_64-pc-windows-msvc` | Omitted | No Windows runner or signal/keyring/terminal evidence | Unix behavior is not emulated on Windows |

The committed fixture family is Tailscale 1.98.9 on Linux. It is fictional or
reviewed redacted test data, not a live tailnet capture. The LocalAPI contract,
watch framing, monotonic reconnect generations, and cancellation are exercised
by a Unix-socket fake daemon.

## Client and API scope

The minimum fixture client is Tailscale 1.98.9 on Linux. No client version is
Supported until the minimum patch, each intentionally supported output family,
and the release-candidate client have been run through the core-flow matrix.
Unknown additive JSON fields are accepted only by DTOs that already allow them;
missing required fields produce `UnsupportedOutput` and retain the last-good
state. There is no legacy parser chain.

The Control API ledger is frozen at the source date recorded in
`docs/contracts/control-api-2026-08-03.md`. Tale does not version-pin the hosted
API. A decode failure is isolated to its resource and must update the ledger and
fixture directly.

## Terminal and keyring scope

No named terminal emulator, tmux environment, alternate-screen behavior,
clipboard, paste, or Unicode rendering combination is Supported. Automated
Ratatui buffer tests cover the built-in themes and color projections, but they
do not replace named-emulator manual evidence or promote a platform row.

The `keyring` dependency is covered by isolated fake-store contract tests. No
real macOS, Linux, or Windows keyring backend is Supported. External-editor
values are parsed as arguments with `shlex`; Tale never invokes a shell.

## Promotion evidence

Before changing a row to Supported, attach dated evidence for:

1. the exact target and stable Rust toolchain;
2. the installed Tailscale client version and platform;
3. CLI/config/path resolution, including non-ASCII and space-containing paths;
4. terminal enter/restore, resize, Ctrl+C, error, and handoff behavior;
5. isolated keyring add/status/remove and editor handoff;
6. process, timeout, cancellation, capped output, and atomic-write tests;
7. the client fixture manifest and redaction review;
8. the mock and admin fake-server acceptance suites;
9. peak-memory and ten-cycle retained-memory measurements.

Until those records exist, the correct status is Experimental or Omitted. The
missing platform and terminal evidence is a release blocker, not a guessed
pass.
