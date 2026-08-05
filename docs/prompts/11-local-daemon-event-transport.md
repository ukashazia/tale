# Invocation prompt — Specification 11

Copy the prompt below into a fresh Codex agent session opened at the Tale
repository root.

```text
Implement Tale Specification 11, “Local daemon event transport,” end to end.

Repository and authority

- Work only in the Tale repository provided as your current working directory.
- Read AGENTS.md completely before any other action and obey it literally.
- This repository uses Jujutsu. Never invoke Git, never rewrite history, never
  push, and never alter an existing change description or parent.
- Before modifying any file, inspect the working copy with `jj status`. If it is
  clean and there is no pre-existing Phase 11 change, start exactly one new
  change with:

    jj new -m "refactor: replace local polling with daemon events"

- If the working copy has changes you did not create, preserve them. If they
  overlap Phase 11, stop and report the exact overlap rather than guessing.

Required reading

Read these files completely before implementation:

  AGENTS.md
  docs/specs/10-v1-release-audit.md
  docs/specs/11-local-daemon-event-transport.md
  docs/architecture.md
  docs/product.md
  docs/ux.md
  docs/configuration.md
  docs/support.md
  docs/security.md
  docs/troubleshooting.md
  docs/decisions/0001-local-preferences-transport.md
  docs/decisions/0003-supported-platform-client-matrix.md

Then inspect the current local adapter, reducer/effects/runtime, source models,
configuration/CLI/doctor, UI source presentation, tests, fixtures, completions,
and man page. Search the whole repository for status polling,
`tailscale status --json`, `PreferenceClient`, `local.refresh_interval`, local
capability assumptions, and process-spawn assertions.

Implementation objective

Replace local status polling and the preferences-only transport with one typed
LocalAPI client for status, preferences, and `watch-ipn-bus`. The watch stream is
an invalidation source; authoritative status/preferences reads create domain
snapshots. Keep typed, direct-process CLI execution for mutations and other
commands that do not have an approved LocalAPI contract.

Implement every requirement, race rule, bound, config replacement, platform
boundary, test, document, and exit condition in Specification 11. In particular:

- create Decision 0004 and the pinned LocalAPI contract ledger before protocol
  implementation;
- verify the Tailscale source tag, headers, endpoints, watch mask, framing, and
  platform endpoints from primary source rather than memory;
- use maintained HTTP/1 machinery over Unix sockets/Windows named pipes; do not
  hand-parse HTTP;
- make daemon observation and CLI execution independent capabilities;
- connect the watcher before authoritative bootstrap reads and close the
  startup race exactly as specified;
- implement bounded newline framing, targeted invalidation, coalescing,
  generations, dirty follow-up reads, reconciliation, reconnect, cancellation,
  and last-good behavior;
- add socket configuration and align supported CLI invocations to that socket;
- remove `local.refresh_interval`, the old status poll/parser, and the separate
  PreferenceClient path with no alias, fallback, or dormant compatibility code;
- ensure `--mock` and tests cannot reach the real daemon or tailnet;
- update all current architecture, UX, configuration, support, troubleshooting,
  completion, man-page, and doctor claims.

Non-negotiable constraints

- Do not implement Specification 12, 13, or 14.
- Do not redesign the command/filter/action/help UI or navigation stack.
- Do not add a custom theme or new product domain.
- Do not write undocumented LocalAPI mutation endpoints.
- Do not add CLI observation fallback.
- Do not shell out through a shell.
- Do not use unsafe, panic, unwrap, expect, or non-idiomatic Rust.
- Do not mutate a real daemon, tailnet, Control API, keyring, clipboard, remote,
  release, or user configuration.
- Do not weaken or delete a test merely to make the suite pass.

Working method

1. Establish the current baseline and record any pre-existing failures.
2. Create the required decision/contract documents from verified primary source.
3. Implement the smallest complete vertical daemon-observation path.
4. Wire reducer/effects/runtime and independent capabilities.
5. Remove the obsolete paths rather than layering compatibility.
6. Build the deterministic fake LocalAPI transport and race/failure tests.
7. Update current documentation and generated CLI artifacts.
8. Run the complete Specification 11 exit gate and inspect the final JJ diff.

Use `rg`/`rg --files` for discovery and `apply_patch` for edits. Reuse current
dependencies when they provide the required facilities; inspect their docs and
types before adding packages. Keep the render path free of I/O and use bounded
channels/buffers and owned cancellation.

Required validation

At minimum run:

  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features --locked

Also run every focused fake-daemon, runtime, local client/observer/operator,
configuration, CLI, compatibility, acceptance, security, documentation,
terminal-restoration, artifact, and platform gate required by Specification 11.
Do not claim a platform that was not exercised on that platform.

Final response

Report:

- the outcome and JJ change ID/description;
- the implemented daemon/CLI boundary and obsolete paths removed;
- decisions/contracts and dependency changes;
- tests and exact command results;
- supported, experimental, and not-proven platform evidence;
- any remaining issue or unavailable external evidence;
- explicit confirmation that you did not implement later specifications, invoke
  Git, rewrite history, push/publish, or mutate real local/remote state.

Do not stop at a plan. Continue until every in-scope requirement is implemented
and verified, or until a concrete blocker requires maintainer authority.
```
