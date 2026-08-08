# Tale design principles

How Tale looks and behaves. This is a living record: when a decision is made or
a flaw is found, it is written down here so the next screen does not repeat it.

Rules here are about the surface. Engineering rules live in `AGENTS.md`.

---

## 1. The surface is not the model

The most common defect in this codebase has been internal vocabulary leaking
into the interface. Every one of these shipped at some point and every one was
wrong:

| Was | Is |
| --- | --- |
| `source:healthy` | `data stale · last updated 17m ago` |
| `exit=false option=false ssh=false shared=false` | `None`, or the names of what is enabled |
| `status: loading` on a resource nobody requested | `No mappings loaded yet` |
| `client not returned` | the row is absent |
| `seen 1753999997` | `3m` |
| `direct argv (each line is one argument): argv[0] = "file"` | `Save incoming files into /tmp.` |
| `target=<exact discovered target>;files=/a\|/b` | a form with a `Files` field |

Concretely:

- **Never render an enum's `Debug` or its wire name.** If a state is worth
  showing, it is worth a sentence.
- **A field the source did not send is absent, not `"not returned"`.** Model it
  as `Option` and skip the row. A sentinel string is a lie that sorts, filters
  and copies like data.
- **Never expose a serialization format for input.** No `key=value;key=value`,
  no `|`-separated lists, no "enter a typed request". If it has fields, it is a
  form.
- **Distinguish "not asked" from "asked, still waiting".** `Idle` and `Loading`
  are different facts and the user acts differently on each.

## 2. Say what happened and what to do next

An empty box is a dead end. Every empty state names the reason and the key:

```
No mappings loaded yet

  load                   r
```

Errors explain the expected shape without discarding the last good result — a
bad filter term leaves the previous rows on screen while you repair it.

Before anything changes, the confirmation states, in this order: how risky it
is, the warning if there is one, **What will happen** in a sentence, the exact
**Command**, then the phrase to type. Risk comes from the *request*, not the
action: receiving files is reversible until the conflict rule says overwrite.

Benign actions get no warning at all rather than a restatement of themselves.

## 3. One grammar for keys

- **Direct keys act immediately.** `j`, `k`, `r`, `a`, `y`, `/`, `s`.
- **A key that opens a chooser opens the same kind of chooser everywhere.**
  Bottom-anchored, grouped, direct keys, no row cursor. The action menu (`a`) is
  the reference; sort, copy, theme and account pickers all follow it.
- **Two-key sequences drill down.** Pressing the first key *replaces* the menu
  with the second level — it does not dim a flat grid. `c` then `t`/`p`;
  `n` then `a`/`d`.
- **Tabs are `Tab` and `Shift-Tab`**, they wrap, and they live *inside* the
  pane's border. A tab strip belongs to the pane it switches, not to the app.
- **Never print key hints inside a widget.** The footer and `?` own that. Two
  copies drift, and the in-box copy is the one that ends up lying: the services
  bar said `[/] section` for months while the real keys were `H`/`L`.
- **A key that shows a pane also hides it.** `i` brings the inspector in and
  takes it away again on `:devices` and `:users` alike; it is not `Enter`, which
  replaces the table with a full-width detail view. Showing and hiding are one
  key, not two states of a key that only ever opens. The pane starts closed: the
  table is what the route is for, and the inspector repeats a row already on
  screen.
- **A key is offered only where it does something.** `applies_to_route` is the
  single filter for both the footer and contextual help. `w columns` must not
  appear on a screen with no columns; the route's own key must not be sorted
  past `? more`.

### Forms

Two modes, one rule: **Enter acts on what is selected.**

| Mode | Keys |
| --- | --- |
| Browsing | `j`/`k` move · `Enter` opens the field · `Esc` closes the form |
| Editing | type or `←`/`→` · `Enter` keeps · `Esc` restores the previous value |

The row past the last field is **Continue**, which submits. That keeps `Enter`
meaning one thing rather than needing a second submit key.

A form asks only for what it cannot already know. The selected row is the
target; the machine is the machine. Anything else is stated above the fields,
not typed into them. Fields Tailscale treats as identity — a mapping's listener
and path — are stated too, because editing them can only produce an error.

Each field has a label in words, a one-line explanation shown while selected,
and a type (text, choice, toggle) that decides how it is edited.

## 4. Components, not copies

There are three shared shapes and everything uses them. A view that hand-rolls
one of these is a bug, not a style choice — the second copy is where the drift
starts.

**`components::panel`** — every bordered box. One place decides that titles are
padded, that content never touches the border, and that a focusable pane's
border says whether keys land there. This exists because `┌inspector─` and
`┌ devices ─` shipped side by side.

**`components::grid`** — every list. A heading row, one line per row, the
selection carrying the row style end to end. Views differ in their *columns*,
never in how a list looks. There is no `>` marker: the highlight is the
selection. Fixed columns are honoured first; the rest share what is left by
weight, so a narrow terminal shrinks flexible columns instead of dropping any.
A cell keeps its own role only when it means something the row does not — a
liveness glyph — otherwise it inherits the row.

