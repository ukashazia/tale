# Specification 04 — Local services

- Implementation phase: 4
- JJ change description: `feat: add local Tailscale services`
- Depends on: Specifications 01, 02, and 03 complete
- Produces: Serve, Funnel, Taildrop, Taildrive, certificate, metrics, and
  bug-report workflows for the current node

This is the Phase 4 contract from the
[end-to-end feature plan](../roadmap.md). Command shapes were checked against
the installed Tailscale 1.98.9 CLI on 2026-08-03. The implementation must still
capability-gate commands and version every structured or human-output fixture.

The implementation agent must begin this phase in a new Jujutsu change:

```text
jj new -m "feat: add local Tailscale services"
```

Do not use Git, rewrite history, or combine this phase with Phase 5. Complete
and verify Specifications 01–03 before starting this work.

## 04.0 Phase contract

### Product outcome

Tale can inspect and operate the local node's supported application-facing
features: Serve, Funnel, Taildrop, Taildrive, certificates, metrics, and bug
reports. Every operation is performed by directly spawning the installed
`tailscale` executable through the local adapter established in Specification
02. No shell is involved.

### In scope

- local Serve and Funnel status;
- guided creation, editing, and reset of node Serve and Funnel mappings;
- Taildrop file send and receive;
- alpha-gated Taildrive share management;
- certificate acquisition to explicit local paths;
- local metrics inspection;
- local bug-report creation;
- cancellation, refresh, confirmation, and task-history integration for all of
  the above.

### Explicitly out of scope

- Tailscale Control API calls or any other admin-plane behavior;
- tailnet-wide Tailscale Services, service collections, grants, or service
  ownership;
- discovered endpoints and service-specific Serve commands;
- editing the tailnet access policy needed to authorize a feature;
- web-console automation or undocumented LocalAPI endpoints;
- a general file manager, terminal emulator, SSH client, or metrics backend;
- secret-driven login;
- background daemons owned by Tale after Tale exits;
- support for a CLI flag or output shape merely because it is visible in help.

If a requested operation is unavailable in the installed CLI version, render
it as unsupported with the capability reason. Do not silently replace it with
an undocumented transport.

### Dependencies

This phase reuses, without bypassing:

- the process runner, capability probe, freshness model, and source identity
  from Specification 02;
- the mutation lifecycle, risk tiers, confirmation overlays, and post-write
  verification from Specification 03;
- the action registry, task engine, notifications, and responsive view shell
  from Specification 01.

### Required code ownership

Keep protocol DTOs, domain values, processes, and rendering separate. The
following names are required unless an existing module already has precisely
the stated responsibility:

```text
src/domain/service.rs
src/domain/transfer.rs
src/domain/certificate.rs
src/local/services.rs
src/local/transfers.rs
src/local/certificates.rs
src/ui/views/services.rs
src/ui/views/transfers.rs
src/ui/views/diagnostics.rs
tests/fixtures/local/services/
tests/fixtures/local/transfers/
```

`src/local/*` owns command construction and decoding. `src/domain/*` contains
no Ratatui or process types. Views dispatch action IDs and never construct
arguments themselves.

## 04.1 Services route and source model

### Route

Register the canonical `services` route when this phase ships. Its default
wide layout contains:

1. a section selector for Serve, Funnel, Taildrop, Taildrive, Certificates,
   Metrics, and Bug report;
2. a collection appropriate to the selected section;
3. an inspector containing source, capability, current configuration, and
   available actions.

At medium width the inspector becomes a drill-in panel. At compact width the
section selector is opened as an overlay and the collection occupies the
screen. Follow the exact breakpoints and focus rules from Specification 01.

Do not register separate top-level routes for every subsection. These
workflows share one local source and belong together.

### Source state

Add a `LocalServicesSnapshot` that records independently:

- Serve status;
- Funnel status;
- Taildrive shares;
- certificate-domain candidates;
- the capability state of every subsection;
- `observed_at`, generation, and the command version used to decode it.

A failure in one subsection must not erase successful data in another. Each
subsection uses `Loading`, `Ready`, `Stale`, `Unsupported`, or `Failed` with
the same last-good-snapshot behavior as Specification 02.

### Domain values

Use typed values rather than storing CLI fragments:

```text
Exposure = Tailnet | Public
Listener = Https(Port) | Http(Port) | Tcp(Port) | TlsTerminatedTcp(Port)
Backend = Port(Port) | HttpUrl | HttpsInsecureUrl | UnixSocket | FileSystemPath
PathMount = Root | Path(AbsoluteUrlPath)
ProxyProtocol = None | Version1 | Version2
```

`Port` accepts 1–65535. A URL path is normalized once to begin with `/`; an
empty path becomes `Root`. Preserve user-entered filesystem paths as paths,
not strings passed through a shell.

## 04.2 Serve inspection

