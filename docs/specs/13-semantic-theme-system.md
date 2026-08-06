# Specification 13 — Tailscale-inspired semantic theme system

- Implementation phase: 13
- JJ change description: `feat: add Tailscale-inspired theme system`
- Depends on: Specification 12 complete
- Produces: a complete semantic visual language for every Tale surface

Theme is application architecture in this phase, not decoration. Color,
modifier, border, symbol, and surface roles must consistently communicate
hierarchy, focus, source, freshness, risk, and state across the entire TUI.

The built-in palettes are inspired by Tailscale's current public design system,
including its warm neutral surfaces, blue focus/accent scale, and semantic
green, orange, red, and purple scales. Tale does not claim to reproduce or be
endorsed by the private Tailscale admin console.

## 13.0 Phase contract

### User-visible result

Tale ships three complete themes:

```text
tailscale-dark   default; explicit warm dark surfaces
tailscale-light  explicit warm light surfaces
terminal         preserves the terminal's default foreground/background
```

Each works in truecolor, ANSI-256, ANSI-16, and no-color capability modes. The
selected theme applies to the first frame, can be previewed and changed for the
current session from Settings, and never leaves a partially restyled screen.

### In scope

- semantic palette and style-role types;
- the three built-in themes and four color-capability projections;
- a complete audit/migration of every widget and view;
- hierarchy, focus, selection, source, freshness, risk, and status grammar;
- Settings preview and session selection;
- configuration, doctor, support, screenshots/snapshots, and theme docs;
- contrast, distinguishability, exhaustive role, and rendered-buffer tests.

### Explicitly out of scope

- user-authored theme files or arbitrary RGB configuration;
- importing themes from k9s, jjui, Base16, terminal schemes, or the web;
- automatic light/dark detection from undocumented terminal escape sequences;
- animated or gradual theme transitions;
- font selection, terminal emulator configuration, image protocols, gradients,
  alpha blending, or assumed transparency;
- a compatibility alias for old theme names;
- leaving literal widget colors as an undocumented escape hatch.

## 13.1 Research and palette provenance gate

Before implementation, add:

```text
docs/decisions/0005-semantic-theme-system.md
docs/design/theme-token-ledger.md
```

Decision 0005 must record:

- official public Tailscale design sources inspected and their retrieval date;
- the exact public CSS artifact or documented palette source used as reference;
- copied numeric color facts and their source URL;
- which Tale tokens are direct references versus Tale-specific mappings;
- contrast calculations and exceptions;
- truecolor-to-ANSI projection method and reviewed results;
- why terminal appearance auto-detection is not claimed;
- how no-color preserves every meaning;
- trademark/attribution wording and why themes are called “inspired”.

At minimum, inspect Tailscale's public “Heart of dark mode” design-system post,
which documents semantic text, border, focus, dual-surface, disabled-state, and
light/dark design considerations:

```text
https://tailscale.com/blog/heart-of-dark-mode
```

The token ledger is the auditable source of palette values. It must include
source, semantic purpose, truecolor value, ANSI-256 index, ANSI-16 color and
modifier, no-color modifier/symbol, and measured contrast for every role pair.
Do not scrape live CSS during build, test, or runtime.

The implementing agent must re-verify the proposed values below against the
retrieval recorded in the ledger. If the current public source differs, update
the ledger and the palette deliberately in this phase; do not retain two
versions or add a fallback.

## 13.2 Type and ownership model

The theme module owns all Ratatui color selection:

```rust
pub enum ThemeId {
    TailscaleDark,
    TailscaleLight,
    Terminal,
}

pub enum ColorCapability {
    TrueColor,
    Ansi256,
    Ansi16,
    None,
}

pub struct Theme {
    id: ThemeId,
    capability: ColorCapability,
    palette: Palette,
}

pub enum StyleRole {
    // exhaustive semantic roles from 13.3
}

impl Theme {
    pub fn style(&self, role: StyleRole) -> ratatui::style::Style;
}
```

Exact internal representation may use static data and avoid allocation. These
boundaries are mandatory:

- views/components request `StyleRole`, never a palette shade;
- only the theme implementation converts palette colors to Ratatui `Color`;
- capability projection is selected once during resolved configuration;
- a render receives one immutable resolved `Theme`;
- theme switching replaces that value atomically before requesting a frame;
- domain types contain no Ratatui style/color values;
- action availability and status semantics do not depend on a theme.

After migration, literal `Color::*`, `Color::Rgb`, `Color::Indexed`, and raw
foreground/background construction are forbidden outside `src/ui/theme/` and
theme tests. Enforce this with a repository test that scans Rust sources. Do not
hide literal colors behind view-local helper functions.

