# Specification 12 — Interaction shell and stack navigation

- Implementation phase: 12
- JJ change description: `feat: redesign interaction shell and navigation`
- Depends on: Specification 11 complete
- Produces: a bottom-anchored, keyboard-first interaction shell inspired by
  jjui and k9s without copying their source

This phase replaces Tale's centered command, filter, action, and copy pickers
with a coherent interaction grammar. Commands and filters are inline at the
bottom, action and copy prefixes are transient key menus, contextual help is a
which-key-style bottom sheet, and route history has explicit backward and
forward movement.

The redesign changes interaction structure, not product-domain behavior. All
existing actions, forms, risk confirmations, tasks, source states, redaction,
and mutation verification contracts remain in force.

## 12.0 Phase contract

### User-visible result

- `:` opens a one-line command prompt in the footer.
- `/` opens a one-line filter prompt in the footer.
- `Tab` completes commands or route-valid filter terms.
- `a` shows contextual action keys at the bottom; the next key invokes one.
- `y` shows contextual copy keys at the bottom; the next key copies one field.
- `?` opens contextual which-key help anchored to the bottom.
- `[` moves backward through view history and `]` moves forward.
- `q` means quit, subject only to the existing active-task confirmation.
- centered modals remain only for alerts and confirmations.

### Design principles

1. Invocation stays close to the footer where the user learned the key.
2. A known mnemonic is one keystroke after its prefix.
3. Navigation history and overlay dismissal are different operations.
4. One action registry supplies bindings, availability, footer hints, transient
   menus, help, and collision validation.
5. State restoration uses stable domain identities, never row positions.
6. Mouse behavior exposes the same operations but does not define them.
7. Inspiration is behavioral. Do not copy jjui, k9s, which-key.nvim, or other
   project source without an explicit license and provenance review.

### Explicitly out of scope

- configurable key remapping;
- plugins, macros, shell commands, arbitrary command execution, or fuzzy file
  finding;
- changing domain action risk levels or confirmation phrases;
- turning forms, editors, task views, or secret results into transient menus;
- adding a permanent sidebar;
- copying another project's implementation or visual assets;
- preserving the old centered pickers as fallback paths.

## 12.1 Interaction state machine

Replace generic picker overlays with explicit mutually exclusive shell modes:

```rust
enum InteractionMode {
    Normal,
    CommandLine(CommandLineState),
    FilterLine(FilterLineState),
    Transient(TransientMenuState),
    HelpSheet(HelpSheetState),
    Form(FormState),
    Confirmation(ConfirmationState),
    Alert(AlertState),
}
```

Exact type placement may follow current application conventions. The behavioral
contract is mandatory:

- exactly one top-level interaction mode receives ordinary key input;
- modal confirmation/alert state blocks the underlying view;
- command, filter, transient, and help state do not enter the centered overlay
  renderer;
- forms remain typed workflows and may be full-view, inspector, or bottom-sheet
  layouts according to available space;
- secret results remain dedicated ephemeral views;
- task and activity routes remain normal routes;
- resize cannot strand an invisible mode or remove its escape path.

The reducer owns mode changes. Rendering is a pure read. Completion generation,
large filtering, and other work that exceeds the input budget uses typed effects
and generation IDs; no widget mutates application state.

### Key precedence

Process a key in this order:

1. terminal handoff/child ownership;
2. active alert or confirmation;
3. active form or dedicated editor;
4. command/filter line editor;
5. transient menu;
6. help sheet;
7. normal contextual action registry;
8. unbound-key no-op.

Global quit does not bypass a secret result, dirty editor, confirmation, or
active-task safety contract. `Ctrl+c` retains the documented cancellation and
second-press exit behavior.

## 12.2 Bottom interaction region

Reserve a bottom region below the route content and above the terminal edge:

```text
normal:
│ a actions  y copy  / filter  : go  r refresh  ? help            │

command with completions:
│ devices      Devices inventory                                  │
│ dev          Alias for devices                                  │
│ : devi█                                                           │

filter with error:
│ owner:        Owner name or ID                                   │
│ / owner:"alice online:true        unclosed quote at column 7     │

transient:
│ Actions  r refresh  p ping  e exit node  s services  Esc cancel  │
```

Rules:

