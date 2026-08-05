# Specification 14 — Post-1.0 integration hardening

- Implementation phase: 14
- JJ change description: `chore: harden post-1.0 architecture and interface`
- Depends on: Specifications 11–13 complete
- Produces: an evidence-backed release candidate containing the daemon,
  interaction, navigation, and theme redesigns

This phase integrates and hardens the post-1.0 redesign. It adds no product
domain. Its job is to find and fix contradictions, races, leaks, inaccessible
states, platform gaps, performance regressions, obsolete paths, and misleading
documentation introduced or exposed by Specifications 11–13.

## 14.0 Phase contract

### User-visible result

Tale behaves as one product rather than three adjacent redesigns: daemon events
update the screen without disrupting prompts or history; navigation restores
current data in the selected theme; source failures remain clear in every color
mode; actions use the selected socket and verification path; and terminal exit
is reliable through stream, modal, form, and theme failures.

### In scope

- cross-feature state-machine audit and fixes;
- deterministic and optional read-only integration journeys;
- daemon/event stress, restart, cancellation, and resource bounds;
- complete keyboard/mouse/resize/theme/platform matrices;
- performance and idle-resource budgets;
- security, privacy, dependency, and terminal-lifecycle re-audit;
- removal of every superseded post-v1 path;
- current architecture, UX, configuration, support, troubleshooting, and
  release evidence;
- reproducible package dry runs for support rows proven by evidence.

### Explicitly out of scope

- new Tailscale resources, mutations, LocalAPI endpoints, or Control API calls;
- user-authored themes, key remapping, plugins, macros, or compatibility modes;
- publishing, pushing, signing with user credentials, or changing a remote;
- mutating a real daemon, tailnet, keyring, clipboard, configuration, or service
  during automated or default manual validation;
- claiming an unavailable platform based on compilation alone;
- weakening a Specification 11–13 contract to close the phase;
- retaining old code as a hidden recovery path.

## 14.1 Preflight and baseline

Before changing code:

1. read `AGENTS.md`, Specifications 10–14, every decision/contract introduced
   by 11–13, and the current core product/architecture/UX/configuration docs;
2. read the Specification 10 audit report supplied by the maintainer and record
   which findings are already resolved, still applicable, or unrelated;
3. capture `jj status`, current change/parents, Rust/tool versions, supported
   platform claims, and exact Tailscale contract versions;
4. run the complete existing check suite without modifying generated artifacts;
5. record pre-hardening benchmark and resource measurements;
6. create `docs/phase-gates-<date>-post-v1.md` with all required gates marked
   `PENDING`, never pre-filled as passing.

Use a new JJ change with the phase description. Never use Git or modify history.
Do not erase pre-existing work. If the working copy contains unrelated changes,
identify and preserve them; ask the maintainer before touching an overlapping
path.

The baseline document records environmental failures separately from product
failures. Missing external evidence is `NOT PROVEN`, not pass.

## 14.2 Integrated state-machine invariants

Audit reducer, effect runner, watcher, history, interaction mode, theme, and
terminal session as one transition system. Add tests and fixes proving:

### Data and event invariants

- only the reducer mutates UI state;
- watcher events carry a watcher generation and cannot invalidate a replacement
  endpoint/source generation;
- read results carry resource generation and cannot overwrite newer snapshots;
- one daemon restart yields at most one reconnect sequence and one full resync;
- reconciliation and mutation verification share per-resource read
  serialization rather than racing duplicate commits;
- status/prefs updates preserve selected stable resource identity when present;
- an event while command/filter/help/transient is active updates underlying data
  without closing, submitting, moving the cursor, or discarding input;
- an event that removes the selected resource follows deterministic selection
  repair and does not rewrite stored history frames;
- history restoration always reads the latest snapshot, never a captured copy;
- switching theme changes styles only and cannot restart adapters or requests;
- source failure preserves last-good data and appears in header, detail source
  section, help/disabled reasons, and no-color output;
- successful CLI mutation is not shown as success until LocalAPI verification;
- shutting down cancels watcher, reads, process tasks, completion/filter work,
  and reconciliation exactly once.

### Interaction invariants

- exactly one interaction mode owns ordinary key input;
- alerts/confirmations prevent underlying action activation;
- no global shortcut inserts or executes while a text editor owns input;
- `q` never travels history and `[`/`]` never invoke a service section action;
- command/filter completion results have generation IDs and stale candidates
  cannot replace candidates for edited text;
- disabled action state is recalculated after source/capability changes while a
  transient/help surface is open;