### Command

Run:

```text
tailscale serve status --json
```

through the direct process runner. Capture stdout and stderr separately,
apply the local command timeout, and cap output using the task limits already
defined.

### Decoding contract

- Add a versioned transport DTO matching each committed fixture.
- Decode into domain mappings keyed by their listener and mount path.
- Preserve unknown JSON fields by ignoring them; reject missing fields needed
  to identify a mapping.
- Never render the raw DTO directly.
- Treat an empty valid document as `Ready` with no mappings.
- A decode failure retains the last good snapshot and reports a concise error
  with the CLI version.

The inspector must show exposure `tailnet`, listener kind and port, mount path,
backend, proxy-protocol mode when present, and last observation time.

### Tests

Commit fictional fixtures for an empty config, one HTTPS proxy, a filesystem
mount, TCP forwarding, Unix-socket forwarding on Unix, and an unknown additive
field. Add malformed and incomplete fixtures that must fail without a panic.

## 04.3 Serve editor

### Entry points

Provide `services.serve.create`, `services.serve.edit`, and
`services.serve.reset` actions. Edit pre-fills a typed form; it does not expose
raw arguments or raw Serve configuration.

The form collects:

- listener kind and port;
- optional mount path for HTTP/HTTPS listeners;
- exactly one backend;
- proxy protocol only for TCP listeners;
- a preview of the direct command arguments.

Reject incompatible fields before confirmation. Unix sockets are offered only
on platforms where the installed CLI advertises support. Filesystem backends
must exist at dispatch time. URL backends accept only `http://` or `https+insecure://`
forms supported by the CLI contract selected for this phase.

### Command construction

Construct one of these argument families without a shell:

```text
tailscale serve --bg --yes --https=<PORT> [--set-path=<PATH>] <BACKEND>
tailscale serve --bg --yes --http=<PORT> [--set-path=<PATH>] <BACKEND>
tailscale serve --bg --yes --tcp=<PORT> [--proxy-protocol=<1|2>] <BACKEND>
tailscale serve --bg --yes --tls-terminated-tcp=<PORT> <BACKEND>
```

`--bg` is mandatory so Tale never becomes the owner of a foreground Serve
process. `--yes` is allowed only after Tale's own confirmation screen has been
accepted. Never invoke a shell, `sudo`, or an interactive prompt.

An edit is implemented only with a documented replacement operation for the
installed CLI. If the command cannot replace the selected mapping precisely,
mark edit unsupported rather than resetting unrelated mappings.

### Reset

`services.serve.reset` previews that every Serve mapping on the current node
will be removed, requires a Tier 2 confirmation, and then invokes:

```text
tailscale serve reset
```

After every successful command, refresh Serve status immediately. The task is
successful only when the intended mapping is observed or, for reset, the
mapping set is empty. A successful exit followed by mismatched state is
`SucceededUnverified`, never a silent success.

### Deferred Serve surface

Do not implement `--service`, `--tun`, `--accept-app-caps`, advertise, drain,
clear, get-config, or set-raw-config in this phase. Those commands overlap the
tailnet Services model and require a separate public-contract decision.

## 04.4 Funnel inspection and editor

### Status

Inspect public mappings with:

```text
tailscale funnel status --json
```

Decode Funnel independently from Serve even if their JSON currently looks
similar. Render a persistent `PUBLIC` badge on every Funnel row and in every
confirmation screen. Never merge a public and tailnet-only mapping because
their listener and backend happen to match.

### Editor

Register `services.funnel.create`, `services.funnel.edit`, and
`services.funnel.reset`. Funnel supports only listener modes advertised by the
installed CLI; HTTP must not be offered as a public listener. Construct:

```text
tailscale funnel --bg --yes --https=<PORT> [--set-path=<PATH>] <BACKEND>
tailscale funnel --bg --yes --tcp=<PORT> [--proxy-protocol=<1|2>] <BACKEND>
tailscale funnel --bg --yes --tls-terminated-tcp=<PORT> <BACKEND>
```

Creating, replacing, or resetting Funnel is Tier 2 because it changes public
reachability. The preview must state the public listener, backend, and that
tailnet policy or node capability may still deny the operation.

Reset invokes `tailscale funnel reset`. Verify all writes by refreshing Funnel
status. Display CLI policy-denial text as a redacted, actionable error; do not
attempt to modify policy or node attributes.

## 04.5 Taildrop send

### Target discovery

Register `services.taildrop.send`. Discover candidate targets with:

```text
tailscale file cp --targets
```

The output is human-readable in the researched CLI, so isolate it behind a
versioned parser with committed fixtures. Never infer a destination from a
display name when multiple devices match.

Each `TaildropTarget` contains a stable command target, display name, device
name, online state when known, and capability reason. Unavailable targets stay
visible with their reason and cannot be dispatched.

