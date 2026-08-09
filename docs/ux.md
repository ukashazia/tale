# Interaction and user flows

## Information architecture

Tale is resource-oriented. A bottom `:` palette provides fuzzy navigation; the UI does
not spend permanent width on a large sidebar.

Canonical routes:

| Route | Default content |
| --- | --- |
| `overview` | health and actionable queues (not yet the default landing route) |
| `local` | local node and preferences |
| `devices` | device inventory |
| `users` | member inventory |
| `routes` | subnet and exit routes |
| `dns` | tailnet DNS and query tool |
| `access` | policy source and tests |
| `services` | local and tailnet services |
| `credentials` | supported credentials |
| `tasks` | what this client did: one row per background run |
| `audit` | what the tailnet was told: configuration log, streams, webhooks |
| `settings` | Tale and supported tailnet settings |

The empty palette groups every route into a breathable adaptive grid with `Fleet`,
`Local`, `Network`, and `Operations` headings. Typing fuzzy-matches route names and
their concise descriptions; `dvcs` finds `devices` and `tsks` finds `tasks`.
Aliases, saved views, filters, and shell syntax are not part of navigation. Filtering
is a separate `/` interaction.

## Frame layout

Wide terminals use a collection-and-inspector layout:

```text
┌ Tale   example.com   connected locally   updated 8s ago          ┐
│ devices  24/31   Data: up to date · refreshed 8s ago              │
├─────────────────────────────────────┬─────────────────────────────┤
│ STATE NAME       OWNER   OS  SEEN   │ build-01                    │
│ ● dir build-01   alice   lin now    │ 100.64.1.8 · linux 1.98.9 │
│ ○     phone      alice   ios 2h     │ Direct 18ms · tx/rx ...    │
│ …                                   │ Tags, routes, key, posture  │
│                                     │ Sources: local 1s/admin 12s │
├─────────────────────────────────────┴─────────────────────────────┤
│ a actions  y copy  / filter  : go  r refresh  [ back  ] forward ? │
└───────────────────────────────────────────────────────────────────┘
```

- At 110 columns or wider, the inspector consumes 34–45% of the width.
- From 80–109 columns, the collection fills the screen and `Enter` opens a
  full-screen detail route.
- Below 80 columns, optional columns disappear in a documented priority order;
  every field remains available in details.
- Below 60x18, Tale shows a minimum-size explanation instead of a corrupted UI.
- Settings shows the semantic palette on the page, so a theme change is visible
  where it is chosen rather than inside a preview modal.
- Command, filter, transient, completion, and help surfaces grow upward from
  the terminal edge. Only alerts and confirmations are centered modals.

The `terminal` theme preserves the terminal's existing background; the two
Tailscale-inspired themes paint explicit warm canvases. Borders communicate
focus, not decoration. Color is semantic and never the only state signal.

## Navigation contract

### Global bindings

| Key | Action |
| --- | --- |
| `:` | open the inline route command line |
| `/` | edit the active view's filter inline with live valid results |
| `?` | open contextual bottom help; `/` filters help labels and keys |
| `Tab` / `Shift+Tab` | select navigation results or complete filter fields |
| `Esc` | cancel the active interaction, or leave an open detail pane |
| `[` / `]` | restore the previous or next view-history frame |
| `r` | refresh the active resource |
| `R` | refresh every source used by the active route |
| `@` | open task history |
| `q` | quit when safe; active tasks retain their confirmation boundary |
| `Ctrl+c` | first press cancels the focused task/input; second press while idle quits |

### Collection bindings

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | move selection |
| `g` / `G` | first or last row |
| `Ctrl+d` / `Ctrl+u` | half-page down or up |
| `Enter` / `l` | open selected resource details |
| `h` / `Esc` | leave details and return to the list |
| `Space` | toggle multi-selection when the current action supports a batch |
| `s` | choose sort field and direction |
| `w` | toggle standard and wide columns |
| `a` | open a direct contextual key menu; the next registered key invokes an action |
| `y` | open a direct copy-key menu; acknowledgement names only the field |
| `H` / `L` | previous/next sibling section in Services |

An uppercase binding matches whether or not the terminal reports Shift, since
Shift is inherent to the character rather than an extra modifier. `G`, `R`, `H`,
and `L` therefore behave like any other key.

Opening a detail is always reversible: `h` and `Esc` both return to the list, and
`h back` appears in the footer for exactly as long as it applies.