- normal mode uses one footer row;
- command/filter mode replaces that row with its prompt;
- transient mode replaces that row with its key menu;
- completion/help rows grow upward and never move the prompt from the final row;
- completion trays show at most six candidates, plus an overflow count when
  necessary;
- bottom content never covers an alert or confirmation;
- at 60x18, prompt editing remains functional even when candidates are hidden;
- no bottom interaction surface is rendered through `Clear` over a centered
  rectangle;
- mouse hit regions are derived from the final rendered spans.

Long labels are elided as whole items. Never show a key without enough of its
label to identify the action. When all items cannot fit, retain `? help` and an
overflow indicator.

## 12.3 Command-line contract

### Grammar

The accepted grammar is:

```text
:<route-or-alias> [route filter]
:view:<saved-view-name>
```

The leading colon is UI chrome and is not stored in the command buffer. Route
names and aliases remain those documented in `docs/ux.md`. This is not a shell:
quotes, separators, substitutions, pipes, redirects, and executable names have
no special execution meaning.

Execution rules:

- an exact route or alias navigates to that route;
- trailing text is parsed by that destination route's filter grammar before
  navigation commits;
- a saved-view name resolves through the existing saved-view domain contract;
- an unknown or ambiguous target leaves the line open with an inline error;
- a route that does not accept a general filter rejects trailing text;
- empty `Enter` closes without navigation;
- successful execution creates one view-history entry;
- navigation to an equivalent current frame is a semantic no-op.

Do not add arbitrary action commands in this phase. Existing action invocation
continues through keys, forms, and the action registry.

### Editing

Support:

| Key | Behavior |
| --- | --- |
| printable text | insert at cursor |
| Left/Right | move by Unicode scalar boundary |
| Home/End | first/last buffer position |
| Backspace/Delete | remove adjacent scalar |
| Ctrl+w | delete preceding whitespace-delimited word |
| Ctrl+u | delete from start to cursor |
| Ctrl+k | delete from cursor to end |
| Up/Down | older/newer command from this process session |
| Tab/Shift+Tab | complete or cycle forward/backward |
| Enter | validate and execute |
| Esc | cancel and restore normal footer |

The cursor must never split UTF-8. Horizontal scrolling keeps the cursor visible
and shows edge indicators when content is clipped. History is bounded to 100
successful commands, deduplicates consecutive identical commands, contains no
secrets, and is not persisted in this phase.

### Completion

Command completion candidates are generated from:

- canonical route names;
- route aliases, labeled as aliases;
- saved-view names after the literal `view:` prefix;
- destination filter field names, operators, and bounded enum/boolean values
  after a route has been resolved.

Completion uses prefix matching first and case-insensitive substring matching
second. Prefix matches sort before substring matches; canonical names sort
before aliases; remaining ties sort by display text and stable ID.

On first `Tab`:

1. if all candidates share additional prefix text, insert the longest common
   prefix;
2. otherwise select the first candidate and open the tray.

Further `Tab`/`Shift+Tab` cycle without losing the original edit span. Any
ordinary edit resets the cycle. A single unambiguous candidate inserts it and
adds the required delimiter. `Enter` never implicitly selects a merely
highlighted candidate when the buffer itself is invalid.

## 12.4 Filter-line contract

Pressing `/` captures a restoration point containing the route's original
filter text, parsed expression, selection identity, and scroll anchor.

While editing:

- every valid parse applies live to the current snapshot;
- an invalid parse remains visible and editable;
- while invalid, the last valid result set remains rendered;
- the error appears on the prompt row with field/column context;
- selection follows stable ID if still visible, otherwise the nearest valid
  deterministic row is selected;
- the header shows visible/total counts from the last valid expression;
- no request is sent unless that route explicitly has a documented server-side
  query mode.

`Enter` commits the latest valid text. Empty valid text clears the filter.
`Enter` on invalid text does nothing except retain the error. `Esc` restores the
entire restoration point. Navigating away is not permitted while the filter line
is active; the user must commit or cancel first.

Filters are owned by view frames. A filter on Devices must not appear on Users,
another history frame, or an unrelated saved view.

### Filter completion

Completion is supplied by the active route's explicit filter schema:

```rust
struct FilterSchema {
    fields: Vec<FilterFieldSpec>,
}

struct FilterFieldSpec {
    canonical_name: &'static str,
    aliases: &'static [&'static str],
    operators: &'static [FilterOperator],
    value_kind: FilterValueKind,
}
```