### File selection

The first implementation uses a typed path-entry overlay, not a general file
browser. Accept one or more existing regular files. Reject directories,
standard-input marker `-`, missing paths, and an empty selection. Show
individual sizes and the aggregate size using filesystem metadata. Reading
file contents is not part of validation.

### Command

Invoke:

```text
tailscale file cp --update-interval=1s <FILE>... <TARGET>:
```

Every file and target is a separate argument. Never join them into a command
string. The confirmation preview shows the exact destination and paths.

Parse documented progress when available. If progress cannot be decoded,
show elapsed time and a spinner; never fabricate a percentage. Cancelling the
task terminates the owned child process and records that a partial remote
transfer may remain. Tale does not attempt remote cleanup.

## 04.6 Taildrop receive

### Form

Register `services.taildrop.receive`. Collect:

- an existing writable destination directory;
- conflict behavior `skip`, `overwrite`, or `rename`;
- whether to wait for one incoming batch.

`overwrite` is Tier 2 and the confirmation must explicitly name the
destination directory. The other conflict modes are Tier 1.

### Command

Invoke:

```text
tailscale file get --conflict=<skip|overwrite|rename> [--wait] <DIRECTORY>
```

Do not implement indefinite `--loop` in this phase. The command is a tracked,
cancellable task. On completion, show the filenames reported by the CLI when
that output is documented; otherwise show the destination and exit outcome.

The local CLI does not provide a stable preflight waiting-file inventory in
the selected phase contract. Do not promise one or invent it by inspecting
daemon storage. If a future documented structured surface appears, add it in
a later specification.

## 04.7 Taildrive shares

### Capability gate

Taildrive commands are alpha in the researched CLI. The Taildrive subsection
is disabled by default and shows that status. It becomes available only when
the capability probe confirms the required `drive` commands and the user
enables alpha local features in Settings for the current run. This opt-in is
not persisted in this phase.

### List and parser

Run `tailscale drive list`. If no structured output exists, parse the exact
installed-version human output behind `src/local/transfers.rs`. Commit fixtures
for empty, single-share, multiple-share, paths containing spaces, malformed,
and additive-column cases. A parser mismatch disables mutations and preserves
the raw redacted command error; it must not guess column boundaries.

### Mutations

Register:

- `services.drive.share` → `tailscale drive share <NAME> <PATH>`;
- `services.drive.rename` → `tailscale drive rename <OLD> <NEW>`;
- `services.drive.unshare` → `tailscale drive unshare <NAME>`.

Share paths must be existing directories. Normalize names once according to
the CLI's documented lowercase and character restrictions, show both the input
and normalized name before apply, and reject a result that is empty or already
exists. Do not auto-append a suffix.

Share and rename are Tier 1. Unshare is Tier 2 because existing clients lose
access. Refresh `drive list` and verify the named result after every write.
Explain access-control prerequisites but do not edit grants or policy.

## 04.8 Certificate acquisition

### Domain selection

Register `services.certificate.obtain`. Offer only certificate domains reported
by documented local node state. A user may type a domain, but dispatch remains
disabled unless it exactly matches an eligible local domain.

### Output contract

Collect explicit certificate and key output paths plus an optional minimum
validity. Both parent directories must exist and be writable. The paths must be
different and may not be `-`. Do not read, preview, copy, or persist the key
contents.

If either output exists, require Tier 2 confirmation and name the files that
will be overwritten. Otherwise the operation is Tier 1.

Invoke:

```text
tailscale cert --cert-file=<CERT_PATH> --key-file=<KEY_PATH> \
  [--min-validity=<DURATION>] <DOMAIN>
```

Do not use `--serve-demo`. After success, verify with filesystem metadata that
both files exist and are non-empty. Do not weaken permissions set by the CLI.
Task history records paths and domain but never certificate or key contents.

## 04.9 Metrics inspection

Register `services.metrics.refresh` and run:

```text
tailscale metrics print
```

Show the bounded Prometheus exposition text in a scrollable, copyable viewer
with capture time and source identity. Preserve line order. Do not build a
time-series database, infer trends from one sample, or persist metrics.

The viewer must distinguish empty output from command failure. Apply the task
output cap and state clearly when output was truncated. Secret-pattern
redaction still runs before display or copy.

## 04.10 Bug reports

Register `services.bugreport.create`. The form contains an optional plain-text
note and a `run diagnostics` boolean. Reject control characters other than
newline and tab; pass the note as one argument rather than shell text.

Invoke one of:

```text
tailscale bugreport [<NOTE>]
tailscale bugreport --diagnose [<NOTE>]
```

This is Tier 1 because it can collect and upload diagnostic information. The
confirmation explains that Tailscale receives a diagnostic report. Parse and
display the returned report identifier using fixture-backed output parsing.
Do not automatically copy, share, or submit that identifier elsewhere.

