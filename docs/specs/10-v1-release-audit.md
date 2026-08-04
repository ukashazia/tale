# Specification 10 — Tale 1.0 independent release audit

- Work type: independent audit, not an implementation phase
- Target: the complete implementation of Specifications 01–09
- Depends on: Specifications 01–09 reported complete
- Repository mutation: prohibited
- Tailnet or Control API mutation: prohibited
- Required output: one evidence-backed audit report in the agent's final response

This specification instructs an independent agent to determine whether the
current Tale working copy satisfies its documented 1.0 contracts. Completion
of this audit means that the report is complete and truthful. It does not mean
that Tale passed.

The auditor does not implement fixes, reformat files, regenerate committed
artifacts in place, edit documentation, change configuration, create a release,
or weaken a requirement. Findings are delivered to the maintainer for a later,
separately authorized change.

## 10.0 Audit authority and non-negotiable boundaries

Before running any other command, read completely:

```text
AGENTS.md
docs/roadmap.md
docs/architecture.md
docs/product.md
docs/ux.md
docs/configuration.md
docs/research.md
docs/specs/01-tui-foundation.md
docs/specs/02-local-observer.md
docs/specs/03-local-operator.md
docs/specs/04-local-services.md
docs/specs/05-admin-observer.md
docs/specs/06-admin-operator.md
docs/specs/07-access-security.md
docs/specs/08-operational-depth.md
docs/specs/09-one-zero-hardening.md
docs/decisions/0001-local-preferences-transport.md
docs/decisions/0002-device-ip-key-mutations.md
docs/decisions/0003-supported-platform-client-matrix.md
docs/contracts/control-api-2026-08-03.md
docs/support.md
docs/security.md
docs/install.md
docs/troubleshooting.md
docs/release-checklist.md
docs/phase-gates-2026-08-05.md
docs/terminal-evidence-2026-08-05.md
docs/benchmarks/phase9-2026-08-05.md
docs/dependencies-2026-08-05.md
tests/acceptance/README.md
tests/acceptance/journeys.md
```

The audit is read-only with respect to tracked and untracked repository
content. Because no repository modification is authorized, do not create a JJ
change. `jj status`, `jj diff`, and `jj log` are permitted read operations.
Never invoke Git. Do not describe, abandon, squash, rebase, bookmark, push, or
otherwise change JJ history.

The following are prohibited:

- editing, creating, deleting, renaming, or formatting repository files;
- running `cargo fmt` without `--check`, `cargo fix`, `cargo update`, or any
  generator with the repository as its output directory;
- accepting a generated diff merely to make an equality test pass;
- changing source, tests, fixtures, snapshots, documentation, `Cargo.lock`, or
  dependency policy;
- using real signing, publishing, package-registry, or remote-release actions;
- using real tailnet mutations, Control API mutations, credentials, keyring
  records, clipboard contents, or secret-bearing environment values;
- running `tailscale set`, `up`, `down`, `switch`, `login`, `logout`, `serve`,
  `funnel`, `file`, `drive`, `cert`, `update`, or any other state-changing local
  command;
- invoking Tale actions that send diagnostics, create files, contact the
  Control API, or can modify local or remote state during a real-environment
  observation;
- treating unavailable evidence as a pass.

Build output under the existing ignored `target/` directory is permitted.
Every other generated file must be written beneath one explicit temporary
directory created with `mktemp -d`. Use a task-specific variable such as
`TALE_AUDIT_ROOT`; never repurpose `HOME`, `CODEX_HOME`, or another system
variable. Do not delete a broad or unresolved path. Report the temporary path
and remove it only after validating that it is the directory created for this
audit.

If the working copy is dirty at the start, preserve it exactly. Identify the
pre-existing paths, avoid commands that could rewrite them, and distinguish
their effects from findings. A dirty tree does not authorize cleanup. If it
prevents trustworthy evidence, mark the affected checks `NOT PROVEN`.

## 10.1 Required auditor behavior

Audit adversarially but fairly:

- A documentation claim is a claim, not proof.
- A file's existence does not prove its contract.
- A test name does not prove the behavior named by the test.
- A passing unit test does not prove terminal, platform, keyring, daemon, API,
  packaging, or release-runner behavior.
- A successful mock journey does not prove a real integration.
- Code inspection alone cannot prove a runtime or cross-platform claim.
- An unavailable platform, daemon, credential, terminal, tool, or network is
  `NOT PROVEN`, never an inferred pass.