Use a `src/ui/theme/` directory if the roles, palettes, and projections no
longer fit clearly in one file:

```text
src/ui/theme/
  mod.rs
  role.rs
  tailscale_dark.rs
  tailscale_light.rs
  terminal.rs
  projection.rs
```

Do not create a generic runtime theme-plugin system.

## 13.3 Required semantic roles

Every role below must be represented and documented. Multiple roles may map to
the same terminal style only when the ledger explains how another signal keeps
them distinguishable.

### Surfaces and structure

```text
Canvas
Surface
SurfaceRaised
SurfaceInset
Backdrop
BorderSubtle
BorderNormal
BorderFocused
BorderDanger
Divider
```

Use Canvas for the application background, Surface for collections/inspectors,
SurfaceRaised for bottom sheets and centered modals, and SurfaceInset for input,
code, diffs, and secondary wells. A focused border is consistent everywhere;
do not use unrelated per-view focus colors.

### Text and interaction

```text
TextPrimary
TextMuted
TextDisabled
TextInverse
TextLink
TextCode
KeyHint
KeyHintDisabled
Prompt
CompletionMatch
CompletionSelected
Selection
SelectionInactive
Focus
```

Selection and focus are different. Selection identifies the current resource;
focus identifies the pane or control receiving input. When both apply, the
style composition rule must preserve both—normally selection background plus a
focused border/cursor or bold primary identity.

### Operational state

```text
StateHealthy
StateInfo
StateWarning
StateDanger
StatePending
StateDisabled
StateUnknown
StateStale
StatePublic
StateDirect
StateRelay
StateOffline
```

Operational roles always include a stable symbol/label mapping:

| Role family | Unicode symbol | ASCII symbol |
| --- | --- | --- |
| healthy/success | `●` or `✓` by context | `+` or `OK` |
| info/direct | `●` or `i` | `i` |
| warning/stale/relay | `▲` or `!` | `!` |
| danger/public/failure | `◆` or `×` | `X` |
| pending | `◌` or spinner | `~` |
| disabled/unknown/offline | `○` or `?` | `-` or `?` |

The exact symbol is chosen by context, but one role may not reverse meaning in
another view. “Public” uses danger/risk emphasis even when technically healthy.
“Relay” is informational or warning according to the existing domain result,
never green merely because the connection works.

### Source, risk, task, and data roles

```text
SourceLocal
SourceAdmin
SourceCombined
RiskObserve
RiskReversible
RiskDisruptive
RiskDestructive
TaskQueued
TaskRunning
TaskSucceeded
TaskFailed
TaskCancelled
DiffAdded
DiffRemoved
DiffChanged
Secret
Redacted
```

Local and admin source colors identify provenance, not health. Combine source
role with a health label/symbol rather than changing source color from green to
red. Combined data uses its own role plus explicit `local+admin` text.

Secrets use emphasis only while legitimately visible. Redacted text must never
reveal length through color runs, per-character styling, or placeholders.

## 13.4 Truecolor reference palettes

The following tables are the initial Tale mapping to verify and record in the
token ledger. These are not permission for widgets to use raw values.

### Tailscale Dark

| Semantic use | RGB |
| --- | --- |
| canvas | `#181717` |
| surface | `#232222` |
| raised | `#2E2D2D` |
| inset/backdrop | `#181717` |
| text primary | `#F9F7F6` |
| text muted | `#AFACAB` |
| text disabled | `#706E6D` |
| border subtle | `#2E2D2D` |
| border normal | `#585757` |
| focus/accent | `#ADC7FC` |
| accent strong | `#85AAF5` |
| healthy | `#85D996` |
| info/local | `#ADC7FC` |
| admin/combined | `#E3C3FA` |
| warning | `#EFC078` |
| danger/public | `#FFB1AB` |

### Tailscale Light

| Semantic use | RGB |
| --- | --- |
| canvas | `#F9F7F6` |
| surface | `#FFFFFF` |
| raised | `#FFFFFF` |
| inset | `#EEEBEA` |
| backdrop | `#DAD6D5` |
| text primary | `#181717` |
| text muted | `#706E6D` |
| text disabled | `#AFACAB` |
| border subtle | `#EEEBEA` |
| border normal | `#DAD6D5` |
| focus/accent | `#3F5DB3` |
| accent strong | `#324994` |
| healthy | `#09825D` |
| info/local | `#4B70CC` |
| admin/combined | `#8052A1` |
| warning | `#BB5504` |
| danger/public | `#B22D30` |

Use contrast-appropriate foreground/background combinations from the ledger;
do not assume a semantic hue works as small text on every surface. Filled
selection and alert styles may use a lighter/darker scale member so long as the
role mapping remains fixed and documented.

### Terminal theme

