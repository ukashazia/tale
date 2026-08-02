# Specification 01 — TUI foundation

- Implementation phase: 1
- JJ change description: `feat: build Tale TUI foundation`
- Depends on: Phase 0 documents
- Produces: a complete offline/mock Tale with no Tailscale process or network
  use

This specification is normative for Phase 1. An implementation agent must not
add local CLI integration, API authentication, future routes, or placeholder
controls while implementing it.

## 01.0 Phase contract

### User-visible result

Running `tale --mock` opens a responsive keyboard-driven TUI containing:

- a header showing `mock` as the active source;
- Overview, Devices, Activity, and Settings routes;
- a fictional device collection and inspector;
- route command palette, filter input, sorting, contextual help, action picker,
  copy picker, notifications, and task history;
- deterministic mock refresh, success, failure, cancellation, and stale-data
  behavior;
- clean terminal restoration on every exit path.

Running `tale` without `--mock` in this phase opens the shell with a source card
that says local integration is unavailable in this build. It must not invoke
`tailscale`. This is a truthful phase limitation, not a “coming soon” menu.

### Implementation rules

- Before editing, start the JJ change named above. Do not modify history or use
  Git.
- Add only modules used by this phase.
- Do not use `unsafe`, `panic!`, `unwrap`, or `expect`.
- Do not add a generic component framework, dependency-injection container,
  repository abstraction, plugin system, theme system, or key-remapping system.
- All terminal and task resources must have explicit ownership and cleanup.
- Rendering functions borrow state and perform no I/O.
- Tests use fictional names, addresses, IDs, and timestamps.

### Expected dependency capabilities

Inspect current documentation and enable only the features required by the
implementation. The phase is expected to need maintained crates providing:

- Ratatui rendering;
- Crossterm terminal/input handling;
- Tokio runtime, channels, timers, and signals;
- typed CLI argument parsing;
- Serde and TOML configuration parsing;
- structured error derivation;
- deterministic terminal-buffer snapshot testing.

Dependency names and versions are chosen during implementation after checking
their current maintenance and types. Do not add process, HTTP, keyring, OAuth,
clipboard, URL-opening, or filesystem-watcher dependencies in this phase.

## 01.1 Command-line entry

### Required files

- `src/main.rs`
- `src/cli.rs`
- `src/error.rs`
- `src/lib.rs` only if integration tests need a library boundary
- `tests/cli.rs`

### Command contract

The binary accepts:

```text
tale [--profile NAME] [--config PATH] [--view ROUTE] [--read-only]
     [--no-local] [--tailscale-path PATH] [--mock]

tale config path [--config PATH]
tale config check [--config PATH]
tale doctor [--config PATH] [--mock]
```

Phase 1 parses `--profile` because it belongs to the configuration contract,
but returns a clear error that admin profiles are not supported in this phase.
It must not render a profile picker.

`--mock` has these exact semantics:

- it prevents local process execution, HTTP, keyring, and real filesystem
  mutation other than explicitly requested config inspection;
- it selects deterministic fictional providers;
- it is incompatible with `--profile` and `TALE_ACCESS_TOKEN`;
- it is not persisted into configuration;
- the header and diagnostic output visibly say `mock`.

`--tailscale-path` and `--no-local` are accepted and represented in resolved
configuration, but have no process behavior until Phase 2.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | normal TUI exit or successful non-interactive command |
| 2 | invalid CLI arguments or invalid configuration |
| 1 | runtime initialization or non-interactive command failure |

Terminal rendering errors and top-level application errors go to stderr only
after the terminal has been restored.

### Tests

- Every documented flag and subcommand parses.
- Unknown flags, invalid route names, and incompatible `--mock --profile` fail
  with exit code 2.
- `config path` does not create a file or directory.
- Help output contains no routes that are unavailable in Phase 1.
- CLI tests invoke the compiled binary without entering the alternate screen.

### Acceptance criteria