The recording mode is deferred. Tale must not start a long-running
`bugreport --record` process without a separately specified lifecycle and stop
contract.

## 04.11 Actions, refresh, and error behavior

### Required action IDs

The action registry must contain, at minimum:

```text
view.services
services.section.next
services.section.previous
services.serve.refresh
services.serve.create
services.serve.edit
services.serve.reset
services.funnel.refresh
services.funnel.create
services.funnel.edit
services.funnel.reset
services.taildrop.send
services.taildrop.receive
services.drive.refresh
services.drive.share
services.drive.rename
services.drive.unshare
services.certificate.obtain
services.metrics.refresh
services.bugreport.create
```

Bindings are contextual and must not collide with global navigation. Help,
footer hints, and the action palette are generated from the registry.

### Capability checks

Before dispatch, combine:

- local source availability;
- installed CLI capability;
- platform support;
- global read-only state;
- task conflict state;
- selected resource validity;
- alpha opt-in for Taildrive.

The reducer rechecks capability when handling the action. A stale visible
button cannot bypass a newly active read-only lock or missing source.

### Task conflicts

Allow reads for unrelated subsections concurrently. Serialize writes that can
change the same Serve, Funnel, Taildrop receive, Taildrive, or certificate
resource. A reset conflicts with every write in its subsection. Sending files
to distinct targets may run concurrently up to the global task limit.

### Error presentation

Map process failures into `NotInstalled`, `DaemonUnavailable`, `Unsupported`,
`PermissionDenied`, `PolicyDenied`, `TimedOut`, `Cancelled`, `DecodeFailed`, or
`CommandFailed`. Preserve bounded redacted stderr in task detail. Never show a
Rust debug representation to the user.

## 04.12 Verification specification

### Unit tests

Cover:

- listener/backend/path validation and command argument construction;
- Funnel exclusion of HTTP and mandatory public-risk metadata;
- Serve and Funnel DTO decoding, including unknown fields;
- Taildrop target and progress parsers for every supported output version;
- receive conflict argument construction;
- Taildrive name normalization and parser failure behavior;
- certificate path collisions, overwrite risk, and eligibility checks;
- bug-report identifier parsing and note argument boundaries;
- capability and read-only checks at both render and dispatch time;
- stale post-write refresh results.

No test may depend on a real daemon, network, tailnet, or user file.

### Adapter contract tests

Use a fake executable that records its argument vector and emits fixture
stdout/stderr. Prove for every operation that:

- no shell is launched;
- each path, note, backend, and target remains one argument;
- timeout and cancellation terminate the owned process;
- stdout and stderr stay separate;
- output caps and redaction are enforced;
- successful writes trigger the expected verification read;
- nonzero exit never mutates the domain snapshot.

### Reducer and UI tests

Test each subsection in loading, empty, ready, partial, stale, unsupported,
read-only, running, failed, and compact-layout states. Snapshot at 60x18,
80x24, 110x30, and 160x45. Include confirmation previews for public Funnel,
Serve reset, Taildrop overwrite, Taildrive unshare, and certificate overwrite.

### Required verification commands

Run the repository's established checks. At minimum:

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Also run a source scan that fails on production `unsafe`, `unwrap`, `expect`,
`panic!`, `todo!`, and `unimplemented!` uses. Test fixtures may contain those
words as data, but executable Rust code may not.

### Manual acceptance journeys

Using only fictional fixtures and the fake executable, demonstrate:

1. Inspect an HTTPS Serve mapping, create another, and observe verified state.
2. Preview a public Funnel mapping, cancel once, then apply and verify it.
3. Send two files whose paths contain spaces and cancel a separate transfer.
4. Receive with `rename`, then preview the stronger `overwrite` confirmation.
5. Enable alpha features for the run, share a directory, rename it, and
   confirm unshare.
6. Obtain a certificate to new files without exposing key contents.
7. Inspect truncated metrics with an explicit truncation notice.
8. Create a diagnostic bug report and display only its identifier.
9. Start with an older fake CLI and see unsupported reasons instead of failed
   or hidden actions.

Repeat the applicable read-only journeys with `--read-only`; no process that
mutates state may be spawned.

## 04.13 Exit gate

Phase 4 is complete only when:

- all supported local service sections work end to end through the direct
  process adapter;
- public, destructive, overwrite, alpha, and diagnostic risks are explicit;
- every write has a confirmation appropriate to its tier and a verification
  read where the CLI exposes one;
- no stable waiting-file inventory or service-control API has been invented;
- no admin-plane resource is presented as local daemon state;
- terminal restoration, cancellation, redaction, and task history remain
  correct under failures;
- the full verification specification passes.

At this gate Tale is a complete local-node product. Phase 5 may add the admin
observer, but it must enrich the application through a separate source rather
than replacing or conflating these local workflows.