- if an action disappears entirely, its old key becomes unknown rather than
  invoking the item that shifted into its visual position;
- resize and theme change preserve command/filter buffer and cursor;
- confirmation target and risk phrase are bound to stable target IDs, not the
  current table row after refresh;
- copy and secret actions remain governed by redaction after theme/help changes.

Represent impossible state through types where practical. Do not solve ordering
bugs with arbitrary sleep, renderer-side mutation, unbounded queues, or clones
of entire source snapshots.

## 14.3 Failure-injection matrix

Create deterministic fault points at adapter/runtime boundaries, not production
feature flags. Test every row:

| Fault | Required behavior |
| --- | --- |
| socket absent at startup | admin/mock UI remains available; exact local remediation |
| socket permission denied | no CLI fallback; last-good absent; capability reason |
| watcher closes before initial reads | reconnect/full resync; no false live state |
| status succeeds, prefs fails | independent freshness; status remains usable |
| prefs succeeds, status fails | source not fully live; prefs last-good retained |
| malformed/oversized notification | bounded failure, last-good retained, reconnect |
| daemon disappears during mutation verification | task fails as unverified, not success |
| daemon restarts repeatedly | bounded one-sequence backoff; responsive UI |
| CLI missing with daemon live | observation works; CLI actions disabled |
| CLI timeout/cancel | process reaped; watcher continues; no optimistic state |
| Control API fails concurrently | local remains independent; admin marked stale |
| completion worker finishes stale | result discarded by generation |
| selected resource disappears | deterministic repair + notice |
| forward frame becomes invalid | frame restores safely against current domain |
| terminal resize during prompt/modal | input and escape path remain visible |
| theme resolution/config failure | fail before alternate screen |
| render failure after watcher starts | terminal restores and all tasks cancel |
| panic in injected worker test | terminal restores; sensitive data absent |
| channel full/receiver closed | bounded typed error or coalescing, never deadlock |

The panic row is a lifecycle fault test around controlled test-only code. It does
not permit panic, `unwrap`, or `expect` in Tale production/test implementation.

Run failure cases under a test timeout and assert task/process/socket cleanup.
Never connect fault injection to the operator's real environment.

## 14.4 End-to-end acceptance journeys

Add scripted, deterministic acceptance journeys. Each records keys, expected
state transitions, effects, fake-adapter requests, rendered-buffer assertions,
and final cleanup.

### Journey A — daemon-only first run

1. Provide a fake daemon and no CLI executable.
2. Start Tale with local enabled and admin absent.
3. Assert watcher-before-GET bootstrap and zero process spawn.
4. Assert local overview, prefs, peers, source label, theme, and disabled CLI
   action reasons.
5. Emit peer/state changes and assert targeted prompt update within budget.
6. Quit and prove stream/task/terminal cleanup.

### Journey B — restart during interaction

1. Open Devices, begin `/ owner:alice`, and leave the cursor mid-buffer.
2. Stop the fake daemon and emit connection close.
3. Assert reconnect/last-good state without closing the prompt.
4. Restart with changed peers, complete resync, and preserve valid input/cursor.
5. Commit filter, move `[` then `]`, and assert restored filter/selection uses the
   new snapshot.

### Journey C — verified local mutation

1. Start fake daemon and fake CLI with the same custom socket.
2. Invoke a reversible action through `a` and its mnemonic.
3. Complete bottom-sheet parameters and centered confirmation.
4. Assert exact non-shell argv includes the custom socket.
5. Return CLI success but old daemon state; assert task remains failed/unverified.
6. Repeat with verified state; only then assert success role/symbol.

### Journey D — command, completion, and history branch

1. Use `:de<Tab>` and finish a Devices command with route-valid filter.
2. Navigate to details, then Services.
3. Move backward twice and forward once.
4. Navigate to Users and assert the old forward branch is absent.
5. Assert no equivalent route duplicate and current data on every restored frame.

### Journey E — transient/help/capability change

1. Open `a`, inspect direct/nested options, and cancel.
2. Open `y`, copy a fictional field, and assert no value in UI/log/history.
3. Open `?`, filter help, and resize through all supported breakpoints.
4. Revoke a fake capability while help is open.
5. Assert the action becomes disabled with reason and its mnemonic cannot run.

### Journey F — semantic theme matrix

1. Open Settings preview in each built-in theme.
2. Cancel and prove exact restoration; apply and prove no history/source change.
3. Exercise healthy, stale, pending, public, destructive, local, admin, combined,
   selection, focus, prompt, help, diff, and secret/redacted roles.