The concrete types may avoid allocation, but every route must declare its
schema rather than using a global list that suggests invalid fields.

Complete:

- field names before the separator;
- supported comparison/operator syntax after a field;
- `true`/`false`, documented enums, resource tags, operating systems, owners,
  and other values already present in the current snapshot when bounded;
- closing quotes only when doing so produces valid syntax.

Dynamic candidates are deduplicated, redacted, capped at 100, and ordered
deterministically. Do not index or suggest secret, policy-source, clipboard, or
one-time-result content.

## 12.5 Transient action menus

### Invocation

`a` enters `TransientKind::Action`; `y` enters `TransientKind::Copy`. The bottom
bar immediately displays the available next keys. The user does not navigate a
list with arrows or `j`/`k`.

Pressing a displayed leaf key invokes that action. Pressing a displayed prefix
key replaces the row with the next group and a breadcrumb:

```text
Actions › service  s serve  f funnel  c clear  Esc cancel
```

Menu depth is at most two keys after `a` or `y`. There is no timer. `Esc`
cancels. The prefix key repeated at a nested level has no magical meaning unless
registered. Unknown keys keep the menu open and show a short non-modal
notification; they never invoke a neighboring item.

### Key sequences

Each contextual action/copy field has one explicit stable key sequence in the
action registry. Do not derive production mnemonics from the first available
letter at runtime. The registry must reject:

- duplicate leaf sequences in one context;
- a leaf that is also a prefix;
- prefixes deeper than two;
- reserved global sequences;
- a sequence whose parent group is absent;
- a visible action with no sequence;
- a sequence pointing to a different action ID.

Disabled actions are visible when space permits and in help, with a reason. A
disabled leaf key shows the reason and stays in the transient. Hidden actions
are reserved for operations that do not apply to the context at all, not for
missing permissions or source availability.

The action registry remains the source for action ID, label, description,
context, capability, risk, key sequence, and availability. Footer, transient,
help, mouse hit regions, and tests consume the same resolved entries.

### Copy behavior

`y` exposes only fields available for the selected resource and allowed by the
redaction/secret contract. Copy invocation:

- uses the existing clipboard abstraction;
- acknowledges the field label, never its copied value;
- leaves no copied value in task history, logs, diagnostics, or notifications;
- closes the transient on success;
- remains open with a safe error on failure;
- never exposes a closed one-time secret result.

## 12.6 Contextual help sheet

`?` opens a bottom-docked which-key-style help sheet for the current route,
focus, selection, capabilities, and interaction mode. It is a reference surface,
not an action picker.

The sheet groups entries in this order:

1. Navigation;
2. View;
3. Actions;
4. Copy;
5. Tasks and refresh;
6. Global and exit.

Each entry shows key sequence, label, and—when disabled—its concise reason.
Groups use columns when width permits and flow vertically otherwise. The sheet:

- grows upward from the footer;
- uses at most 60% of terminal height;
- scrolls only when content exceeds that bound;
- does not select rows or execute actions;
- closes with `?` or `Esc`;
- supports `/` to filter help labels and keys only while the sheet is open;
- restores the prior route focus unchanged;
- includes a legend for disabled, destructive, and prefix entries without
  relying on color alone.

Arrow/PageUp/PageDown and `j`/`k` may scroll overflowing help content; this is
document navigation, not option selection. At minimum size, show the most
important global escape and navigation keys plus an overflow count.

Help is generated from the same registry and route metadata as normal input.
Static duplicate binding tables may remain in user documentation but may not
become a second runtime source of truth.

## 12.7 View history stack

Replace `Vec<Route>` with a bounded browser-style history:

```rust
struct ViewHistory {
    frames: Vec<ViewFrame>,
    cursor: usize,
    capacity: usize, // exactly 100
}

struct ViewFrame {
    route: Route,
    focus: FocusTarget,
    selection: Option<ResourceIdentity>,
    scroll_anchor: Option<ResourceIdentity>,
    filter: ViewFilterState,
    sort: ViewSortState,
    section: Option<ViewSection>,
    saved_view: Option<SavedViewId>,
}
```

Use route-specific enums/state where necessary; do not serialize unrelated
fields into strings. A frame stores presentation intent only. It never stores
source snapshots, credentials, secret results, form input, active tasks,
notifications, confirmation state, or adapter handles.

### History operations