Column sets are declared once, as an ordered list with a predicate for when
each column appears. The device table previously carried five parallel header
lists and five parallel width lists that had to be kept in step by hand.

**`components::grid::detail`** — every label/value pane. One thing described,
rather than many things listed.

Column headings use `TextPrimary`. Do not reach for `SectionHeading` — in the
default theme it is a highlight, and the heading reads as a second selected
row.

Route context lives in the border title, not in a separate row:

```
┌ mappings · 2 of 5 · 2 public · /exposure:public · port ↓ ─────┐
```

Counts come from the view that owns them, so a view without a collection shows
none.

## 5. Show what matters, when it matters

The header is a wordmark, the connection state as a chip, the tailnet, and
versions — spaced apart, not packed. It collapses to one line below 26 rows.

Freshness is silent while data is current. `tailscale:` appears only when a
version was read. Tasks appear only while running or failed. A hint appears
beside the state only when a key acts on it: `(press r to retry)`.

Every border title is padded: `┌ inspector ─`, not `┌inspector─`.

## 6. Model the domain as it is, not as the CLI spells it

Serve and Funnel were two tabs because the CLI has two commands. They read the
same configuration, partitioned by `AllowFunnel` — so they are one table with an
exposure column. That single change gave `/`, `s` and `y` something to act on,
for free.

The test: if `/`, `s` or `y` has nothing to bind to on a screen, the screen
probably has no collection — differently-shaped blobs sharing chrome. Fix the
model, not the keybinding.

Conversely, things that are not services do not belong on the services route.
Metrics and bug reports are evidence about this machine; they live under
`:diagnostics`. Taildrop's rows were the tailnet's devices listed a second time,
under different column headings and with no filter, sort or copy — so it is an
action on `:devices`, where the selected row is already the target.

The same test again: a tab whose rows duplicate another route's collection is
not a section, it is that collection with one extra verb.

The inverse also happens. `:activity` meant two subjects — what this client
did, and what the tailnet was told — and so was neither. Its rows were a
`List` of `* succeeded  task-3 laptop-0`, and its right pane concatenated task
detail, an audit summary, audit events, flow logs, log streams and webhooks
into one `format!`. Nothing in it could be sorted, filtered or copied, because
there was no collection to bind to. Split by subject: `:tasks` is a grid of
runs with an inspector, `:audit` is the tailnet's log with the delivery
mechanisms beside it. Each then answers one question, which is the only way a
border title can honestly say how much of it is showing.

A route named after a category rather than a subject is the warning sign.
`activity` covers anything that happened; so does `stuff`.

**An action belongs to the route that shows its subject.** The local client's
verbs were one list handed to every route that had none of its own, so
`:credentials` offered `remove local account` and `open tailscale ssh` — the
second of which acts on whichever row `:devices` had selected, out of sight.
Split by subject instead: the machine's verbs on `:local`, the ones that act on
the selected row on `:devices`, the summary's on `:diagnostics`. A route with
nothing of its own then offers nothing of its own, which is the honest answer.
A menu is not a place to park a verb that fits nowhere else.

## 7. Public exposure is the one dangerous state

It gets the `StatePublic` role in the table, a sentence in the inspector
(`Reachable from the public internet`), a `Disruptive` risk, and a typed
confirmation phrase. Nothing else on the services route is styled for danger,
so the styling means something.

## 8. Colour is semantic

No colour literals outside `ui/theme`. Views request a `StyleRole`; the theme
decides. New meaning means a new role, not a new hex value.

## 9. Mock mode is a first-class surface

`--mock` must render every route with plausible data. A route that is blank
under `--mock` cannot be demonstrated, screenshotted, or snapshot-tested — which
is exactly how the services route went unreviewed.

## 10. Verify in the running app

Reading the source is not evidence. Changes to rendering, key handling or
cursor behaviour are checked by driving the real binary in a PTY, or by
rendering through `TestBackend` and reading the buffer. Two shipped bugs — the
cursor blink and the dead `[/]` hint — survived review by reasoning about code
that did not do what it appeared to.

Hand-maintained lookup tables that fall through to a placeholder are a standing
trap. `Binding::label()` silently rendered unlisted keys as the literal `"key"`;
it is now an index over printable ASCII, with a test asserting no registered
binding renders as a placeholder. Prefer a total mapping over a match with a
default whenever the default is wrong rather than merely unspecified.

---

## Open

- ~20 admin and operator actions still use the `key=value;key=value` operator
  form. `FormField` replaces it; `render_operator`'s 190 lines of hint strings
  go away with it.
- Dense views (Local, Settings) do not scroll and lose their last rows on short
  terminals.
- Overview is not yet a home screen.
- Authentication happens outside the TUI, which breaks first run.