Direct destructive bindings are forbidden. Destructive operations live behind
the `a` prefix and still require their typed confirmation.

Every menu that asks the user to pick one value — sort, service section,
appearance, account — uses the action menu's grammar: bottom-anchored, grouped,
direct keys, no row cursor, and no centered rectangle. A menu with one question is a single
level. A menu with two drills down: the top level lists the subjects, and the
first key replaces the menu with that subject's variants rather than filtering a
flat list. Sort lists the columns, `n` swaps the menu for `Sort · name` offering
`a ascending` and `d descending`, and `Esc` returns to the columns before it
closes the menu. `·` marks the value already in force, at both levels. None of
these are overlays.

`y` copies one value. It uses the same grouped grid as the `a` and `?` menus,
grouped as Identity, Network, and Diagnostics; a field holding several values
says how many and opens a second level listing each one plus an entry for all of
them, so copying an address never means copying a joined list. The three menus
share one grid implementation so they cannot drift apart.

A view with nothing in it never shows an empty box. It names the resource, the
reason it is empty, and the next step — an admin-backed route with no profile
says so and gives the command that adds one, rather than reporting an internal
`idle` state. Route lines for those views read `Needs an admin profile` instead
of claiming to be loading forever.

Status glyphs follow `ui.symbols`. `auto` draws Unicode, because the frame
already uses box-drawing borders and `·` separators; `ascii` is the explicit
opt-out.

### Contextual help

The footer, transient menus, and bottom help sheet are generated from the action
registry. When space is insufficient the footer ends with `? more`; it never
truncates a key while leaving a misleading label. The `a` menu is a tall adaptive
grid of semantic groups. It shows complete one- and two-key sequences at once;
disabled actions remain visible as dimmed, crossed-out entries and report their
reason only when invoked. Transients do not use row selection, arrows, or a
timeout. `Esc` first clears a pending two-key prefix, then closes the menu; an
unknown key leaves the menu open.

The `?` sheet is a tall, responsive grid of immediately usable keys, ordered as
Navigation, Current view, Search & commands, Data, and Global. It does not
duplicate the action or copy catalogs: `a` and `y` lead to those dedicated
menus. Keys use the key-hint role, headings use the section role, and labels use
muted text. Every listed key closes help and immediately performs its normal
action; `Esc` and `?` close without acting. The sheet and quick footer use the
same compact lower-case vocabulary (`: command`, `C-d page-down`) and color
keys separately from their muted one-word descriptions.

The `:` palette supports Unicode-safe editing and true fuzzy matching. It has no
selection cursor; `Enter` opens the highest-scoring result.

Both prompts mark the insertion point with the real terminal cursor rather than
a drawn caret, so it sits on the character it is about to overwrite and costs no
column. Its shape and blink are never changed, so it looks like the cursor every
other program on that terminal draws. It appears only while an editor is open.

The `/` filter uses the same grouped grid as the help and action menus. Opening
it shows the whole field catalogue for the current route at once, each field
beside a concise description, so nothing has to be known in advance. Typing narrows the grid to one ranked list of matches. Matching is
token-aware and fuzzy: the token under the cursor completes field names before
the separator and that field's values after it. `Tab` takes the best completion
and then walks forward, `Shift+Tab` walks backward, and `Enter` applies the
query. Field names, operators, and values each have their own color, in the grid
and in the prompt, and an unknown field name is marked as you type it. The rows
match by the same rule the grid ranks with, so a term that the tray offered
always selects rows. Every valid parse applies live; an invalid one explains the
expected syntax on its own row while the last valid rows stay on screen. `Esc`
restores the filter, stable selection, and scroll.

View history is browser-style and bounded to 100 frames. New navigation after
moving backward discards the forward branch. Frames restore stable identities,
filters, sorts, focus, and Services sections against current data; they never
restore forms, tasks, adapters, or closed one-time secrets.

## Filtering and sorting

Free text matches the primary visible identity fields, one field at a time.
Structured filters use `field:value`, comparisons, and comma-separated OR
values:

```text
owner:alice@example.com tag:server online:true last-seen:<7d os:linux
```

- Separate terms are ANDed.
- Comma-separated values within one field are ORed.
- Quoted values permit spaces.
- `!field:value` negates a term.
- A named free-text field matches on substring, so `name:build` finds
  `build-01` without the full value, and `os:ios` cannot reach `windows`. Fields
  with a fixed vocabulary, such as `online` and `path`, stay exact because the
  parser already pins them to a declared value.