- `[` moves to `cursor - 1` when available.
- `]` moves to `cursor + 1` when available.
- moving at either boundary is a no-op with a subtle notification.
- navigating to a new non-equivalent frame after moving backward truncates all
  forward frames before appending.
- an equivalent navigation does not append a duplicate.
- exceeding 100 removes the oldest frame and adjusts the cursor.
- history is process-local and is not persisted.
- first run begins with exactly one frame for the resolved initial route.

Before leaving a route, capture its current restorable fields. On restoration:

- select the stored stable identity when present;
- if it no longer exists, choose the first deterministic visible item and show
  a non-modal `previous selection no longer exists` notice;
- clamp scroll and section values to current content;
- re-evaluate filter/sort against the current snapshot;
- never resurrect a closed secret or stale form;
- schedule only the normal route freshness policy, not an unconditional refresh.

`q` no longer consumes history. In normal mode it exits immediately when safe,
or opens the existing active-task quit confirmation. `Esc` only cancels the
active interaction; in Normal mode it is a no-op. `h` retains collection/detail
navigation where registered but does not mean browser history.

### Bracket collision

`[` and `]` are globally reserved for history. Remove their current Services
section behavior. Use `H` and `L` for previous/next sibling section in Services
and any view with peer sections. Show those keys only where applicable. Literal
bracket input remains possible inside text editors through the editor's higher
key-precedence mode.

## 12.8 Alerts, confirmations, forms, and panels

Centered modal overlays are limited to:

- alerts that require explicit acknowledgement before continuing;
- risk confirmations that require review, mnemonic, target, or phrase.

The following must not use a centered modal:

- command input;
- filter input;
- action choice;
- copy-field choice;
- contextual help;
- autocomplete results;
- ordinary non-blocking errors or success notices.

Typed action parameters are not alerts. Render short forms in a bottom sheet and
long/complex forms in the main collection/inspector area or a dedicated route.
Policy editors and one-time secret results retain dedicated views. A form's
preview may lead to a centered confirmation because that confirmation is a risk
boundary.

Modal rendering must dim or visually subordinate underlying content through
the semantic theme roles introduced in Specification 13; until then use the
current theme abstraction, not new literal widget colors.

## 12.9 Mouse and resize parity

When mouse support is enabled:

- clicking a footer hint invokes the same action as its key;
- clicking a completion selects/inserts it but does not bypass validation;
- clicking a transient leaf invokes it; a prefix opens its group;
- clicking outside a command/filter/transient/help surface does not silently
  commit; it cancels only when equivalent to `Esc` for that mode;
- history remains available through footer/help targets, with no assumption of
  terminal mouse back buttons;
- hit regions are cleared and rebuilt every frame.

On resize, preserve buffers, cursor, history, and transient prefix. Recompute
tray rows and help columns. If the terminal falls below 60x18, render the
minimum-size explanation plus the active prompt text and `Esc cancel`; do not
discard edits.

## 12.10 Required implementation touchpoints

Inspect and update all affected paths, including:

```text
src/action.rs
src/app.rs
src/event.rs
src/effect.rs
src/domain/filter.rs
src/domain/saved_view.rs
src/ui/layout.rs
src/ui/mod.rs
src/ui/components/footer.rs
src/ui/components/help.rs
src/ui/components/overlay.rs
src/ui/components/action_picker.rs       # remove
src/ui/components/copy_picker.rs         # remove
src/ui/components/command_palette.rs     # remove
src/ui/components/filter.rs              # replace with inline implementation
src/ui/components/command_line.rs        # add if this boundary is useful
src/ui/components/completion_tray.rs     # add if this boundary is useful
src/ui/components/transient_menu.rs      # add
src/ui/components/help_sheet.rs          # add or replace help.rs
src/ui/views/services.rs
tests/actions.rs
tests/app_reducer.rs
tests/filter.rs
tests/ui_snapshots.rs
tests/ui_services.rs
tests/acceptance/
docs/architecture.md
docs/product.md
docs/ux.md
docs/configuration.md
docs/troubleshooting.md
```

Remove obsolete overlay variants, picker state, renderer dispatch, key handlers,
and snapshots. Do not retain old components behind unused code or a config flag.
Search for `route_stack`, `ActionPicker`, `CopyPicker`, `CommandPalette`,
`FilterEditor`, and bracket bindings after migration.

## 12.11 Required tests

