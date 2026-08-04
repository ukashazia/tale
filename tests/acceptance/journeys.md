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

Automated rows are run with fictional data and no operator credentials. The
remaining real-environment evidence is a release blocker and is not converted
to a pass by this file.