- `main` performs argument parsing, runtime setup, application dispatch, and
  final error printing only.
- Business behavior is not implemented in `main.rs`.
- No path uses `std::process::exit` before terminal cleanup; non-TUI command
  dispatch may return an exit code to `main`.

## 01.2 Terminal session lifecycle

### Required files

- `src/terminal.rs`
- `src/event.rs`
- `tests/terminal_restore.rs`

### Terminal ownership

Create one `TerminalSession` that owns whether Tale has:

- enabled raw mode;
- entered the alternate screen;
- enabled bracketed paste;
- enabled mouse capture;
- hidden the cursor.

Construction performs setup in a defined order. Destruction restores only the
states successfully acquired, in reverse order. Cleanup returns/records errors
without panicking. Calling cleanup more than once is harmless.

Mouse capture is disabled by default and is not configurable to `true` until
Phase 8; Phase 1 may parse the existing setting but must report it as
unsupported when enabled.

### Input contract

Emit typed events for:

- key press, excluding release/repeat events when the terminal reports them;
- resize;
- paste;
- focus gained/lost when supported;
- timer tick;
- shutdown signal.

Pasted text is delivered only to the focused text editor. It never dispatches
bindings one character at a time.

### Signal and error behavior

- `Ctrl+c` is an application key event while raw mode is active.
- Supported operating-system termination signals request graceful shutdown.
- If rendering or the event source fails, stop task writers, restore the
  terminal, and return the error.
- Terminal restoration is tested through a PTY or an injectable terminal
  backend; checking a boolean in a fake object alone is insufficient.

### Tests

- Setup failure after each acquired terminal state restores prior states.
- Normal quit restores raw mode, cursor, paste, alternate screen, and mouse
  capture state.
- Render failure and event-source failure restore the terminal.
- Resize emits one event with the latest dimensions.
- Paste cannot trigger `q`, `:`, or another global action.

### Acceptance criteria

- No other module directly enables/disables raw mode or the alternate screen.
- The terminal is usable after normal exit and every injected failure.
- Rendering never writes to stdout outside the terminal backend.

## 01.3 Event, update, and effect runtime

### Required files

- `src/app.rs`
- `src/event.rs`
- `src/effect.rs`
- `src/runtime.rs`
- `tests/app_reducer.rs`
- `tests/runtime.rs`

### State mutation contract

Use one reducer boundary:

```text
App::update(&mut self, event: Event) -> Vec<Effect>
```

The exact Rust syntax may differ, but the semantics may not:

- `update` is synchronous and performs no I/O;
- background work is described by typed effects;
- effect results return as typed events;
- only the reducer mutates `App` UI state;
- rendering receives `&App` and cannot dispatch I/O.

Required event families:

```text
Event
  Input(InputEvent)
  Tick(Instant)
  Task(TaskEvent)
  Source(SourceEvent)
  ShutdownRequested(ShutdownReason)
```

Required effect families for Phase 1:

```text
Effect
  StartMockLoad { resource, generation, scenario }
  StartMockTask { task_id, behavior }
  CancelTask { task_id }
  WriteConfigCandidate { path, bytes }   # only if a Phase-1 setting is saved
  RequestShutdown
```

Do not use boxed closures as application effects.

### Queues and redraw

- The central event queue is bounded to 256 entries.
- Task progress may be coalesced; completion/error/cancellation events may not
  be dropped.
- A full queue applies backpressure to producers except high-frequency cosmetic
  ticks, which can be coalesced.
- Render after a state-changing event, resize, or active-spinner tick; do not
  redraw continuously while idle.
- Maintain one monotonically increasing generation per resource collection.
  A result whose generation is not current is discarded without changing data
  or error metadata.

### Shutdown

Shutdown proceeds in this order:

1. stop accepting user actions;
2. request cancellation of active effects;
3. wait for bounded graceful completion;
4. abort remaining owned tasks;
5. restore terminal;
6. return final application result.