### Reducer and registry tests

Prove:

1. key precedence for every interaction mode;
2. all registry sequences are collision-free per context;
3. disabled actions remain discoverable and cannot execute;
4. action/copy keys invoke the exact registered ID without list navigation;
5. transient prefix depth, breadcrumb, unknown-key, escape, and no-timeout rules;
6. command grammar, aliases, saved views, invalid target, invalid trailing
   filter, empty submit, and equivalent no-op;
7. Unicode editing and horizontal cursor visibility;
8. bounded/deduplicated session history;
9. completion ordering, longest common prefix, cycling, reset-on-edit, and no
   implicit invalid execution;
10. route-specific filter completion never suggests an invalid field;
11. live valid filter, invalid-last-good, commit, clear, and full cancel restore;
12. help groups and disabled reasons derive from the registry;
13. `q`, `Esc`, `[`, `]`, `h`, `H`, and `L` follow the new ownership rules.

### History property tests

Generate navigation sequences and prove:

- cursor is always within a non-empty frame list;
- capacity never exceeds 100;
- backward then forward restores equivalent frames;
- new navigation after backward removes every forward frame;
- equivalent navigation does not increase length;
- snapshot refresh cannot alter stored resource identity;
- missing identities restore deterministically without panic;
- no secret/form/task/adapter data is representable in `ViewFrame`.

### Rendered-buffer tests

At 160x45, 110x30, 80x24, and 60x18, assert rendered cells for:

- normal footer;
- command prompt with zero, one, six, and overflowing candidates;
- filter prompt with a positioned error;
- action and nested action transients;
- copy transient;
- normal and filtered help sheet;
- backward/forward availability;
- alert and every risk-tier confirmation;
- short and long typed forms;
- Unicode and wide-character command/filter input;
- resize while each mode is open.

Assert command, filter, transient, completion, and help surfaces are bottom
anchored. Assert only alert/confirmation snapshots contain a centered modal
rectangle. Snapshot labels alone are insufficient; inspect buffer coordinates.

### Acceptance journeys

Script deterministic mock-mode journeys for:

1. `:devices owner:alice online:true`, completion, execute, back, and forward;
2. live `/` filtering, invalid edit, cancel restoration, then valid commit;
3. `a` direct action, disabled action reason, nested service action, and
   confirmation;
4. `y` direct copy with redacted acknowledgement;
5. `?` contextual help, help filter, resize, and close;
6. history selection restoration after resource removal;
7. new navigation after backward proving the forward branch was discarded;
8. `q` with no tasks and with active tasks;
9. mouse parity for footer, completion, transient, and cancellation.

No journey may contact a real daemon, CLI, Control API, keyring, clipboard, or
tailnet.

## 12.12 Performance and accessibility gates

- normal key dispatch to render request remains p95 at or below 16 ms;
- completion over 100 candidates remains p95 at or below 16 ms;
- filtering 5,000 devices retains the Phase 9 off-thread budget;
- opening help does not clone source snapshots or action payloads;
- a held key cannot grow history, notifications, or completion buffers without
  their documented bounds;
- every operation is available without mouse;
- every color distinction also has a symbol, label, border, or modifier;
- screen-reader/linear terminal output encounters prompt text after route
  content in visual order;
- focus and active mode remain identifiable in ANSI16 and no-color modes.

## 12.13 Exit gate

Phase 12 is complete only when:

- `:` and `/` are inline bottom prompts with the specified editors;
- completion and filtering obey their exact schemas and bounds;
- `a` and `y` are direct transient key menus with no up/down picker UX;
- `?` is a contextual which-key-style bottom help sheet;
- `[` and `]` implement bounded backward/forward history restoration;
- `q`, `Esc`, `h`, `H`, and `L` have no conflicting meaning;
- only alerts and confirmations use centered modals;
- the action registry is the single runtime binding source;
- old picker components, overlay states, and `route_stack` are absent;
- keyboard, mouse, minimum-size, resize, and accessibility contracts pass;
- rendered-buffer tests prove placement rather than relying on state alone;
- all Specification 11 transport behavior remains passing;
- `cargo fmt --all -- --check` passes;
- `cargo clippy --all-targets --all-features -- -D warnings` passes;
- `cargo test --all-targets --all-features --locked` passes.

Do not begin Specification 13 until the old interaction paths have been removed
and this gate is green.
