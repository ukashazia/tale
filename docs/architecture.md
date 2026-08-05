# Application architecture

## Architectural decision

Tale is one Rust binary with a LocalAPI observer, a Local CLI adapter, an
admin adapter, and one domain/UI core:

```text
                         ┌─────────────────────────────┐
terminal input ────────▶ │ App state + update reducer  │ ────────▶ Ratatui view
                         └──────────────┬──────────────┘
                                        │ typed effects
                    ┌───────────────────┴───────────────────┐
                    ▼                                       ▼
          ┌──────────────────┐                    ┌──────────────────┐
          │ LocalAPI observer│                    │ Control API client│
          │ local daemon     │                    │ tailnet resources │
          └────────┬─────────┘                    └────────┬─────────┘
                   │                                       ▼
             UDS / named pipe                         HTTPS API

          ┌──────────────────┐
          │ Local CLI adapter│
          │ typed operations │
          └────────┬─────────┘
                   ▼
             tailscale process
```

There is no generic backend/repository framework. The local daemon, local CLI,
and admin sources have different semantics and return explicit source models.
Composition happens in small domain functions where a view genuinely needs
both. A working daemon endpoint is enough for read-only local observation; CLI
discovery is an independent capability for mutations and process-backed work.

## Runtime model

Use a Model–Update–View event loop:

1. Input, timers, task progress, and adapter results become typed `Event`s.
2. `App::update` synchronously mutates application state and returns zero or
   more typed `Effect`s.
3. An effect runner performs I/O on Tokio tasks and emits result events.
4. `ui::render` is a pure read of state.

Only the reducer mutates UI state. Background tasks never hold mutable references
to widgets or selections.

Required properties:

- bounded channels and bounded output buffers;
- cancellation tokens for refreshes, streams, and child operations;
- monotonically increasing request generations per resource;
- stale results are discarded when a newer generation exists;
- one active mutation per resource identity;
- terminal rendering never waits on network or process I/O;
- terminal cleanup is owned by an RAII session guard and also runs on ordinary
  errors and signals.

## Planned module boundaries

```text
src/
  main.rs                 process entry, error reporting
  cli.rs                  Tale command-line arguments
  config.rs               paths, parsing, validation, precedence
  app.rs                  App state and update reducer
  event.rs                input/tick/task event types
  effect.rs               effect types and dispatcher
  task.rs                 lifecycle, cancellation, progress, redaction
  action.rs               action registry, capability and risk metadata
  terminal.rs             setup, restore, resize, child handoff
  domain/
    mod.rs
    capability.rs
    device.rs
    user.rs
    route.rs
    dns.rs
    policy.rs
    service.rs
    credential.rs
    activity.rs
    health.rs
  local/
    mod.rs
    daemon.rs              LocalAPI HTTP/1 snapshots and watch transport
    ipn.rs                 bounded watch framing and invalidation scheduling
    client.rs              typed tailscale argv construction
    dto.rs                 versioned CLI output boundary
    process.rs            bounded non-shell runner
  admin/
    mod.rs
    client.rs             HTTP, pagination, response metadata
    auth.rs               keyring and OAuth token lifecycle
    dto.rs                API wire types
  ui/
    mod.rs
    layout.rs
    theme.rs
    text.rs
    components/
      table.rs
      inspector.rs
      interaction_shell.rs
      form.rs
      task_view.rs
      confirm.rs
    views/
      overview.rs
      local.rs
      devices.rs
      users.rs
      routes.rs
      dns.rs
      access.rs
      services.rs
      credentials.rs
      activity.rs
      settings.rs
```

A file is added only when its phase introduces the concern. The tree is a target
boundary, not permission to generate empty modules.

## Core state

`App` owns:

- a bounded 100-frame browser-style `ViewHistory` with a cursor;
- one explicit bottom-shell `InteractionMode`, pane focus, and modal/dedicated-view state;
- per-view selection, filter, sort, scroll, and form state;
- current `LocalSnapshot` and per-profile `AdminSnapshot` values;
- freshness and error metadata for each resource collection;
- capability results;
- active and recent tasks;
- short-lived notifications.

Snapshots are replaced atomically after successful decoding. A failed refresh
updates resource metadata but preserves the last successful snapshot. Selection
is tracked by opaque domain ID, never row index, so sorting and refreshes do not
silently select another resource.