- A bare word has no field to aim at, so it matches fuzzily instead: `bld` finds
  `build-01`. It searches each identity field on its own, so a fuzzy match never
  spans two unrelated values.
- `field:starts_with=text` narrows a substring to a prefix. There is no
  `contains=`; a bare term already means exactly that.
- Every field has exactly one spelling. There are no aliases and no hidden
  alternates, so what `/` offers is what the parser accepts.
- Each route declares its own fields. A field that route does not declare
  neither completes nor parses there.
- Invalid terms remain editable and show the expected syntax; they never
  silently become free text, and they never discard the last valid result.
- The view header always shows the visible and total row counts.
- Filtering is local over the current snapshot unless the UI labels a query as
  server-side.
- Sort is stable, with the resource's opaque ID as the final tie-breaker.

The filter language covers exactly the fields each view declares. There is no
generic expression engine.

## Action flow and safety

All actions use the same sequence:

1. Select one or more resources.
2. Press `a`, then the registered mnemonic; unavailable actions include a reason.
3. Enter parameters in a typed form.
4. Review a preview containing target, source, requested change, and impact.
5. Confirm according to risk.
6. Observe progress in a task row without freezing the UI.
7. Re-fetch the affected resource and show the verified result or mismatch.

Risk tiers:

| Tier | Examples | Confirmation |
| --- | --- | --- |
| 0: observe | refresh, copy, filter, ping, validate | none |
| 1: reversible | select exit node, toggle shields-up | review plus `Enter` |
| 2: disruptive | disconnect local client, suspend user, approve route, enable Funnel | review plus explicit action mnemonic |
| 3: destructive/secret | remove device, delete user, revoke credential, expire key now | type the target name or generated phrase |

Batch actions show every target, partial-failure behavior, and whether the API
will issue separate requests. A failed batch never reports global success.

Secrets returned once, such as auth keys or rotated webhook secrets, use a
dedicated ephemeral result view. They can be copied, cannot be reopened from
history, and are removed from memory when the view closes.

## Source and freshness presentation

Every resource detail has a Sources section. A value may show:

- `local daemon · live · 1s` — LocalAPI snapshot observed one second ago;
- `local daemon · reconnecting · last good 12s` — last-good LocalAPI data is retained;
- `local daemon · permission denied` — the configured endpoint cannot be read;
- `local CLI · unavailable` — process-backed actions are unavailable independently;
- `admin · 18s` — Control API snapshot observed 18 seconds ago;
- `stale · 4m · refresh failed` — last good value retained after failure;
- `not returned` — source succeeded but omitted the optional field;
- `unavailable · scope devices:routes:read` — known capability gap.

The top line names the tailnet, then how Tale is connected, then how current the
data is. A segment with no answer is omitted rather than filled with a
placeholder, and task state appears only while something is running or has
failed. There is no single green "connected" indicator that conflates daemon and
API connectivity.

Route lines describe the snapshot, never the fleet: `Data: up to date · refreshed
8s ago`, `Data: stale · last updated 17m ago`, or `Data unavailable · r to
retry`. "Healthy" is not used for data freshness, because a fresh snapshot says
nothing about whether the devices in it are well.

Device detail names the capabilities a device has — `capabilities  Exit node ·
Subnet router · SSH`, or `None` when it has none. Fields are never rendered as a
row of booleans.

## Core user flows

### First run: local client available

1. Tale connects to the configured LocalAPI endpoint and bootstraps status and preferences.
2. It opens Devices in local mode without asking for credentials.
3. If available, Tale separately discovers `tailscale` for CLI-backed actions.
4. The header says `admin: not configured` and offers `:settings` or the
   action `Add admin profile`; it does not block the peer list.
5. The footer teaches navigation from the current context.

### First run: local CLI missing or daemon unavailable

1. Tale renders a diagnostic state rather than exiting into raw stderr.
2. It distinguishes missing CLI, endpoint permission/transport failure, stopped
   daemon, and logged-out client.
3. If an admin profile exists, admin mode remains available.
4. Remediation is copyable. Tale does not install, start, or elevate anything.

### Add an admin profile

1. From Settings, choose `Add profile`.
2. Enter profile name and tailnet ID (`-` is accepted for a credential-owned
   tailnet).
3. Choose scoped OAuth client or temporary access token.
4. Tale explains the precise scopes needed for read-only or operator use.
5. Secret values are read from a hidden prompt and stored in the OS keyring.
6. Tale validates the credential with a minimal read request, reports identity
   and effective access where exposed, then selects the profile.
