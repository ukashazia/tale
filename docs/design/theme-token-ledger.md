# Theme token ledger

Reviewed 2026-08-08. Numeric provenance is Decision 0005. `B` means a literal
value from the Tailscale Brand Toolkit (October 2025); `T` means a Tale semantic
mapping, used only where the toolkit publishes no step. ANSI-256 values are
static reviewed xterm indices. ANSI-16 names do not assert emulator RGB values.

The dark and light columns define `tailscale-dark` and `tailscale-light`;
`terminal` uses Reset for neutral tokens and the same capability-appropriate
semantic accents.

## Primitive palette and measured contrast

| Token | Dark RGB / 256 / ANSI-16 | Light RGB / 256 / ANSI-16 | Source | Measured contrast on principal surface |
| --- | --- | --- | --- | --- |
| canvas | `#1F1E1E` / 234 / black | `#FAF9F8` / 255 / white | B Gray900 / Gray0 | primary 15.82 both |
| surface | `#2A2929` / 235 / black | `#FFFFFF` / 231 / white | T / T | primary 13.80 / 16.63 |
| raised | `#353434` / 236 / dark-gray | `#FFFFFF` / 231 / white | T / T | primary 11.80 / 16.63 |
| inset | `#1F1E1E` / 234 / black | `#EEEBEA` / 255 / gray | B Gray900 / Gray200 | primary 15.82 / 14.02 |
| backdrop | `#1F1E1E` / 234 / black | `#DAD6D5` / 188 / gray | B Gray900 / Gray300 | structural, dimmed |
| primary | `#FAF9F8` / 231 / white | `#1F1E1E` / 234 / black | B Gray0 / Gray900 | 11.80 minimum / 14.02 minimum |
| muted | `#AFACAB` / 145 / gray | `#706E6D` / 242 / dark-gray | B Gray400 / Gray500 | 5.50 minimum / 4.82 minimum on used surfaces |
| disabled | `#706E6D` / 242 / dark-gray | `#AFACAB` / 145 / gray | B Gray500 / Gray400 | exception; explicit disabled signal required |
| border-subtle | `#353434` / 236 / dark-gray | `#EEEBEA` / 255 / gray | T / B Gray200 | paired with border glyph |
| border-normal | `#AFACAB` / 145 / gray | `#706E6D` / 242 / dark-gray | B Gray400 / Gray500 | 6.43 / 5.07 on panel surface |
| selection-ink | `#1F1E1E` / 234 / black | `#1F1E1E` / 234 / black | B Gray900 | 4.50 on the selection fill, both themes |
| focus | `#85AAF5` / 111 / light-blue | `#3F5DB3` / 61 / blue | B Blue200 / Blue600 | 6.26 / 6.13 |
| focus-strong | `#5A82DE` / 68 / blue | `#5A82DE` / 68 / light-blue | B Blue400 | selection fill; 4.50 against its ink |
| healthy | `#33C27F` / 78 / green | `#09825D` / 29 / green | B Green200 / Green400 | 6.33 / 4.81 |
| info/local | `#85AAF5` / 111 / light-blue | `#4B70CC` / 68 / blue | B Blue200 / Blue500 | 6.26 / 4.68 |
| admin/combined | `#BE8FE1` / 140 / magenta | `#8052A1` / 97 / magenta | B Purple200 / Purple500 | 5.67 / 5.77 |
| warning | `#E5993E` / 215 / yellow | `#BB5504` / 130 / yellow | B Orange200 / Orange400 | 6.19 / 4.76 |
| danger/public | `#F68F87` / 210 / red | `#B22D30` / 88 / red | B Red200 / Red500 | 6.35 / 6.34 |

Light info measures 4.45 against canvas, below the gate, so the role is never
unadorned small body text: it is underlined or bold and accompanied by `i`,
`local`, or another explicit label. Light muted on inset is 4.28 and is
prohibited; inset instructions use primary.

Two ANSI-256 indices are deliberately not the nearest match, as Decision 0005
permits where hierarchy would otherwise collapse. Light canvas takes 255 rather
than 231 so it stays under surface; light info takes 68 rather than 61, which
light focus already holds.

## Deviation from the toolkit's recommended accent tier

The toolkit directs that the 400-range values be used as the primary accent
colors. Tale follows that for the selection fill, for light healthy, and for
light warning. It cannot follow it for the remaining accent roles, because the
400 tier is sized for large display type and fills, and every Tale accent role
is one-cell foreground text held to 4.5:1:

| 400 value | On light canvas | On dark canvas |
| --- | --- | --- |
| Blue400 `#5A82DE` | 3.51 fails | 4.50 passes |
| Green400 `#09825D` | 4.58 passes | 3.46 fails |
| Red400 `#D04841` | 4.26 fails | 3.71 fails |
| Orange400 `#BB5504` | 4.52 passes | 3.50 fails |
| Purple400 `#995FC3` | 4.18 fails | 3.78 fails |

No hue clears the gate on both backgrounds. Every one of them clears 3:1, the
large-text gate, which is the tier the toolkit is written for. Tale therefore
takes the 200 tier on dark and the 400–600 tier on light, choosing per hue the
nearest published step that clears 4.5:1. Blue400 is retained as the selection
fill, where it is a background carrying Gray900 ink rather than foreground text,
and where it is the most prominent branded element in the interface.

## Exhaustive semantic-role ledger

Every role below resolves in all twelve theme/capability combinations. `fg`
means the primitive above; surface roles set foreground and background. The
no-color column is the guaranteed modifier/symbol or label, never color alone.