The grace duration is an internal constant of one second in Phase 1. It is not
user configuration.

### Tests

- Each event produces the expected state and effects.
- An older generation cannot replace a newer snapshot.
- Queue saturation does not drop task completion.
- Idle application does not render on every timer interval.
- Shutdown cancels mock tasks and terminates within the grace bound.

### Acceptance criteria

- No task retains `&mut App` or a widget reference.
- There is one event receiver and one reducer mutation path.
- Every spawned task is registered and owned by the runtime.

## 01.4 Application state, routes, and overlays

### Required files

- `src/app.rs`
- `src/domain/mod.rs`
- `src/domain/device.rs`
- `src/ui/mod.rs`
- `src/ui/layout.rs`
- `src/ui/views/overview.rs`
- `src/ui/views/devices.rs`
- `src/ui/views/activity.rs`
- `src/ui/views/settings.rs`
- `src/ui/components/overlay.rs`

Create directories/modules only when the first concrete type in this section is
implemented.

### Required state model

The state must represent these concepts explicitly:

```text
App
  route_stack
  focus
  overlays
  views
  devices_resource
  tasks
  notifications
  resolved_config
  shutdown_state

Route
  Overview
  Devices
  Activity
  Settings

Overlay
  CommandPalette
  FilterEditor
  Help
  ActionPicker
  CopyPicker
  QuitConfirmation
  TaskInspector
```

Each collection state owns selected domain ID, scroll position, filter draft,
applied filter, sort field/direction, and standard/wide column mode. A row index
is derived for rendering and never persisted as identity.

### Back and quit behavior

Resolve `Esc`, `q`, and `Ctrl+c` in this order:

1. text editing/candidate selection;
2. top overlay;
3. inspector focus/detail route;
4. route stack;
5. active focused task cancellation for `Ctrl+c`;
6. root quit.

At root:

- `q` quits immediately when no task is active;
- `q` opens a confirmation when tasks are active;
- the first `Ctrl+c` cancels the focused cancellable task or input;
- `Ctrl+c` while idle requests quit;
- no double-key timing heuristic is used.

### Acceptance criteria

- Overlay state is a stack; opening help over an action picker restores the
  picker when help closes.
- A refresh/re-sort preserves selection by `DeviceId` when present, selects the
  nearest visible row when removed, and selects nothing for an empty list.
- Routes unavailable in Phase 1 are absent from palette, help, and bindings.

## 01.5 Responsive frame and visual contract

### Required files

- `src/ui/layout.rs`
- `src/ui/theme.rs`
- `src/ui/text.rs`
- `src/ui/components/header.rs`
- `src/ui/components/table.rs`
- `src/ui/components/inspector.rs`
- `src/ui/components/footer.rs`
- `tests/ui_snapshots.rs`

### Breakpoints

| Terminal width | Layout |
| --- | --- |
| 110+ | collection and 34–45% inspector |
| 80–109 | collection full-screen; `Enter` opens full-screen detail |
| 60–79 | compact columns and full-screen detail |
| below 60 or height below 18 | minimum-size message only |

The minimum-size screen still handles resize and quit.

### Frame regions

From top to bottom:

1. one-line context header;
2. one-line route title/filter/sort/count row;
3. content region;
4. optional one-line notification/progress region;
5. one-line contextual footer.

The content region is the only flexible-height region. Tables never wrap rows.
Truncated values are fully present in the inspector or copy picker.

### Visual semantics

- Use the terminal default background.
- Focus uses border/title emphasis.
- Selection uses reverse video or bold plus a cursor marker.
- Healthy/success, attention/stale, error/destructive, and informational states
  each have text/symbol fallbacks.
- ASCII mode is the reference snapshot mode.
- Unicode mode may use only known-width symbols and must preserve alignment.
- No Nerd Font glyphs, rounded-box novelty, gradients, or animated transitions.

### Tests

Snapshot at minimum:

- 60x18 ASCII/no-color;
- 80x24 ASCII/no-color;
- 110x30 Unicode/ANSI256;
- 160x45 Unicode/TrueColor;
- empty list, populated list, stale source, error source, overlay, long text,
  and minimum-size screen.

### Acceptance criteria

- Every field/action reachable at 110 columns remains reachable through detail
  or an overlay at 80 columns.
- No snapshot contains broken borders, clipped overlay controls, or wrapped
  table rows.
- Color-disabled snapshots still distinguish every state.

## 01.6 Command palette, filtering, and sorting

### Required files

- `src/action.rs`
- `src/ui/components/command_palette.rs`
- `src/ui/components/filter.rs`
- `src/domain/filter.rs`
- `tests/filter.rs`

### Command palette

Phase-1 routes and aliases:

```text
overview: ov, home
devices: device, dev, nodes
activity: tasks
settings: config
```

Palette completion is case-insensitive; canonical route display is lowercase.
`Enter` accepts an exact selection. An unknown command remains editable with an
inline error. It never executes a shell command or silently becomes a filter.

### Filter grammar

Phase 1 implements the parser shape and fictional fields:

```text
free text
field:value
!field:value
field:value1,value2
field:<duration
"quoted value"
```

Separate terms are ANDed. Comma values are ORed. Negation applies to the entire
field term. Invalid structured terms remain in the editor and do not alter the
applied filter.

Fictional Device fields: `online`, `owner`, `os`, `path`, `tag`, `lastSeen`.
Free text searches display name, hostname, owner label, tags, and addresses.

### Sorting

- Sorting is stable.
- `DeviceId` is the final tie-breaker.
- Missing optional values sort after present values in ascending order and
  before them in descending order.
- Changing sort does not change selected identity.

### Tests

- Parser table tests cover quoting, negation, OR, comparisons, whitespace,
  invalid fields, invalid durations, and incomplete input.
- AND/OR behavior matches the UX contract.
- Sorting is deterministic with equal and missing values.
- Filtering 5,000 fictional devices produces the expected selection mapping.

## 01.7 Action registry and contextual help

### Required files

- `src/action.rs`
- `src/ui/components/action_picker.rs`
- `src/ui/components/help.rs`
- `src/ui/components/footer.rs`
- `tests/actions.rs`

### Action definition

Every executable action has:

```text
ActionSpec
  id                 stable dotted identifier
  label
  description
  contexts
  selection_rule     none, one, many
  default_bindings
  capability
  risk               observe, reversible, disruptive, destructive_or_secret
```

Phase-1 action IDs include at least:

```text
app.quit
view.command_palette
view.filter
view.refresh
view.refresh_all
view.help
view.tasks
collection.move_up
collection.move_down
collection.first
collection.last
collection.page_up
collection.page_down
collection.open
collection.sort
collection.wide_columns
resource.actions
resource.copy
task.cancel
```

### Single-source rule

Key dispatch, footer hints, full help, and action picker all resolve through the
registry. A binding must not call implementation code directly outside action
dispatch.

Disabled actions appear in full help/action picker with a reason. They do not
appear in the compact footer unless needed to explain a blocked primary flow.

### Tests

- Every binding references a registered action.
- No duplicate active binding exists in the same context.
- Footer and help labels match the action registry.
- Disabled actions cannot dispatch.
- Footer width calculation ends with `? more` rather than truncating a key.

## 01.8 Task history and notifications

### Required files

- `src/task.rs`
- `src/ui/views/activity.rs`
- `src/ui/components/task_view.rs`
- `src/ui/components/notification.rs`
- `tests/tasks.rs`

### Task model

```text
Task
  id
  action_id
  target_label
  state
  started_at
  finished_at
  progress
  summary
  detail
  cancellable

TaskState
  Queued
  Running
  Cancelling
  Succeeded
  Failed
  Cancelled
```

Task output/detail is capped at 256 KiB per task in Phase 1. When capped, retain
the beginning and end with a visible truncation marker. The configured
`history.max_tasks` bounds completed in-memory tasks; active tasks are never
evicted.