- An environmental failure must be separated from a product failure, but it
  may still block the 1.0 release gate.
- Additive behavior is not acceptable when a specification explicitly forbids
  it, even if it appears useful.
- Do not audit against proposed post-1.0 redesigns. Record those separately as
  design debt unless Specifications 01–09 already require the behavior.

Read implementation and tests directly. Trace user actions from registration
through capability evaluation, reducer/event handling, effect dispatch,
adapter invocation, verification, state replacement, rendering, task history,
and error reporting. Sample checks are insufficient where the contract says
`every`, `all`, `never`, or `only`.

## 10.2 Evidence vocabulary

Every audited requirement receives exactly one status:

| Status | Meaning |
| --- | --- |
| `PASS` | Direct evidence proves the complete applicable requirement. |
| `FAIL` | Direct evidence demonstrates a reproducible contract violation. |
| `PARTIAL` | Some required behavior is proven and some is absent or unproven. |
| `NOT PROVEN` | The required evidence cannot be produced in the audit environment. |
| `NOT APPLICABLE` | The requirement does not apply to the declared support scope, with a cited reason. |

Do not collapse `PARTIAL` or `NOT PROVEN` into `PASS`.

Every finding receives one severity:

| Severity | Definition |
| --- | --- |
| `BLOCKER` | Release safety, secret protection, terminal restoration, mutation truth, mandatory gate, support claim, or artifact reproducibility is violated or cannot meet the strict Phase 9 exit gate. |
| `HIGH` | A core journey fails, observed state can be materially false, a mutation can duplicate or affect the wrong resource, or source isolation is broken. |
| `MEDIUM` | A shipped workflow is incomplete, misleading, inaccessible at a required viewport, or inconsistent with its documented contract. |
| `LOW` | Localized quality, copy, discoverability, or polish defect without material state or safety impact. |

The final release verdict is exactly one of:

- `READY FOR MAINTAINER LOCK` — every Phase 9 exit condition passes, every
  claimed Supported row has current evidence, no `BLOCKER` or `HIGH` finding
  remains, and all required external evidence is present;
- `NOT READY` — any mandatory exit condition fails, is partial, or is not
  proven, or any `BLOCKER`/`HIGH` finding remains;
- `AUDIT INCOMPLETE` — the auditor itself could not finish the required work
  for a reason other than missing product support evidence.

There is no guessed, waived, or "ready with assumptions" verdict.

## 10.3 Preflight and provenance record

Before building, record:

- audit date and timezone;
- absolute workspace path;
- current JJ change ID, commit ID, description, parents, and `jj status`;
- host OS, architecture, and Rust target;
- `rustc`, Cargo, JJ, and available `tailscale` versions;
- terminal identity and relevant capability classification without dumping the
  environment;
- whether the network is available for dependency/advisory metadata;
- whether `cargo-deny` and all documented audit tools are installed;
- exact supported/experimental/omitted rows currently claimed by
  `docs/support.md`;
- SHA-256 of `Cargo.lock` and the release binary audited, when built.

Use read-only commands and redact usernames, hostnames, tailnet names, device
names, IP addresses, domains, credentials, absolute private paths, and terminal
session content from the report. Exact tool versions, target triples, file
paths inside the repository, fixture versions, and fictional identifiers are
safe.

Capture the initial `jj status` and `jj diff --summary`. Capture both again at
the end and require identical repository content state. A pre-existing dirty
state may remain dirty; the audit must not add to it.

## 10.4 Specification traceability audit

Build a traceability matrix covering every numbered requirement, acceptance
criterion, verification section, manual journey, phase gate, and explicit
prohibition in Specifications 01–09.

Each matrix row contains:

```text
requirement ID or source heading
source file and line
applicability
implementation owner file/symbol
test or fixture evidence
runtime/manual evidence when required
status
finding IDs
```

At minimum, audit these phase themes:

| Phase | Required focus |
| ---: | --- |
| 1 | terminal lifecycle, bounded event/task model, routing, focus/overlay rules, configuration precedence, mock determinism, responsive rendering, help, and restoration on every exit/failure path |
| 2 | executable capability discovery, exact-version structured DTOs, local snapshots, last-good-state preservation, filtering/sorting/selection identity, diagnostics, cancellation, refresh generations, and no mutation reachability |
| 3 | read-only enforcement, mutation locks, previews, risk confirmations, preference authority, verification reads, exit-node/routes/accounts, unknown outcomes, and interactive terminal handoff |
| 4 | Serve/Funnel public/private semantics, transfer safety, Taildrive gating, certificate privacy, metrics/bug-report bounds, capability discovery, confirmation, and post-write verification |
| 5 | profile/config/keyring boundaries, OAuth/token lifecycle, least-privilege scopes, HTTP limits, pagination, resource-level failures, local/admin composition by exact stable ID, and independent freshness |
| 6 | device/route/DNS/user mutations, optimistic-state prohibition, conflict and unknown-outcome handling, audit correlation, partial batch truth, and separation of local advertisement from admin approval |
| 7 | policy source fidelity, editor/temp-file safety, validation/tests/preview/save, credential creation and revocation, one-time secrets, audit inspection, redaction, and remote-versus-local credential separation |
| 8 | deterministic fleet findings, bounded flow logs, webhook/log-stream contracts, saved-view semantics, deterministic secret-free export, authoritative Access Explorer, mouse parity, and large-data responsiveness |
| 9 | support evidence, client/API compatibility, resilience, performance, memory bounds, security, terminal/accessibility matrix, CLI artifacts, doctor, packaging, reproducibility, documentation, and all fifteen acceptance journeys |

Requirements inherited from earlier phases remain active unless a later
specification explicitly replaces them. Identify genuine contradictions rather
than silently choosing the easier requirement.

## 10.5 Architecture and implementation audit

Inspect the entire repository-authored Rust surface, tests included. Verify:

### Ownership and state correctness

- Domain models do not depend on Ratatui, process, HTTP, keyring, or terminal
  implementation details.
- Local and admin adapters cannot write UI state directly.
- Snapshots are replaced atomically only after successful decoding.
- Failed refreshes retain the last good snapshot and update only source
  metadata.
- Generations prevent stale work from replacing newer state.
- Selection is stable-ID based, not row-index based.
- Filter, sort, scroll, form, overlay, and route state have the documented
  ownership and do not leak into unrelated routes.
- Local and admin resources compose only through exact shared stable IDs.
- Source failure is isolated; loss of one source does not erase another.

### Action and mutation correctness

- Every advertised action has an action ID, capability rule, handler, effect,
  result path, error path, help entry, and test.
- No hidden key bypasses capability, read-only, confirmation, or risk gates.
- Global and profile read-only locks are enforced at dispatch/effect boundaries,
  not only by disabled rendering.
- Destructive actions require the specified confirmation and target identity.
- No mutation updates verified domain state optimistically.
- Timeout and unknown-outcome paths never retry mutations automatically.
- Batch actions report each item independently and never imply atomicity.
- Local and remote actions cannot target the wrong plane.

### I/O and bounds

- No shell command construction, `sudo`, privilege helper, or interpolated
  command string exists.
- Process argv, stdin mode, output caps, timeout, cancellation, redaction, and
  terminal handoff match their specifications.
- HTTP origin, method, path/query encoding, authentication, redirect,
  decompression, response-size, pagination, timeout, and error classification
  are endpoint-specific and bounded.
- Every queue, task history, stream, cache, log window, request body, response
  body, parser, and export is bounded as documented.
- File writes are explicit, atomic where required, permission-aware, and refuse
  overwrite without the required confirmation.
- Temporary secret files and buffers have documented lifetime and cleanup.

### Static prohibitions

Run the repository's syntax-aware security scan and independently inspect
candidate matches. Repository-authored Rust, including tests, must contain no
executable use of:

```text
unsafe
unwrap
expect
panic!
todo!
unimplemented!
```

Also audit shell launch patterns, `sudo`, secret-bearing `Debug`, broad domain
dumps, token-bearing URLs, unredacted command/API bodies, and secrets stored in
tasks, errors, snapshots, exports, doctor output, configuration, or logs.
Comments and string literals must not be misreported as executable violations;
every candidate is manually classified.

## 10.6 Tests and fixture integrity

Inventory all unit, integration, acceptance, compatibility, snapshot,
benchmark, documentation, and security tests. For each phase, determine whether
the tests exercise the actual production boundary or only a helper.

Verify that:

- fixtures are fictional or reviewed-redacted and have the required manifests;
- fixture manifests identify exact client version, platform, command, argv,
  exit status, capture date, and redaction review where required;