Each history frame stores only presentation intent: route, focus, stable resource
identity, scroll anchor, filter, sort, Services section, and saved-view identity.
It never stores snapshots, credentials, forms, tasks, adapter handles, or secret
results. Back and forward restoration re-evaluate the frame against the current
snapshot and select deterministically when a stored identity disappeared.

View state is ephemeral unless a specific setting is documented as persistent.
The initial release persists the last route and profile only; filters become
persistent only through an explicit saved-view feature.

## Local adapter

### Process execution

The adapter constructs `std::process::Command`/Tokio command arguments from
typed action values. It must never invoke `sh -c`, `bash -c`, PowerShell command
strings, or interpolate user input into a shell program.

Every invocation defines:

- executable path;
- explicit argv;
- whether stdin is closed, piped, or handed to an interactive child;
- timeout or streaming cancellation behavior;
- maximum stdout/stderr bytes retained;
- parser and expected exit codes;
- redaction metadata for secret arguments;
- whether it requires alternate-screen suspension.

Local mutations are followed by a read operation. UI state changes only when the
read confirms the new daemon state. A spinner may indicate the requested state,
but the underlying value remains the last verified value.

### Output compatibility

Structured CLI output is decoded through DTOs that:

- deserialize only required fields as required;
- keep optional fields optional;
- permit unknown fields;
- convert into Tale domain types in one module;
- include fixtures labeled with platform and exact Tailscale version.

There are no legacy output fallbacks. If a supported command's contract changes,
the adapter returns `UnsupportedOutput` with the client version and failing
field. Support is added deliberately with a fixture and test.

Human output is not parsed when a JSON or JSON-line mode exists. Commands with
only human output use a purpose-built parser with representative fixtures; Tale
does not scrape terminal colors or column positions.

### Interactive handoff

SSH, `nc`, login, file choice through an external tool, and editor workflows can
own the terminal. The handoff sequence is:

1. cancel or pause input capture;
2. leave raw mode and alternate screen;
3. launch the child directly and forward relevant signals;
4. wait for completion without other tasks writing to the terminal;
5. restore terminal mode, force a full redraw, and refresh affected resources.

No action is automatically re-run after a handoff failure.

## Admin client

The Control API client owns URL construction, authentication, timeouts,
pagination, error decoding, and response metadata. DTOs do not leak into the UI.

Rules:

- use HTTPS and the documented API base URL;
- send bearer tokens only in the Authorization header;
- never include tokens in URLs, errors, tracing fields, or debug formatting;
- use endpoint-specific methods instead of a generic JSON request exposed to
  the rest of the application;
- respect server retry metadata and apply capped exponential backoff only to
  idempotent reads;
- do not automatically retry mutations unless the endpoint provides an
  idempotency contract;
- cancel superseded reads;
- capture the server request ID when exposed;
- treat authentication, authorization, plan restriction, validation, conflict,
  rate limit, and transient failure as separate error classes.

The client does not claim a capability simply because a menu item exists. A
profile begins with configured intent, then records observed endpoint outcomes.
An endpoint-specific `403` disables only the related operation.

## Authentication and secret handling

Supported credential forms are:

1. A scoped OAuth client stored in the OS credential store. Tale exchanges it
   for a one-hour token and refreshes shortly before expiry.
2. An API access token from an environment variable or the OS credential store.
   This is supported for evaluation and cases where OAuth is unsuitable.

The configuration file stores only a credential reference. Secret-bearing
types must not implement `Debug` or `Display`; use secrecy/zeroizing containers
after evaluating the maintained crate APIs. Memory cleanup is best-effort and is
not described as protection from a compromised process.

Keyring writes are transactional with profile creation: validate first, write
the credential, then write configuration. On failure, remove the newly written
credential. Removing a Tale profile does not revoke a Tailscale credential;
those are separate, clearly named actions.

## Actions and capabilities

The action registry is the single source for:

- stable action ID, label, and description;
- contexts and selection cardinality;
- default bindings;
- required local command/API endpoint/scopes;
- risk tier;
- input schema;
- preview builder;
- effect constructor.

