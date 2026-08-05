# Specification 12 — Interaction shell and stack navigation

- Implementation phase: 12
- JJ change description: `feat: redesign interaction shell and navigation`
- Depends on: Specification 11 complete
- Produces: a bottom-anchored, keyboard-first interaction shell inspired by
  jjui and k9s without copying their source

This phase replaces Tale's centered navigation, filter, action, and copy pickers
with a coherent interaction grammar. Navigation uses a fuzzy adaptive grid at the
bottom, filters remain inline, action and copy prefixes are transient key menus,
contextual help is a which-key-style bottom sheet, and route history has explicit
backward and forward movement.

The redesign changes interaction structure, not product-domain behavior. All
existing actions, forms, risk confirmations, tasks, source states, redaction,
and mutation verification contracts remain in force.

## 12.0 Phase contract

### User-visible result

- `:` opens the canonical route grid and fuzzy navigation prompt.
- `/` opens a one-line filter prompt in the footer.
- `Tab` cycles navigation results or completes route-valid filter terms.
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

navigation palette:
│ Views   Esc Close                                                 │
│                                                                │
│ Fleet                  Local                 Network             │
│ devices  Machines...   local     This...     routes  Network...  │
│ users    Members       services  Serve...    dns     Name...     │
│ overview Fleet summary                       access  Policies    │
│                                                                │
│ Operations                                                     │
│ credentials Keys & tokens                                      │
│ activity    Tasks & audit                                      │
│ settings    Configuration                                      │
│                                                                │
│ : dvcs▏                                                         │
│ Enter Open best match                                           │

filter with error:
│ owner:        Owner name or ID                                   │
│ / owner:"alice online:true        unclosed quote at column 7     │

transient:
│ Actions  r refresh  p ping  e exit node  s services  Esc cancel  │
```

Rules:

- normal mode uses one footer row;
- navigation mode reserves a stable-height adaptive grid with its prompt and hints;
- filter mode replaces the normal row with its prompt and completion tray;
- transient mode replaces that row with its key menu;
- navigation uses three group columns at 120 columns and two at supported widths
  below that;
- the navigation grid reserves two group bands, so filtering never moves the prompt;
- filter completion trays show at most six candidates, plus an overflow count;
- bottom content never covers an alert or confirmation;
- at 60x18, the complete two-column grouped catalog and prompt remain functional;
- no bottom interaction surface is rendered through `Clear` over a centered
  rectangle;
- mouse hit regions are derived from the final rendered spans.

Long labels are elided as whole items. Never show a key without enough of its
label to identify the action. When all items cannot fit, retain `? help` and an
overflow indicator.

## 12.3 Command-line contract

### Catalog and matching

The palette contains exactly these canonical routes:

```text
devices  local  services  users  routes  dns
access  credentials  activity  overview  settings
```

The leading colon is UI chrome and is not stored in the input buffer. There are
no aliases and no secondary command grammar. Saved views and filters remain in
their dedicated action and `/` interactions. The palette is not a shell: quotes,
separators, substitutions, pipes, redirects, and executable names have no
execution meaning.

Execution rules:

- empty input shows the complete canonical catalog;
- input fuzzy-matches canonical names and concise descriptions;
- matches sort by fuzzy score with catalog order as the stable tie-breaker;
- `Enter` opens the highest-scoring result, never raw input;
- no matches leave the palette open with `No matching view`;
- successful execution creates one view-history entry;
- selecting the current route closes the palette, resets collection focus, and
  creates no redundant history entry.

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
| Enter | open the highest-scoring canonical route |
| Esc | cancel and restore normal footer |

The cursor must never split UTF-8. Horizontal scrolling keeps the cursor visible
and shows edge indicators when content is clipped. Navigation input is not stored
as command history.

### Fuzzy result presentation

Each result contains:

- one canonical route name;
- one concise description;
- match indices for semantic accent highlighting;
- the canonical `Route` value executed by `Enter`.

Use a maintained fuzzy matcher with case-insensitive smart normalization. Matching
characters use the key/accent role and descriptions use muted text. Routes are
grouped under filled semantic headings. Within each group, the widest visible route
name determines the description column, avoiding arbitrary global spacing while
preserving alignment. Blank rows separate group bands. The grid renders no selection
cursor or highlight; results are already ordered by match quality and `Enter` opens
the first one. Grid height remains constant while the result count changes.

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
- navigation grid with the full catalog, fuzzy results, and no matches;
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

1. `:dvcs`, execute, `/ owner:alice online:true`, back, and forward;
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