- unknown additive fields, missing required fields, malformed data, oversized
  data, timeout, cancellation, and permission failures are covered;
- snapshot tests assert rendered terminal buffers at required sizes and modes,
  not merely reducer state;
- mutation tests cover timeout before dispatch, unknown outcome, server-applied
  response loss, verification failure, and audit-correlation failure;
- secret canaries traverse success and failure paths;
- tests cannot contact a real daemon, API, keyring, clipboard, editor, or
  tailnet unless explicitly isolated and authorized;
- ignored tests and feature/target conditional tests are enumerated and their
  missing evidence is reported;
- passing tests are not accepted as cross-platform proof when they ran only on
  the audit host.

Inspect test bodies for assertions that are tautological, constant, overly
broad, snapshot-only without semantic assertions, or disconnected from the
production code they claim to prove.

## 10.7 Mandatory automated command ledger

Run the following commands sequentially from the repository root. Do not run
Cargo commands concurrently against a shared target directory. Record the
exact command, start time, duration, exit status, and concise result. Preserve
the first failure and continue with later independent checks when safe.

```text
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo doc --no-deps --locked
cargo deny check
cargo test --locked --test acceptance --test compatibility --test phase_nine
cargo bench --locked --bench phase9 -- --noplot
cargo build --release --locked
```

Do not omit a failure because a broader or later command passes. If a required
tool is absent, its row is `NOT PROVEN` and the Phase 9 gate remains blocked.
Do not install tools globally or modify the user's environment without explicit
authorization.

Run generated-artifact production into the audit temporary directory, never
the repository. Compare the generated Bash, Zsh, and Fish completions and man
page byte-for-byte with their committed counterparts. A mismatch is a finding;
do not copy generated files back.

Run `cargo install --locked --path .` with `--root` pointing beneath the audit
temporary directory. It must not write into a user installation prefix.

The standard test suite is necessary but not sufficient. Also run the
repository-documented security, secret-canary, redaction, generated-artifact,
Markdown/documentation, compatibility, acceptance, and packaging checks if
they are not already demonstrably included. Record how inclusion was proven.

## 10.8 Performance, memory, and resilience evidence

Compare the Phase 9 benchmark results against every documented budget. Record
the host CPU, OS, Rust version, profile, sample count, variance, and whether the
host is the declared reference runner. A benchmark below budget on an
unqualified host is useful evidence but does not prove the release-runner gate.

Verify the benchmarks use the specified dataset sizes and production-relevant
paths:

- 5,000 devices;
- 50,000 audit events;
- 250,000 flow messages;
- 5,000 health findings;
- 4 MiB policy input/diff;
- maximum configured task history.

Inspect or run the ten-cycle retained-memory measurement. Require no unbounded
growth and no more than the specified retained-memory increase after returning
to the same idle state. If the repository contains only a written claim or no
repeatable measurement harness, mark the gate `NOT PROVEN`.

Audit fault coverage for every boundary in Specification 09: process, daemon,
HTTP, credentials, filesystem, terminal, editor/handoff, CPU work, streaming,
and cancellation. Confirm that recovery is bounded, last-good data survives,
input is not starved, and only one refresh per resource generation runs.

## 10.9 Packaging and reproducibility audit

Use isolated target and output directories beneath the audit temporary root.
Never write to `release/`, never sign, never publish, and never push.

For each target currently claimed `Supported`:

1. Verify the complete support evidence row.
2. Build with the pinned toolchain and `--locked`.
3. Generate the archive through the documented packager.
4. Inspect archive membership, ordering, paths, permissions, timestamps, and
   absence of workspace paths or secrets.
5. Produce the checksum manifest.
6. Repeat from identical source with a separate isolated target directory and
   the same fixed `SOURCE_DATE_EPOCH`.
7. Compare archive bytes and checksum bytes.

If there is no Supported target, do not invent one. Run only the documented
host dry run, label it as such, and mark the Supported-artifact release gate
`NOT PROVEN` or `FAIL` according to the current release claim.

Verify that README, install/support/security/troubleshooting documentation,
license/notices, man page, and relevant completions are present in the archive.
Confirm publication and signing remain manual custody steps.

## 10.10 Safe runtime observation

Runtime observation has two separate tracks.

### Deterministic mock/fake track

