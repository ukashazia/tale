# Decision 0003: supported platform and client matrix

Status: accepted with a reduced release-candidate scope; full 1.0 support is blocked

Date: 2026-08-05

## Decision summary

Tale does not claim a fully Supported target, terminal, keyring backend, or
Tailscale client family for the current release candidate. The repository has
automated mock and fake-adapter evidence on the current host, but it does not
have the real-platform and real-client evidence required by Specification 09.
Rows below remain Experimental or omitted until their complete core-flow matrix
and terminal evidence are recorded.

This is a deliberate support-scope reduction. A passing semantic version check,
a cross-compiled binary, or a Linux-shaped fixture is not evidence for a
platform or client support claim.

## Evidence available at the decision gate

The release environment on 2026-08-05 reported:

| Item | Evidence |
| --- | --- |
| host OS/architecture | macOS 26.6, `aarch64-apple-darwin` |
| Cargo | Cargo 1.97.0, host `aarch64-apple-darwin` |
| Tailscale executable | not installed on `PATH` |
| terminal environment | `TERM=dumb`; no attached terminal emulator evidence |
| tmux | not exercised |
| repository local fixtures | Tailscale 1.98.9 Linux fixtures; fictional and redaction-reviewed; no macOS or Windows fixture family |
| Control API ledger | frozen source date 2026-08-03; Phase 6 recheck 2026-08-04 |
| code checks | `cargo fmt`, locked check, locked all-target tests, and locked Clippy passed before Phase 9 edits |

The local fixture family establishes a parser contract for the Linux client
shape. It does not establish that the same output, executable behavior, socket,
permissions, signals, or terminal handoff work on another platform.

## Candidate target evaluation

| Rust target | Matrix result | Evidence and missing gate |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Experimental | Linux 1.98.9 parser fixtures exist; no release-runner core-flow, keyring, terminal, or real-client evidence |
| `aarch64-unknown-linux-gnu` | Experimental | Linux 1.98.9 parser fixtures exist; no release-runner core-flow, keyring, terminal, or real-client evidence |
| `x86_64-apple-darwin` | Omitted | no target-specific build and platform matrix evidence |
| `aarch64-apple-darwin` | Experimental | host mock/fake suites run; no installed macOS Tailscale client, real daemon, standalone-client LocalAPI check, or named-terminal evidence |
| `x86_64-pc-windows-msvc` | Omitted | no Windows runner, named-pipe/keyring/ACL evidence, or Windows Terminal evidence |

No artifact is declared release-supported for these rows. The current host may
be used for deterministic mock, fake HTTP, and parser tests only.

## Client range

The minimum fixture-backed client version is Tailscale 1.98.9 on Linux. It is
the only local client/output family currently represented by committed fixtures.
The release candidate therefore makes no Supported client-family claim. The
following output rule is mandatory for every future supported row:

- a fixture manifest names the exact client version, platform, command, argv,
  exit status, output files, capture date, and redaction review;
- required-field changes return `UnsupportedOutput` with version and platform;
- additive unknown fields remain accepted only where the existing DTO contract
  explicitly permits them;
- there is one parser per proven output family and no legacy fallback chain.

The Mac App Store Tailscale client remains outside the local preference
transport decision in `docs/decisions/0001-local-preferences-transport.md`.
No client family is promoted from Experimental without a fresh contract review.

## Platform-specific boundaries

### Local client and process

The shared domain and action types are platform-neutral. Native `Path` and
`OsStr` values remain native through process and filesystem calls. Platform
modules may differ only for executable resolution, LocalAPI transport,
permissions, signals, and terminal handoff. Tale does not emulate Unix signal
or permission semantics on Windows.

### Keyring

The production credential service is the maintained `keyring` crate using the
platform-selected backend. The current evidence proves only an isolated fake
keyring contract in tests; it does not prove a real macOS Keychain, Linux
Secret Service, or Windows Credential Manager installation. No backend is
listed as Supported until add/status/remove is exercised in an isolated
namespace on that platform.

### External editor

`VISUAL` then `EDITOR` is parsed into an explicit argument vector by `shlex`.
Shell evaluation, substitutions, redirections, pipes, aliases, and shell
builtins are not supported. A platform row must exercise success, non-zero
exit, spawn failure, and terminal re-entry with a fake direct executable.

### Terminal and signals

The automated terminal contract covers the owned state lifecycle, resize,
keyboard input, cancellation, and PTY restoration on the current Unix host.
Named terminal products are not supported claims until the matrix in
`docs/support.md` contains evidence for Unicode width, color, paste/input,
optional mouse, resize, clipboard behavior, alternate-screen use, and restore.

## Evidence required to promote a row

For each target/client/terminal row, the release record must include:

1. a clean release-mode build on the target with the locked toolchain;
2. the full core-flow matrix from Specification 09.2, including real keyring
   namespace isolation and native temp-file permissions;
3. exact local-client fixtures for the minimum, intentionally supported output
   families, and the release-candidate client;
4. real executable discovery and process timeout/cancellation evidence;
5. external-editor and interactive-child handoff evidence;
6. JSON/CSV atomic-write and mock-mode evidence;
7. fake Control API contract evidence on that target;
8. named terminal evidence and a tmux run where applicable;
9. a dated report identifying the runner, OS build, toolchain, client build,
   terminal versions, and any disabled features.

Until all nine items exist, the row stays Experimental or is omitted. No
compatibility fallback, alternate parser, migration, or guessed support is
added to promote a row.

## Consequences

The release candidate can be tested and packaged for local review, but it cannot
be advertised as a supported cross-platform Tale 1.0 release from this
environment. Missing platform/client/terminal access is a release blocker, not
a reason to weaken bounds or fabricate evidence. The safe local Phase 9 work
continues, and the exact remaining evidence is listed in `docs/release-checklist.md`.