`terminal` uses `Color::Reset` for Canvas, Surface, and primary foreground. It
still uses capability-appropriate semantic accents for focus and state. It must
not paint a large dark or light background. Raised/inset hierarchy is expressed
with borders and modifiers when a reliable background cannot be assumed.

## 13.5 Capability projection

### Truecolor

Use the verified RGB values exactly. No runtime color-space conversion is
needed.

### ANSI-256

For every palette value, precompute and review one xterm-256 index using a
documented perceptual-distance method. Store the chosen indices in the ledger
and source; do not calculate them on every frame. If nearest-color projection
collapses two adjacent semantic roles, choose a reviewed neighboring index or
add a modifier/symbol.

### ANSI-16

Map roles deliberately to named standard colors plus modifiers. Never assume
terminal RGB definitions. Required families are:

| Meaning | Base ANSI family | Additional signal |
| --- | --- | --- |
| focus/info/local | blue/cyan | bold or underline |
| healthy/success | green | symbol |
| warning/stale | yellow | bold + symbol |
| danger/public/failure | red | bold + symbol |
| admin/combined | magenta | label |
| muted/disabled | dark gray/default | dim where supported + label |

Selection must remain legible on terminals that implement “bright” as bold.
Test both direct color and modifier output; do not require exact emulator RGB.

### No color

`ColorCapability::None` emits only `Color::Reset` foreground/background. It uses
bold, dim, underline, reversed, borders, symbols, and explicit words. No-color
must retain:

- focused versus unfocused pane;
- selected versus unselected row;
- healthy/warning/danger/pending/offline;
- local/admin/combined source;
- disabled versus available action;
- risk tiers;
- diff add/remove/change;
- active prompt and completion selection.

Never rely on dim alone because terminals may ignore it.

## 13.6 Style composition rules

Widgets often need several meanings. Resolve them in this precedence:

1. secret/redaction safety;
2. modal/prompt cursor and active focus;
3. selected row/control;
4. danger/public/destructive state;
5. resource operational state;
6. source provenance;
7. base text/surface.

Higher precedence may replace foreground/background but lower-precedence
meaning must remain through a label, symbol, border, or surviving modifier. Add
a small typed style-composition helper rather than ad hoc `.patch()` ordering in
views. Document every allowed composition and test it.

Examples:

- selected offline admin device: selection background, primary identity text,
  `○ offline`, and `admin` source label;
- focused destructive confirmation: danger border, focused title modifier,
  explicit risk phrase, and visible cursor;
- stale local snapshot: local source label in SourceLocal plus `! stale 4m` in
  StateStale;
- disabled public-service action: disabled key style plus `public exposure`
  description and capability reason.

## 13.7 UI application contract

Audit every rendered cell-producing path. At minimum:

- application canvas and minimum-size screen;
- header source indicators and route title;
- collection tables, selection, hover, empty/error/loading rows;
- inspectors, sections, values, links, code, and redaction;
- normal footer, command/filter prompts, completions, transients, and help;
- forms, editors, cursors, validation, previews, and diffs;
- alerts, confirmations, backdrop, and risk tiers;
- notifications and task states;
- health, diagnostics, flow, audit, policy, service, transfer, credential, and
  secret-result views;
- charts/sparklines and legends where present;
- disabled states and source freshness in Settings.

Color must describe the interface consistently:

- blue is focus, navigation, information, and local-source identity—not generic
  decoration on every heading;
- green means verified healthy/success, never merely requested or running;
- orange/yellow means warning, stale, relay, or reversible caution;
- red means failed, dangerous, destructive, or public exposure;
- purple distinguishes admin/combined provenance where a label accompanies it;
- warm neutrals carry hierarchy and reduce border noise.

Pending optimistic state may not use success green. A mutation becomes green
only after its required verification read succeeds.

## 13.8 Configuration and session selection

Add:

```toml
[ui]
theme = "tailscale-dark"
color = "auto"
```

`theme` accepts exactly `tailscale-dark`, `tailscale-light`, or `terminal`.
Unknown values are configuration errors. There are no aliases.

`color` continues to accept the existing capability policy. Resolve it with the
existing CLI/environment precedence. `NO_COLOR` forces no-color regardless of
theme. A forced capability unsupported by the terminal follows the existing
documented policy; theme selection never upgrades capability.

Settings shows:

- configured theme and provenance;
- resolved color capability and reason;
- a compact preview containing surfaces, selection, focus, all state families,
  sources, and risk tiers.

An `Appearance` action from Settings opens a bottom-sheet form with the three
built-ins. Changing selection previews immediately. `Enter` applies it for the
current process session; `Esc` restores the original theme exactly. Tale does
not edit the configuration file automatically in this phase. The form tells the
user the exact configuration key needed for persistence.

