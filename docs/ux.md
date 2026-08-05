# Interaction and user flows

## Information architecture

Tale is resource-oriented. A bottom `:` command line provides direct navigation; the UI
does not spend permanent width on a large sidebar.

Canonical routes and aliases:

| Route | Aliases | Default content |
| --- | --- | --- |
| `overview` | `ov`, `home` | health and actionable queues |
| `local` | `self` | local node and preferences |
| `devices` | `device`, `dev`, `nodes` | device inventory |
| `users` | `user` | member inventory |
| `routes` | `route`, `rt` | subnet and exit routes |
| `dns` | — | tailnet DNS and query tool |
| `access` | `policy`, `acl`, `grants` | policy source and tests |
| `services` | `service`, `serve`, `funnel` | local and tailnet services |
| `credentials` | `keys`, `auth` | supported credentials |
| `activity` | `logs`, `tasks`, `events` | tasks and audit/network logs |
| `settings` | `config` | Tale and supported tailnet settings |

Typing `:devices owner:alice online:true` navigates and applies the trailing
filter. Commands select known routes and parameters; they are not a shell.

## Frame layout

Wide terminals use a collection-and-inspector layout:

```text
┌ Tale · ops · example.com ─ local: running ─ admin: 12s ─ RO: off ┐
│ Devices  24/31       / owner:alice online:true      sort:lastSeen │
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
- Command, filter, transient, completion, and help surfaces grow upward from
  the final terminal row. Only alerts and confirmations are centered modals.

The terminal's existing background is the default surface. Borders communicate
focus, not decoration. Color is semantic and never the only state signal.

## Navigation contract

### Global bindings

| Key | Action |
| --- | --- |
| `:` | open the inline route command line |
| `/` | edit the active view's filter inline with live valid results |
| `?` | open contextual bottom help; `/` filters help labels and keys |
| `Tab` / `Shift+Tab` | complete or cycle in command/filter editors |
| `Esc` | cancel only the active interaction; no-op in normal mode |
| `[` / `]` | restore the previous or next view-history frame |
| `r` | refresh the active resource |
| `R` | refresh every source used by the active route |
| `@` | open task and command history |
| `q` | quit when safe; active tasks retain their confirmation boundary |
| `Ctrl+c` | first press cancels the focused task/input; second press while idle quits |

### Collection bindings

| Key | Action |
| --- | --- |
| `j` / `k`, arrows | move selection |
| `g` / `G` | first or last row |
| `Ctrl+d` / `Ctrl+u` | half-page down or up |
| `Enter` / `l` | open selected resource details |
| `h` | return from details or focus the collection |
| `Space` | toggle multi-selection when the current action supports a batch |
| `s` | choose sort field and direction |
| `w` | toggle standard and wide columns |
| `a` | open a direct contextual key menu; the next registered key invokes an action |
| `y` | open a direct copy-key menu; acknowledgement names only the field |
| `H` / `L` | previous/next sibling section in Services |

Direct destructive bindings are forbidden. Destructive operations live behind
the `a` prefix and still require their typed confirmation.

### Contextual help

The footer, transient menus, and bottom help sheet are generated from the action
registry. When space is insufficient the footer ends with `? more`; it never
truncates a key while leaving a misleading label. Disabled actions remain
visible with their capability reason. Transients do not use row selection,
arrows, or a timeout; `Esc` cancels and an unknown key leaves the menu open.

The `:` editor supports Unicode-safe cursor movement, Home/End, adjacent scalar
deletion, `Ctrl+w/u/k`, bounded successful-command history, and schema-aware
Tab/Shift+Tab completion. The `/` editor applies every valid parse live, keeps
the last valid rows during an invalid edit, and restores filter, stable
selection, and scroll on `Esc`.

View history is browser-style and bounded to 100 frames. New navigation after
moving backward discards the forward branch. Frames restore stable identities,
filters, sorts, focus, and Services sections against current data; they never
restore forms, tasks, adapters, or closed one-time secrets.

## Filtering and sorting

Free text matches the primary visible identity fields. Structured filters use
`field:value`, comparisons, and comma-separated OR values:

```text
owner:alice@example.com,tag:server online:true lastSeen:<7d os:linux
```

- Separate terms are ANDed.
- Comma-separated values within one field are ORed.
- Quoted values permit spaces.
- `!field:value` negates a term.
- Invalid terms remain editable and show an inline error; they never silently
  become free text.
- The view header always shows the visible and total row counts.
- Filtering is local over the current snapshot unless the UI labels a query as
  server-side.
- Sort is stable, with the resource's opaque ID as the final tie-breaker.

The initial filter language is implemented only for fields used by core views.
There is no generic expression engine.

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

The overview header reports source health separately. There is no single green
“connected” indicator that conflates daemon and API connectivity.

## Core user flows

### First run: local client available

1. Tale connects to the configured LocalAPI endpoint and bootstraps status and preferences.
2. It opens Overview in local mode without asking for credentials.
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

- Default background is `Reset`; Tale does not paint a full opaque canvas.
- Focus uses a stronger border and title, selection uses reverse video or bold,
  and active input has a visible cursor.
- Green means verified healthy/success, yellow means attention/stale, red means
  failed/destructive, blue/cyan means informational or selected. Text or symbols
  accompany every color.
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
- Focus order follows visual order.
- Help names actions, not just keys.
- Forms keep invalid input and explain the correction; they do not clear it.
- Time is rendered in the configured local/UTC mode with an exact timestamp in
  details even when a relative age is shown in a table.