The footer, bottom help sheet, direct `a`/`y` transient menus, mouse hit regions,
and key dispatch all read this registry. Stable transient sequences are
validated for duplicate leaves, leaf/prefix conflicts, depth, and reserved
global keys. This
prevents help from drifting from executable behavior. Custom keybindings and
plugins are not part of the first release; stable action IDs make later
remapping possible without redesigning execution.

Capability evaluation is pure over current platform, client version, source
health, profile mode, observed authorization, selected resource, and active
tasks. Disabled actions remain discoverable with a reason.

## Policy workflow

Policy source is an opaque HuJSON document until a maintained parser is chosen.
Tale preserves the remote bytes for edit and diff workflows.

The API is authoritative for validation, policy tests, and permission preview.
The save workflow requires:

- the base remote document hash;
- a candidate document;
- successful current validation;
- a final remote fetch equal to the base, unless the API later provides and
  Tale adopts a documented concurrency token;
- a displayed diff and explicit confirmation.

If the remote changed, Tale blocks save and retains both candidate and new remote
content in temporary files for manual reconciliation. It does not auto-merge or
normalize policy text.

## Derived health analysis

Health findings are pure, deterministic functions over snapshots. Every finding
contains:

- stable finding ID and severity;
- observed facts and their timestamps;
- affected resource IDs;
- explanation;
- zero or more typed suggested actions.

Initial findings may cover expired/soon-expiring keys, pending approvals, client
version skew, overlapping advertised CIDRs, and failed refreshes. Offline age is
informational unless the user configures an expectation later. Findings never
claim policy reachability or packet-path truth without an authoritative result.

## Error presentation

Errors are structured:

```text
ErrorKind
  LocalBinaryMissing
  LocalDaemonUnavailable
  LocalPermissionDenied
  UnsupportedClientVersion
  UnsupportedOutput
  AuthenticationRequired
  AuthorizationDenied
  PlanRestricted
  ValidationFailed
  RemoteChanged
  RateLimited
  TimedOut
  Cancelled
  Transport
  Internal
```

Each error carries a safe summary, optional redacted technical detail, source,
operation, retryability, and suggested next action. Raw HTTP bodies and stderr
are bounded and redacted before storage or rendering.

## Rust and dependency policy

Mandatory implementation rules:

- no `unsafe` code;
- no `panic!`, `unwrap`, or `expect` in application or test support code;
- use explicit error propagation and exhaustive state handling;
- avoid cloning snapshots or secret material; use ownership, borrowing, and
  shared immutable data only where measured needs justify it;
- add dependencies only after checking existing dependencies, current docs,
  maintenance, license, and whether they reduce total complexity.

Likely foundation crates are Ratatui, Crossterm, Tokio, Reqwest, Serde, a TOML
parser, tracing, URL/IP types, and a cross-platform keyring library. This is not
a locked dependency list. Each is selected in the phase that first needs it.

## Verification strategy

### Unit tests

- reducers: event → state/effect transitions;
- filters, stable sorting, selection preservation, and responsive column choice;
- capability and risk evaluation;
- DTO-to-domain conversion for every captured version fixture;
- redaction and bounded-output behavior;
- derived health findings;
- config validation and precedence.

### Adapter contract tests

- a fake executable records argv and emits fixtures without a shell;
- local actions assert exact argv, timeouts, exit interpretation, and follow-up
  reads;
- an HTTP test server verifies methods, paths, headers, pagination, errors, and
  that secrets never appear in recorded URLs/logs;
- policy workflows cover validation failure and remote-change races.

### UI tests

- deterministic snapshots at 60x18, 80x24, 110x30, and 160x45;
- ASCII/no-color and Unicode/256/TrueColor modes;
- keyboard-only completion of every core flow;
- rendered-cell assertions that command, filter, transient, completion, and
  help surfaces are bottom anchored within each viewport;
- centered modal assertions limited to alerts and confirmations;
- selection stability during refresh and sorting.

### Integration tests

- a mock mode with fictional fixtures and no access to the real CLI/network;
- opt-in tests against a local Tailscale installation that perform reads only by
  default;
- mutating integration tests require an explicit disposable tailnet/profile and
  are never part of the default test command;
- PTY tests verify terminal restoration after success, error, cancellation, and
  child signal.

The project uses `cargo fmt --all --check`, strict Clippy, unit/contract tests,
and documentation link/format checks as the baseline. Exact commands are added
with the first implementation slice.