The selected theme is resolved before terminal entry/first render. There is no
default-theme flash. Applying a session theme has no animation, delay, adapter
I/O, route change, selection loss, or history entry.

## 13.9 Required implementation touchpoints

Inspect and update all style-producing paths, including:

```text
src/config.rs
src/doctor.rs
src/app.rs
src/action.rs
src/ui/theme.rs                  # replace or convert to module
src/ui/layout.rs
src/ui/text.rs
src/ui/components/
src/ui/views/
tests/config.rs
tests/app_reducer.rs
tests/ui_*.rs
tests/ui_snapshots.rs
tests/acceptance/
docs/architecture.md
docs/configuration.md
docs/product.md
docs/ux.md
docs/support.md
docs/troubleshooting.md
docs/design/theme-token-ledger.md
```

Search all Rust files for `Color::`, `.fg(`, `.bg(`, `Style::default()`, and
modifier construction. Not every `Style::default()` is wrong, but every result
must be reviewed. Remove current color helper functions when their semantics are
replaced; do not preserve them as aliases.

## 13.10 Required tests and evidence

### Exhaustiveness and policy

Tests must prove:

1. every `StyleRole` resolves in every ThemeId × ColorCapability combination;
2. no production Rust file outside the theme module contains literal Ratatui
   colors;
3. every operational role has Unicode and ASCII/non-color signaling;
4. every action risk and source state maps to an explicit role;
5. unknown theme configuration is rejected;
6. `NO_COLOR` produces no non-reset color cells;
7. theme preview cancel restores byte-for-byte-equivalent resolved theme state;
8. session apply changes theme only, without navigation/history/source effects.

### Contrast and distinguishability

For truecolor palettes, calculate WCAG relative luminance and require:

- normal primary/muted text: at least 4.5:1 against every surface on which it
  appears;
- large/bold labels and non-text state boundaries: at least 3:1;
- focus, selection, and input cursor: at least 3:1 against adjacent colors;
- disabled text: document any intentional lower contrast, retain an explicit
  disabled label/symbol, and never use it for required instructions.

ANSI-256 tests verify distinct indices or compensating modifiers. ANSI-16 and
no-color tests verify distinct serialized styles plus labels/symbols; they do
not claim RGB contrast the terminal controls.

### Render matrix

Render representative complete screens at 160x45, 110x30, 80x24, and 60x18 for
all 12 ThemeId × ColorCapability combinations. Include:

- overview with mixed source health;
- selected device with local/admin data;
- inline command and filter errors;
- action transient and help sheet;
- pending/success/failed task rows;
- public service warning;
- policy diff and destructive confirmation;
- Settings theme preview;
- minimum-size screen.

Rendered-buffer assertions must inspect actual foreground, background,
modifier, symbol, border, and label cells. Snapshot files are reviewed for
meaning and hierarchy, not mechanically accepted.

### Manual visual evidence

For each claimed Supported platform, capture a sanitized terminal screenshot or
buffer dump for:

- truecolor dark;
- truecolor light;
- terminal theme;
- ANSI-16;
- no-color;
- focused confirmation over a complex view.

Record terminal emulator, `$TERM` classification without private environment,
OS, Tale build hash, and reviewer outcome. Evidence cannot contain real tailnet
identities or addresses.

## 13.11 Performance and resilience

- theme lookup is allocation-free on the render hot path;
- theme switching performs no source clone, I/O, or background task restart;
- a full theme switch to next frame remains within the 33 ms 160x45 render
  budget;
- snapshot count and theme tables remain bounded and static;
- a malformed config fails before alternate-screen entry;
- terminal restoration is unchanged after a theme/render error;
- no-color and terminal themes work when the terminal reports no background
  information;
- theme data cannot contain secrets or user-derived escape sequences.

## 13.12 Exit gate

Phase 13 is complete only when:

- three built-in themes and four capability projections are complete;
- Tailscale inspiration and exact palette provenance are documented;
- every view/component uses semantic roles;
- no literal production widget color exists outside the theme module;
- hierarchy, focus, selection, source, freshness, risk, and state meanings are
  consistent across the whole interface;
- color is never the sole carrier of meaning;
- Settings preview/apply/cancel and configuration behavior are exact;
- truecolor contrast and reduced-color distinguishability gates pass;
- the full rendered-buffer and manual evidence matrices are reviewed;
- no custom-theme, auto-detection, compatibility, or fallback system was added;
- Specifications 11 and 12 remain passing;
- `cargo fmt --all -- --check` passes;
- `cargo clippy --all-targets --all-features -- -D warnings` passes;
- `cargo test --all-targets --all-features --locked` passes.

Do not begin Specification 14 until every production rendering path has passed
the semantic-role audit.