| Roles | Purpose and truecolor token | ANSI-256 | ANSI-16 + modifier | No-color signal |
| --- | --- | --- | --- | --- |
| Canvas | application background, primary/canvas | 231/234 | default/black or white | Reset fg/bg |
| Surface | collections and inspectors, primary/surface | 231/235 or 234/231 | default neutral | border/title label |
| SurfaceRaised | sheets and modals, primary/raised | 231/236 or 234/231 | dark-gray/white | bold title + border |
| SurfaceInset | inputs, code, diffs, primary/inset | 231/234 or 234/255 | neutral + italic context | inset border/italic |
| Backdrop | subordinate underlying content | 145/234 or 242/188 | dark-gray + dim | dim plus modal border |
| BorderSubtle, Divider | low hierarchy structure | 236 or 255 | dark-gray/gray | border glyph + dim |
| BorderNormal | ordinary pane boundary | 145 or 242 | gray/dark-gray | border glyph |
| BorderFocused | input-receiving pane | 111 or 61 | light-blue/blue + bold | bold border |
| BorderDanger | destructive boundary | 210 or 88 | red + bold | bold reversed border/title |
| TextPrimary | required body text | primary | white/black | normal text |
| TextMuted | secondary text | 145 or 242 | gray/dark-gray | dim plus context |
| TextDisabled | unavailable text | 242 or 145 | dark-gray/gray + dim | crossed-out + disabled label |
| TextInverse | filled-control identity | canvas | black/white | reverse |
| TextLink | navigable text | 111 or 68 | light-blue/blue + underline | underline |
| TextCode | commands/code | 111 or 61 | light-blue/blue + italic | italic/code delimiters |
| KeyHint | available key | 111 or 61 | light-blue/blue + bold | underline + key text |
| KeyHintDisabled | unavailable key | 242 or 145 | gray + dim/crossed | crossed-out + reason |
| Prompt | active editor text | primary on raised surface | neutral + bold | bold prefix |
| CompletionMatch | matched candidate | focus | light-blue/blue + underline | underline |
| CompletionSelected | candidate selection, selection-ink/focus-strong fill | 234/68 | black on blue + bold | reverse + `>` |
| Selection | current resource, selection-ink/focus-strong fill | 234/68 | black on blue + bold | reverse + row marker |
| SelectionInactive | retained resource outside focus | primary/raised | neutral + underline | underline + marker |
| Focus | active control/pane | focus | light-blue/blue + bold | bold border/cursor |
| StateHealthy | verified good | healthy | 78/29 | green + bold | `✓` / `+`, healthy |
| StateInfo | informational | info | 111/68 | light-blue/blue + underline | `i`, info |
| StateWarning | caution | warning | 215/130 | yellow + bold | `▲` / `!`, warning |
| StateDanger | failure/danger | danger | 210/88 | red + bold | `◆` / `X`, danger |
| StatePending | requested/running, never green | focus | 111/61 | blue + italic | `◌` / `~`, pending |
| StateDisabled | unavailable state | disabled | 242/145 | dark-gray + crossed | `○` / `-`, disabled |
| StateUnknown | not established | disabled | 242/145 | gray + crossed | `?`, unknown |
| StateStale | retained old data | warning | 215/130 | yellow + bold | `▲` / `!`, stale + age |
| StatePublic | public exposure risk | danger | 210/88 | red + bold | `◆` / `X`, public |
| StateDirect | direct path | info | 111/68 | light-blue/blue + underline | `i`, direct |
| StateRelay | relayed path | warning | 215/130 | yellow + bold | `▲` / `!`, relay |
| StateOffline | offline resource | disabled | 242/145 | gray + crossed | `○` / `-`, offline |
| SourceLocal | local provenance, not health | info | 111/68 | light-blue/blue + underline | `i`, local |
| SourceAdmin | admin provenance, not health | admin | 140/97 | magenta + italic | `A`, admin |
| SourceCombined | composed provenance | admin | 140/97 | magenta + bold/italic | `L+A`, local+admin |
| RiskObserve | read-only action | info | 111/68 | blue + underline | `O`, observe |
| RiskReversible | reversible mutation | warning | 215/130 | yellow + underline | `!`, reversible |
| RiskDisruptive | disruptive mutation | warning | 215/130 | yellow + bold/underline | `!`, disruptive |
| RiskDestructive | destructive/secret mutation | danger | 210/88 | red + bold/reverse | `X`, destructive phrase |
| TaskQueued | waiting task | focus | 111/61 | blue + italic | `~`, queued |
| TaskRunning | executing task | focus | 111/61 | blue + italic | `~`, running |
| TaskSucceeded | verified completion | healthy | 78/29 | green + bold | `+`, succeeded |
| TaskFailed | failed task | danger | 210/88 | red + bold | `X`, failed |
| TaskCancelled | cancelled task | disabled | 242/145 | gray + crossed | `-`, cancelled |
| DiffAdded | addition | healthy | 78/29 | green + bold | `+`, added |
| DiffRemoved | removal | danger | 210/88 | red + bold | `-`/`X`, removed |
| DiffChanged | changed line | warning | 215/130 | yellow + underline | `!`, changed |
| Secret | legitimately visible one-time value | warning | 215/130 | yellow + bold | `*`, secret label |
| Redacted | fixed redaction placeholder | muted | 145/242 | gray + crossed | fixed `###`, redacted |

## Composition review

The typed slots are `base → source → state → risk → selection → focus →
safety`. A selected offline admin device therefore keeps selection fill while
the row still contains `○ offline` and `admin`; a focused destructive prompt
keeps danger wording and border while the cursor remains reversed; redaction is
always last and fixed-length.