7. A failed validation leaves config and keyring unchanged.

### Diagnose a peer connection

1. Filter Devices and select the peer.
2. The inspector shows the last known direct/DERP/peer-relay path.
3. Choose `Probe connection`.
4. A cancellable task streams samples and transitions between path types.
5. The result summarizes loss and latency and suggests `netcheck`, DNS query,
   or policy preview based on observed evidence—not on guesses.
6. The user can copy a redacted diagnostic bundle.

### Change local exit node

1. Open Routes or the local node's actions.
2. Select an eligible exit node; list latency and online state are visible.
3. Choose LAN-access behavior.
4. Review the old and new exit-node settings.
5. Apply, then re-read authoritative LocalAPI status and preferences.
6. If verification differs, show the daemon's actual state and retain the task
   as failed; never leave an optimistic state behind.

### Approve a device or route

1. Open the Overview approval queue or filter the resource list.
2. Inspect owner, creation time, tags, posture, advertiser, and requested CIDRs.
3. Choose approve and review the exact affected resource.
4. Apply using Control API credentials.
5. Re-fetch, display the verified status, and link the resulting audit event
   when it becomes available.

Tailnet Lock signing is a different action and is never offered as “approve.”

### Edit the policy file

1. Open Access and refresh the remote HuJSON source.
2. Choose Edit; Tale writes the exact source to a mode-0600 temporary file and
   suspends the alternate screen for `$VISUAL` or `$EDITOR`.
3. On return, Tale first checks whether the remote source changed.
4. It sends the candidate to validation and preview endpoints and runs declared
   policy tests.
5. The UI displays validation output and a textual diff. Comments and formatting
   are preserved.
6. If the remote changed, applying is blocked until the user reopens or manually
   reconciles the new source. There is no blind overwrite.
7. Apply requires an explicit confirmation and a successful final validation.
8. Tale fetches the saved policy and links its audit diff.

### Create an auth key

1. Choose Credentials → Create auth key.
2. Select tags, reusable/ephemeral/preauthorized properties, and expiry.
3. Review the scope and risk, then create.
4. Show the secret once in an ephemeral view with Copy and Close actions.
5. Closing destroys the displayed value. History stores metadata and result
   status only.

### Run SSH or another interactive child

1. Choose the device action and enter a remote user if required.
2. Tale pauses input/render tasks, restores the terminal, and launches a typed
   child process without a shell.
3. The child owns stdin/stdout until exit.
4. Tale always restores raw mode and the alternate screen, even after signal or
   non-zero exit, then records only redacted argv, timing, and exit status.

## Visual language

- `terminal` is the default and preserves terminal defaults for neutral
  surfaces. `tailscale-dark` and `tailscale-light` remain explicit warm canvas
  choices. All three support truecolor, ANSI-256, ANSI-16, and
  no-color.
- Focus is a stronger consistent border/cursor; selection is a filled or
  reversed current-resource marker. They are separate and remain visible when
  combined.
- Green means verified healthy/success, never pending. Yellow/orange means
  warning, stale, relay, or reversible caution. Red means failure,
  destructive risk, or public exposure. Blue/cyan means focus, navigation,
  information, or local provenance. Purple plus a label means admin/combined
  provenance. Text and stable symbols accompany every color.
- Composition precedence is safety, active focus/cursor, selection,
  danger/public/destructive risk, operational state, source, then base
  text/surface. Lower meanings remain in explicit labels or symbols.
- ASCII is the baseline. Unicode symbols are optional and must preserve column
  alignment. Nerd Font glyphs are never required.
- Animation is limited to a spinner for active tasks and a brief success marker.
  Data changes do not slide, pulse, or reorder without respecting the active
  selection.
- Tables do not wrap rows. Long content is truncated with an ellipsis and fully
  available in the inspector.

## Accessibility and input

- Every mouse operation has a keyboard equivalent.
- Mouse capture is off by default so terminal selection continues to work.
- `NO_COLOR` disables semantic colors but preserves text markers.
- Settings → Appearance previews the three built-ins immediately. `Enter`
  applies for the session, `Esc` restores the exact prior theme, and the sheet
  identifies `ui.theme` as the persistence key.
- Focus order follows visual order.
- Help names actions, not just keys.
- Forms keep invalid input and explain the correction; they do not clear it.
- Time is rendered in the configured local/UTC mode with an exact timestamp in
  details even when a relative age is shown in a table.
