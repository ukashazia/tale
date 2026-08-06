# Theme token ledger

Reviewed 2026-08-05. Numeric provenance is Decision 0005. `D` means directly
observed in the pinned public Tailscale CSS artifact; `T` means a Tale semantic
mapping. ANSI-256 values are static reviewed xterm indices. ANSI-16 names do not
assert emulator RGB values.

The dark and light columns define `tailscale-dark` and `tailscale-light`;
`terminal` uses Reset for neutral tokens and the same capability-appropriate
semantic accents.

## Primitive palette and measured contrast

| Token | Dark RGB / 256 / ANSI-16 | Light RGB / 256 / ANSI-16 | Source | Measured contrast on principal surface |
| --- | --- | --- | --- | --- |
| canvas | `#181717` / 234 / black | `#F9F7F6` / 255 / white | D | primary 16.75 both |
| surface | `#232222` / 235 / black | `#FFFFFF` / 231 / white | D | primary 14.86 / 17.89 |
| raised | `#2E2D2D` / 236 / dark-gray | `#FFFFFF` / 231 / white | D | primary 12.86 / 17.89 |
| inset | `#181717` / 234 / black | `#EEEBEA` / 255 / gray | D | primary 16.75 / 15.09 |
| backdrop | `#181717` / 234 / black | `#DAD6D5` / 252 / gray | D | structural, dimmed |
| primary | `#F9F7F6` / 255 / white | `#181717` / 234 / black | D | 12.86 minimum / 15.09 minimum |
| muted | `#AFACAB` / 145 / gray | `#706E6D` / 242 / dark-gray | D | 6.09 minimum / 4.75 minimum on used surfaces |
| disabled | `#706E6D` / 242 / dark-gray | `#AFACAB` / 145 / gray | D | exception; explicit disabled signal required |
| border-subtle | `#2E2D2D` / 236 / dark-gray | `#EEEBEA` / 255 / gray | D | paired with border glyph |
| border-normal | `#585757` / 240 / gray | `#DAD6D5` / 252 / dark-gray | T/D | paired with border glyph |
| focus | `#ADC7FC` / 153 / cyan | `#3F5DB3` / 61 / blue | D | 9.33 / 6.13 |
| focus-strong | `#85AAF5` / 111 / light-blue | `#324994` / 60 / blue | D | selected fill plus bold identity |
| healthy | `#85D996` / 114 / green | `#09825D` / 29 / green | D | 9.34 / 4.81 |
| info/local | `#ADC7FC` / 153 / cyan | `#4B70CC` / 68 / blue | D | 9.33 / 4.44; underlined label on light |
| admin/combined | `#E3C3FA` / 183 / magenta | `#8052A1` / 97 / magenta | D | 10.17 / 5.77 |
| warning | `#EFC078` / 222 / yellow | `#BB5504` / 130 / yellow | D | 9.43 / 4.76 |
| danger/public | `#FFB1AB` / 217 / red | `#B22D30` / 124 / red | D/T | 9.17 / 6.34 |

The 4.44 light info value is not used as unadorned small body text: the role is
underlined or bold and accompanied by `i`, `local`, or another explicit label.
Light muted on inset is 4.28 and is prohibited; inset instructions use primary.

## Exhaustive semantic-role ledger

Every role below resolves in all twelve theme/capability combinations. `fg`
means the primitive above; surface roles set foreground and background. The
no-color column is the guaranteed modifier/symbol or label, never color alone.