4. Repeat in truecolor, ANSI-256, ANSI-16, and no-color.
5. Assert symbol/label parity and only semantic theme-produced colors.

### Journey G — concurrent source isolation

1. Start local daemon and admin fake server.
2. Fail admin while local events continue, then fail local while admin recovers.
3. Assert each source's data/freshness/capabilities remain independent.
4. Open a combined device and verify provenance per field.
5. Restore both and assert no global false-connected interval.

### Journey H — exit and terminal safety

For normal, command, filter, help, transient, form, confirmation, reconnecting,
and active-task states, test quit/cancel/Ctrl+C/signal/error paths. Verify cursor,
mouse capture, bracketed paste, alternate screen, raw mode, and child processes
are restored or ended according to the terminal contract.

## 14.5 Optional real-environment observation

A maintainer may explicitly authorize a read-only observation against their
local daemon. It is never part of an ordinary automated gate and never contacts
the hosted API unless separately authorized.

The observer may:

- start Tale against the configured LocalAPI endpoint;
- view redacted source health and navigation behavior;
- wait for naturally occurring notifications;
- quit and inspect process/terminal cleanup.

The observer must not:

- invoke any local or remote mutation;
- run diagnostics that upload data;
- copy/export real values;
- capture screenshots, logs, fixtures, or reports containing tailnet data;
- change Tailscale, Tale, terminal, or keyring configuration;
- infer a platform support claim from one short session.

Record only pass/fail behavior and sanitized timing. If authorization or a safe
environment is absent, mark the evidence `NOT PROVEN`; the deterministic fake
suite remains mandatory.

## 14.6 Performance and resource budgets

Measure release builds on the documented reference runner with deterministic
fictional datasets. Retain all Phase 9 budgets and add:

| Operation | Budget |
| --- | --- |
| watch notification received to render request | p95 ≤ 100 ms outside debounce bursts |
| burst first event to targeted read dispatch | ≤ 250 ms |
| watcher reconnect timer dispatch | requested delay + ≤ 50 ms |
| command completion over registered routes/100 candidates | p95 ≤ 16 ms |
| transient/help open to render request | p95 ≤ 16 ms |
| history back/forward over a prepared 5,000-row view | p95 ≤ 16 ms |
| theme switch plus 160x45 frame | p95 ≤ 33 ms |
| daemon cancellation observed | ≤ 100 ms |
| shutdown after idle watcher | ≤ 500 ms excluding active child policy |

Idle local observation must have:

- zero child-process spawns;
- one watch connection;
- no snapshot request before reconciliation unless invalidated;
- bounded channel, notification frame, history, completion, task, and error
  storage;
- no steady growth over a 30-minute paused/accelerated deterministic soak;
- no render loop caused solely by an idle watcher.

Stress with 5,000 peers, event bursts, 100 history frames, maximum help entries,
and concurrent admin refresh. Record CPU, peak resident memory, allocations where
tooling supports them, runner details, iterations, and variance. A regression is
fixed or explicitly fails the gate; budgets are not relaxed to fit results.

## 14.7 Security and privacy re-audit

Audit new boundaries for:

- socket/named-pipe path injection and permission handling;
- HTTP request smuggling, unbounded frames/bodies, and malicious daemon JSON;
- LocalAPI capability-header correctness;
- process argv/socket consistency and absence of shell execution;
- notification, completion, help, copied-value, and error redaction;
- one-time secret lifecycle through history and theme changes;
- terminal escape/control characters in daemon and admin strings;
- fake-server isolation from real default endpoints;
- dependency advisories, provenance, license, features, and unsafe transitive
  code according to existing policy;
- screenshots, snapshots, benchmark output, and support bundles containing only
  fictional/redacted data.

Run the existing dependency/security gates plus the repository's approved
advisory tools. Do not run networked updates that rewrite the lockfile. Any
accepted advisory must have scope, reachability, upstream status, owner, and
expiry; this phase may not silently renew an exception.

## 14.8 Platform and terminal evidence matrix

For every row claimed Supported, prove on that platform:

- endpoint resolution and native path handling;
- Unix socket or named-pipe request/stream behavior;
- watcher restart, cancellation, and permissions;
- custom endpoint alignment for CLI commands;
- missing-CLI daemon-only observation;
- first frame and clean exit;
- command/filter Unicode editing and completion;
- transient/help/history keys on representative terminals;
- truecolor, reduced-color, and no-color rendering;
- signal/console-control terminal restoration;
- package install/run/help/doctor behavior.