Use mock and fake adapters to inspect all routes, loading/empty/partial/stale/
forbidden/unsupported/error states, overlays, forms, action capability reasons,
tasks, cancellation, and the fifteen acceptance journeys. Mock mutations affect
only fictional in-memory state and may be exercised when the mock contract
explicitly guarantees no external adapter construction.

Exercise at minimum:

- 60x18 minimum-size behavior;
- complete 80x24 keyboard-only drill-down journeys;
- 110x30 and 160x45 reference layouts;
- ASCII and Unicode symbols;
- no color, ANSI16, ANSI256, and TrueColor presentation;
- resize while a form, confirmation, secret result, diff, live task, and
  terminal-handoff boundary is active;
- `q`, Escape/back, Ctrl+C, cancellation, ordinary failure, and fatal render
  failure restoration;
- command/filter input, completion, contextual help, action/copy selection,
  focus order, and mouse parity where enabled.

Inspect actual rendered buffers or a real PTY. Do not infer visual correctness
from reducer state. Accumulated raw ANSI output is not itself proof of a corrupt
frame; reconstruct or visually inspect the frame before reporting rendering
defects.

### Real local read-only track

This track is permitted only when a local Tailscale installation is already
available. Build the release binary from the audited source and launch only
with Tale's global read-only lock. Isolate Tale configuration, state, cache, and
output beneath the audit temporary directory so the operator's profiles and
credentials are not loaded or changed.

Observe passive discovery and navigation only. Do not dispatch diagnostics,
transfers, certificate retrieval, bug reports, service actions, preference
changes, account actions, remote sessions, admin requests, or any other effect
beyond passive local reads. Opening help or a capability-disabled action list
is allowed. Exit normally and with Ctrl+C in separate runs, then verify terminal
restoration.

Never print real tailnet identity, device/user names, addresses, domains, IDs,
paths, or credentials in the report. Summarize behavior generically.

If the daemon, named terminal, isolated keyring, platform, or credential needed
for a real journey is unavailable, record `NOT PROVEN`. Do not install,
authenticate, reconfigure, elevate, or mutate anything to obtain evidence.

## 10.11 Fifteen-journey acceptance matrix

Audit every journey from Specification 09 independently:

1. Launch with no config and observe a local tailnet.
2. Diagnose direct-versus-relay behavior and copy a redacted report.
3. Change and verify a local exit node.
4. Configure and remove a private Serve mapping.
5. Enable and disable Funnel with public-exposure confirmation.
6. Add a read-only admin profile and inspect every permitted resource.
7. Approve a device and route and later locate their audit events.
8. Edit ordered DNS configuration and refresh local diagnosis.
9. Suspend and restore a user.
10. Edit policy, fail a declared test, repair, preview, save, and inspect the
    audit diff.
11. Create an auth key, copy once, close, and prove it cannot reopen.
12. Investigate a fleet finding and export filtered evidence.
13. Lose the local daemon while admin mode remains usable.
14. Lose API authentication while local mode remains usable.
15. Cancel process, HTTP, CPU, editor, and streaming tasks and exit with the
    terminal intact.

For each journey report separately:

- deterministic mock/fake evidence;
- current-host real-environment evidence;
- required 80x24 keyboard/ASCII/no-color evidence;
- terminal restoration evidence where applicable;
- final status and findings.

Do not execute journeys 2–12 against a real tailnet, credential, clipboard, or
keyring during this audit. Their real-environment column must be satisfied by
already existing, independently reviewable evidence or remain `NOT PROVEN`.

## 10.12 Support, compatibility, and documentation claims

Treat `docs/support.md` as the sole support claim, then verify every claimed row
against dated evidence. Confirm target, OS, client version, daemon transport,
keyring, terminal, signals, process behavior, fixtures, acceptance, memory, and
release-runner evidence belong to the same claimed support scope.

Audit:

- minimum and release-candidate Tailscale client evidence;
- every intentionally supported parser family;
- fixture manifests and redaction review;
- frozen Control API ledger date and response fixtures;
- behavior for additive unknown fields and changed/missing required fields;
- absence of legacy parser fallback chains;
- named-terminal evidence rather than generic terminal assumptions;
- exact limitations for Experimental and Omitted rows;
- consistency among support, install, troubleshooting, README, man page,
  completions, doctor output, and release checklist.

An Experimental row must not be advertised as Supported elsewhere. A release
artifact cannot be called Supported merely because it compiles locally.