| Roles | Purpose and truecolor token | ANSI-256 | ANSI-16 + modifier | No-color signal |
| --- | --- | --- | --- | --- |
| Canvas | application background, primary/canvas | 255/234 | default/black or white | Reset fg/bg |
| Surface | collections and inspectors, primary/surface | 255/235 or 234/231 | default neutral | border/title label |
| SurfaceRaised | sheets and modals, primary/raised | 255/236 or 234/231 | dark-gray/white | bold title + border |
| SurfaceInset | inputs, code, diffs, primary/inset | 255/234 or 234/255 | neutral + italic context | inset border/italic |
| Backdrop | subordinate underlying content | 145/234 or 242/252 | dark-gray + dim | dim plus modal border |
| BorderSubtle, Divider | low hierarchy structure | 236 or 255 | dark-gray/gray | border glyph + dim |
| BorderNormal | ordinary pane boundary | 240 or 252 | gray/dark-gray | border glyph |
| BorderFocused | input-receiving pane | 153 or 61 | cyan/blue + bold | bold border |
| BorderDanger | destructive boundary | 217 or 124 | red + bold | bold reversed border/title |
| TextPrimary | required body text | primary | white/black | normal text |
| TextMuted | secondary text | 145 or 242 | gray/dark-gray | dim plus context |
| TextDisabled | unavailable text | 242 or 145 | dark-gray/gray + dim | crossed-out + disabled label |
| TextInverse | filled-control identity | canvas | black/white | reverse |
| TextLink | navigable text | 153 or 68 | cyan/blue + underline | underline |
| TextCode | commands/code | 153 or 61 | cyan/blue + italic | italic/code delimiters |
| KeyHint | available key | 153 or 61 | cyan/blue + bold | underline + key text |
| KeyHintDisabled | unavailable key | 242 or 145 | gray + dim/crossed | crossed-out + reason |
| Prompt | active editor text | primary on raised surface | neutral + bold | bold prefix |
| CompletionMatch | matched candidate | focus | cyan/blue + underline | underline |
| CompletionSelected | candidate selection | primary/focus fill | cyan/blue + bold | reverse + `>` |
| Selection | current resource | primary/focus-strong fill | blue + bold | reverse + row marker |
| SelectionInactive | retained resource outside focus | primary/raised | neutral + underline | underline + marker |
| Focus | active control/pane | focus | cyan/blue + bold | bold border/cursor |
| StateHealthy | verified good | healthy | 114/29 | green + bold | `✓` / `+`, healthy |
| StateInfo | informational | info | 153/68 | cyan/blue + underline | `i`, info |
| StateWarning | caution | warning | 222/130 | yellow + bold | `▲` / `!`, warning |
| StateDanger | failure/danger | danger | 217/124 | red + bold | `◆` / `X`, danger |
| StatePending | requested/running, never green | focus | 153/61 | blue + italic | `◌` / `~`, pending |
| StateDisabled | unavailable state | disabled | 242/145 | dark-gray + crossed | `○` / `-`, disabled |
| StateUnknown | not established | disabled | 242/145 | gray + crossed | `?`, unknown |
| StateStale | retained old data | warning | 222/130 | yellow + bold | `▲` / `!`, stale + age |
| StatePublic | public exposure risk | danger | 217/124 | red + bold | `◆` / `X`, public |
| StateDirect | direct path | info | 153/68 | cyan/blue + underline | `i`, direct |
| StateRelay | relayed path | warning | 222/130 | yellow + bold | `▲` / `!`, relay |
| StateOffline | offline resource | disabled | 242/145 | gray + crossed | `○` / `-`, offline |
| SourceLocal | local provenance, not health | info | 153/68 | cyan/blue + underline | `i`, local |
| SourceAdmin | admin provenance, not health | admin | 183/97 | magenta + italic | `A`, admin |
| SourceCombined | composed provenance | admin | 183/97 | magenta + bold/italic | `L+A`, local+admin |
| RiskObserve | read-only action | info | 153/68 | blue + underline | `O`, observe |
| RiskReversible | reversible mutation | warning | 222/130 | yellow + underline | `!`, reversible |
| RiskDisruptive | disruptive mutation | warning | 222/130 | yellow + bold/underline | `!`, disruptive |
| RiskDestructive | destructive/secret mutation | danger | 217/124 | red + bold/reverse | `X`, destructive phrase |
| TaskQueued | waiting task | focus | 153/61 | blue + italic | `~`, queued |
| TaskRunning | executing task | focus | 153/61 | blue + italic | `~`, running |
| TaskSucceeded | verified completion | healthy | 114/29 | green + bold | `+`, succeeded |
| TaskFailed | failed task | danger | 217/124 | red + bold | `X`, failed |
| TaskCancelled | cancelled task | disabled | 242/145 | gray + crossed | `-`, cancelled |
| DiffAdded | addition | healthy | 114/29 | green + bold | `+`, added |
| DiffRemoved | removal | danger | 217/124 | red + bold | `-`/`X`, removed |
| DiffChanged | changed line | warning | 222/130 | yellow + underline | `!`, changed |
| Secret | legitimately visible one-time value | warning | 222/130 | yellow + bold | `*`, secret label |
| Redacted | fixed redaction placeholder | muted | 145/242 | gray + crossed | fixed `###`, redacted |

## Composition review

The typed slots are `base → source → state → risk → selection → focus →
safety`. A selected offline admin device therefore keeps selection fill while
the row still contains `○ offline` and `admin`; a focused destructive prompt
keeps danger wording and border while the cursor remains reversed; redaction is
always last and fixed-length.
