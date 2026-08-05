# Acceptance journey evidence matrix

This matrix is deliberately split between deterministic adapter evidence and
real-environment evidence. A mock/fake pass does not promote a platform,
client, keyring, or terminal to Supported. The real-environment column remains
blocked where the evidence is unavailable on the release host.

| # | Journey | Deterministic evidence | Real-environment evidence |
| ---: | --- | --- | --- |
| 1 | Launch without config and observe a local tailnet | local observer fixtures and mock reducer | Blocked: no installed client |
| 2 | Diagnose direct/relay behavior and copy a redacted report | diagnostic fixtures, redaction canaries, secret-result tests | Blocked: no named terminal/clipboard evidence |
| 3 | Change and verify a local exit node | typed preference/route mutation tests | Blocked: no daemon |
| 4 | Configure/remove private Serve | service command/parser and reducer tests | Blocked: no daemon |
| 5 | Enable/disable Funnel with exposure confirmation | service mutation and confirmation tests | Blocked: no daemon |
| 6 | Add a read-only admin profile and inspect permitted resources | fake Control API, scopes, and read-only tests | Blocked: no real credential |
| 7 | Approve device/route and locate audit events | admin mutation/audit fixtures | Blocked: no real tailnet |
| 8 | Edit ordered DNS and refresh local diagnosis | DNS mutation and local preference fixtures | Blocked: no daemon/API |
| 9 | Suspend/restore a user | fake admin mutation contract | Blocked: no real credential |
| 10 | Edit, fail, repair, preview, save, and audit policy | policy workflow, diff, validation, and audit fixtures | Blocked: no real tailnet |
| 11 | Create an auth key, copy once, and prove close | one-time secret and clipboard contract tests | Blocked: no isolated real keyring |
| 12 | Investigate a fleet finding and export filtered evidence | health, filter, export, and 50k-row tests | Blocked: no real tailnet |
| 13 | Lose local daemon while admin remains usable | resource isolation and fake admin tests | Blocked: no daemon/API pair |
| 14 | Lose API authentication while local remains usable | auth classification and local isolation tests | Blocked: no real credential |
| 15 | Cancel process/HTTP/CPU/editor/stream tasks and exit intact | process, flow cancellation, handoff, runtime, and terminal tests | Blocked: no named terminal matrix |
| 16 | Complete `:devices owner:alice online:true`, back, and forward | `command_filter_and_browser_history_restore_and_branch`; rendered prompt cells | Not required: process-local mock interaction |
| 17 | Apply a live filter, retain last-valid rows, cancel, then commit | `filter_invalid_last_good_and_escape_restore_the_full_point`; filter parser tests | Not required: process-local mock interaction |
| 18 | Invoke direct, disabled, nested, and confirmed `a` actions | transient registry/reducer tests and existing confirmation suites | Blocked: no real mutation target; mock dispatch is isolated |
| 19 | Copy a field with `y` and acknowledge only its label | typed clipboard effect, mock isolation, security scan | Blocked: no named real clipboard evidence |
| 20 | Open, filter, resize, scroll, and close contextual help | rendered-buffer tests at four viewports and shell reducer tests | Not required: process-local mock interaction |
| 21 | Restore history after selected resource removal | stable-ID reconciliation and missing-selection notice tests | Not required: deterministic snapshot replacement |
| 22 | Navigate after back and prove the forward branch is discarded | `command_filter_and_browser_history_restore_and_branch` | Not required: process-local mock interaction |
| 23 | Quit immediately when safe and confirm with active tasks | `quit_and_ctrl_c_follow_task_rules` and terminal restoration tests | Blocked: no additional named real terminal evidence |
| 24 | Use mouse footer/completion/transient/cancellation parity | mouse and bottom-buffer UI tests; mouse remains opt-in | Blocked: no named real terminal mouse evidence |

Automated rows are run with fictional data and no operator credentials. The
remaining real-environment evidence is a release blocker and is not converted
to a pass by this file.