Check user-facing documentation against actual CLI help and runtime behavior.
Verify only shipped flags, subcommands, routes, settings, and actions are
documented. Internal/test-only controls must not leak into public help, man
pages, completions, archives, or ordinary Settings presentation when
Specification 09 excludes them.

## 10.13 Security and privacy audit

Trace every secret class listed in `docs/security.md` through input, memory,
adapter dispatch, error handling, task history, rendering, copy, persistence,
doctor, export, and destruction. Verify each allowlist and prohibition against
code and tests.

At minimum prove:

- access tokens are sent only in authorized Bearer headers;
- redirects cannot carry credentials to another origin;
- OAuth client secrets and tokens never enter URLs, argv, task output, logs,
  snapshots, exports, doctor bundles, or errors;
- auth-key and rotated webhook secrets are view-once and cannot be reopened;
- certificate private-key contents are never rendered, copied, or logged;
- policy, audit, and flow content do not leak through support bundles;
- keyring namespace tests are isolated from the operator's real records;
- temporary and exported files have required permissions and overwrite rules;
- redaction happens before storage, not only during rendering;
- zeroization claims remain accurately qualified as best effort;
- dependency advisories, sources, and licenses have current evidence and every
  exception has package, version, exposure, control, owner, and expiry.

Never include a secret, real identity, private tailnet data, or unredacted
diagnostic output in audit evidence.

## 10.14 Finding format

Assign stable finding IDs in discovery order: `AUD-001`, `AUD-002`, and so on.
Each finding must contain:

```text
ID and severity
concise title
requirement source with file and line
affected component and user journey
observed behavior
expected behavior
direct evidence
minimal safe reproduction
impact
confidence
recommended correction direction
whether it blocks maintainer lock
```

Do not propose broad rewrites when a narrow correction is supported by the
evidence. Do not implement the recommendation. Group multiple symptoms under
one finding only when they have the same demonstrated cause; otherwise keep
them separate.

Record non-contractual usability observations and planned post-1.0 work in a
separate `Design debt` section. They do not change the release verdict unless
they violate an existing Specification 01–09 requirement.

## 10.15 Required final report

The final response is self-contained and ordered as follows:

1. **Verdict** — one permitted verdict and one-sentence reason.
2. **Repository integrity** — initial/final JJ state and explicit confirmation
   that the audit changed no repository content or external state.
3. **Environment and scope** — revision, host, toolchain, available integrations,
   support claims, and redactions.
4. **Executive summary** — counts by status and severity; the five most
   consequential results.
5. **Mandatory command ledger** — exact commands, durations, exits, and result.
6. **Specification matrix** — Phase 1–9 status with evidence and finding IDs.
7. **Phase 9 exit-gate matrix** — every bullet from 09.13 separately assessed.
8. **Fifteen acceptance journeys** — deterministic and real evidence separated.
9. **Support/platform matrix** — claim versus evidence and missing proof.
10. **Findings** — complete `AUD-NNN` records ordered by severity then ID.
11. **Security and secret-flow result**.
12. **Performance, memory, packaging, and reproducibility result**.
13. **Not proven** — every missing external or environmental proof.
14. **Design debt** — observations outside the 1.0 contract.
15. **Recommended remediation order** — findings only, without edits.

Do not bury failures beneath a long test list. Lead with the verdict and release
blockers. Do not claim that Phase 9, Tale 1.0, a platform, client, terminal, or
artifact is ready when its required evidence is missing.

## 10.16 Audit completion gate

The audit is complete only when:

- the complete required source set was read;
- every Specification 01–09 requirement is represented in the traceability
  matrix;
- every mandatory command was attempted and recorded;
- code, tests, fixtures, generated artifacts, docs, security, dependencies,
  performance, memory, resilience, packaging, and reproducibility were audited;
- every Phase 9 exit-gate bullet has an explicit status;
- all fifteen journeys have separate deterministic and real-evidence statuses;
- every support claim is reconciled with dated evidence;
- every failure, partial result, and missing proof is reported without waiver;
- no real secret or tailnet identity appears in the report;
- no repository content, JJ history, tailnet state, Control API state, keyring,
  clipboard, signing identity, package registry, or remote release was changed;
- the final `jj status` and `jj diff --summary` prove the audit added no
  repository changes beyond any state that existed before it began.

If any audit activity would cross these safety boundaries, skip it, mark the
requirement `NOT PROVEN`, explain the missing authority, and continue with the
remaining safe evidence.