Notifications are short-lived references to a result. They do not contain the
only copy of an error. Failures remain inspectable in Activity.

### Mock tasks

Provide deterministic actions to exercise:

- delayed success with progress;
- delayed failure with bounded detail;
- cancellable long task;
- non-cancellable task;
- stale source refresh result.

These are available only in `--mock` mode and are labeled as simulations.

### Tests

- Valid state transitions only; terminal states cannot restart.
- Cancellation is idempotent.
- Active tasks survive history eviction.
- Output cap and truncation marker are deterministic.
- Notifications expire without deleting task results.

## 01.9 Mock domain and source

### Required files

- `src/mock.rs`
- `src/domain/device.rs`
- `tests/fixtures/mock_devices.toml` or an equivalent fictional static fixture
- `tests/mock_source.rs`

### Fixture contract

Use at least 12 devices covering:

- online/offline/unknown liveness;
- direct/DERP/peer-relay/no-path;
- Linux, macOS, Windows, iOS, Android, and an unknown OS string;
- user-owned and tagged devices;
- IPv4/IPv6 and missing optional addresses;
- exit-node option, subnet router, SSH, Funnel, shared, expired, and approval
  properties as fictional display data;
- long names and Unicode user-provided labels.

All IPs use documentation ranges or Tailscale examples. Emails use
`example.com`. No fixture is copied from a real tailnet.

Mock time is injected. Snapshot tests must not depend on wall-clock time.

### Acceptance criteria

- `--mock` performs no process spawn, network connection, keyring call, or
  secret lookup; tests assert the real adapters are not constructed.
- Repeated runs with the same scenario produce the same device order, IDs,
  timestamps, and task results.

## 01.10 Configuration foundation

### Required files

- `src/config.rs`
- `src/paths.rs`
- `tests/config.rs`

### Fields implemented

Implement the documented root, `[local]`, `[admin]`, `[ui]`, and `[history]`
fields. Profile blocks may parse into a preserved typed structure, but profile
activation is rejected until Phase 5.

Precedence is exactly:

1. CLI flags;
2. documented environment variables;
3. TOML file;
4. built-in defaults.

`--mock` is CLI-only and outranks all source configuration.

### Path behavior

Follow `docs/configuration.md` exactly. Path lookup must not create directories.
Use lexical absolute paths for display where canonicalization would require the
target to exist.

### Validation

- Unknown fields are errors with their full dotted path.
- Invalid enum/duration/range values name the field and allowed values.
- `default_profile` referencing a missing profile is an error even though
  profiles are not active in Phase 1.
- `NO_COLOR` forces `ui.color = "none"`.
- Literal credential/token-like fields are unknown and therefore rejected.

### Settings view

Display resolved paths, source mode, read-only state, UI modes, refresh values,
and whether each value came from CLI, environment, file, or default. Phase 1
Settings is read-only.

### Tests

- Default paths on Unix/macOS and Windows path logic through injected platform
  inputs.
- Full precedence matrix.
- Unknown fields and every range boundary.
- Missing config uses defaults and performs no write.
- `config check` contains no secret/environment values in errors.

## 01.11 Phase verification and handoff

Run, at minimum:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Also verify manually or through PTY automation:

1. `tale --mock` at 80x24.
2. Navigate all four routes through `:`.
3. Filter and sort Devices without losing selection.
4. Open stacked action/help overlays and return correctly.
5. Start, inspect, cancel, and complete mock tasks.
6. Resize across every breakpoint.
7. Quit with and without an active task.
8. Confirm the terminal remains usable.
9. Run `tale config path`, `config check`, and `doctor --mock`.

The implementation handoff must report:

- exact files added or modified;
- dependency decisions and enabled features;
- test commands and results;
- any platform behavior not actually exercised;
- confirmation that no Tailscale process/API/keyring access exists in Phase 1.