macOS client distributions are separate rows when their LocalAPI transports
differ. Windows named-pipe behavior requires Windows runtime evidence. Linux
Unix-socket success does not prove either. Unsupported/Experimental labels must
be precise and the UI must explain unavailable local observation without
offering an unimplemented fallback.

## 14.9 Obsolete-path removal audit

Search code, tests, docs, fixtures, completions, man pages, generated artifacts,
and config samples. These must be absent except in historical numbered specs or
explicit migration rationale in decisions:

```text
local status polling
tailscale status --json observation
PreferenceClient/preferences-only HTTP transport
local.refresh_interval
route_stack / q-as-back / Esc-as-route-back
centered command palette
centered filter editor
ActionPicker and CopyPicker list navigation
Services [ and ] section switching
literal widget colors outside the theme module
undocumented theme aliases or custom-theme loading
CLI fallback for LocalAPI observation
```

Historical specifications remain immutable records and may describe superseded
behavior. Current product, architecture, UX, configuration, support,
troubleshooting, install, CLI, and release documents must describe only the
post-v1 design. Do not add compatibility prose implying both behaviors work.

Add a source-policy test for machine-detectable forbidden paths. A text search
alone is not enough for semantic removal; inspect all hits and runtime wiring.

## 14.10 Documentation and release artifacts

Update and cross-check:

```text
README.md
docs/product.md
docs/architecture.md
docs/ux.md
docs/configuration.md
docs/support.md
docs/security.md
docs/install.md
docs/troubleshooting.md
docs/release-checklist.md
docs/cli/tale.1
completions/
```

Document clearly:

- Tale talks directly to the local daemon for observation;
- which local operations still require the CLI and why;
- local daemon, local CLI, and admin API capability separation;
- watcher/reconciliation/reconnect and last-good semantics;
- the new bottom interaction grammar and view history;
- action/copy transient menus and modal limits;
- built-in themes, color capabilities, no-color behavior, and persistence;
- exact supported platform/client rows and limitations;
- safe troubleshooting that does not expose tailnet data.

Regenerate and compare shell completions, man page, help fixtures, package
manifests, checksums, and dry-run archives using existing deterministic tooling.
Do not publish, push, sign with user credentials, or write outside validated
temporary/output directories.

## 14.11 Required check sequence

Run the repository's current equivalents in this order, stopping to fix product
failures rather than accepting generated diffs:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
cargo test --locked --test documentation
cargo test --locked --test terminal_restore
cargo test --locked --test acceptance
cargo test --locked --test compatibility
cargo test --locked --test security
```

Then run documented benchmark, advisory, artifact-generation comparison,
package dry-run, and platform/manual matrices. Use supported JJ inspection; do
not substitute Git. `jj diff --check` is not a valid gate where the installed JJ
does not implement it.

Record exact commands, versions, results, skipped evidence, artifact hashes, and
sanitized paths in the phase-gate document. A pass without evidence is not a
pass.

## 14.12 Exit gate and final report

Phase 14 is complete only when:

- every integrated invariant and failure-injection row passes;
- Journeys A–H pass with rendered-buffer and adapter evidence;
- all performance/resource budgets pass without unbounded growth;
- security, privacy, dependency, and terminal lifecycle re-audits pass;
- every claimed platform/client row has the required current evidence;
- obsolete code/config/UI paths are absent from current implementation/docs;
- release help, completions, man page, packages, and documentation agree;
- Specifications 11–13 remain individually green;
- the required check sequence and release dry run pass;
- `jj status` contains only intentional Phase 14 changes;
- no real daemon, tailnet, credential, clipboard, remote, or publication state
  was changed by validation;
- all `BLOCKER` and `HIGH` findings are resolved and no mandatory evidence is
  `PARTIAL` or `NOT PROVEN` for a Supported row.

The implementing agent's final report must provide:

1. outcome and current JJ change ID/description;
2. fixes grouped by transport, interaction, theme, lifecycle, security, and
   documentation;
3. exact checks and results;
4. benchmark/resource table with environment;
5. supported/experimental/not-proven platform matrix;
6. release artifact names and SHA-256 values;
7. unresolved findings with severity and evidence;
8. explicit confirmation that no Git, history, remote, publish, real mutation,
   or credential operation occurred.

Do not call the release candidate ready when a mandatory gate is unavailable.
Report `NOT PROVEN` and leave the support/release claim correspondingly limited.
