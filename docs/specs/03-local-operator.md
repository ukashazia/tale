# Specification 03 — Local operator

- Implementation phase: 3
- JJ change description: `feat: add local Tailscale controls`
- Depends on: Specifications 01 and 02 complete
- Produces: safe control of the Tailscale node running Tale

This specification never authorizes controlling another device's local daemon.
Admin-plane approval and remote execution remain outside this phase.

Command shapes were checked against the installed Tailscale 1.98.9 CLI on
2026-08-03. Account commands are alpha in that client and must be capability-
gated and fixture-versioned.

## 03.0 Phase contract

### User-visible result

The user can:

- connect or disconnect the current node;
- view and change supported local preferences;
- choose/clear an exit node and LAN-access behavior;
- advertise an exit node, subnet routes, app connector, and supported peer
  relay settings;
- list, switch, add, log out, and remove local accounts where supported;
- open Tailscale SSH and `nc` sessions to a selected device;
- inspect and reload system policy where supported.

Every mutation is typed, previewed, confirmed according to risk, executed once,
then verified from fresh daemon state. Tale never runs sudo.

### Required new module boundaries

- `src/domain/preference.rs`
- `src/domain/route.rs`
- `src/domain/account.rs`
- `src/domain/mutation.rs`
- `src/local/preferences.rs`
- `src/local/accounts.rs`
- `src/local/handoff.rs`
- `src/ui/views/routes.rs`
- `src/ui/components/form.rs`
- `src/ui/components/confirm.rs`
- `docs/decisions/0001-local-preferences-transport.md`

Extend the existing local client/process modules; do not create a generic
mutation service or remote-execution abstraction.

## 03.1 Mutation lifecycle

### Required state

```text
Mutation<TTarget, TRequest>
  id
  action_id
  target
  base_observation
  request
  risk
  state

MutationState
  Editing
  Preview
  AwaitingConfirmation
  Running
  Verifying
  Succeeded
  Failed
  CancelledBeforeDispatch
  VerificationMismatch
```

After dispatch, cancellation may stop waiting but must not claim the command did
not run. A process killed after dispatch yields `OutcomeUnknown` until a fresh
read determines actual state.

### Universal flow

1. Capability evaluation.
2. Typed form or parameter selection.
3. Preview generated from verified base state and request.
4. Risk-tier confirmation.
5. Acquire a mutation lock for the local node or affected resource.
6. Dispatch exactly one typed command.
7. Record bounded/redacted process result.
8. Fetch fresh authoritative local state.
9. Compare requested fields only.
10. Report verified success, command failure, read failure, or mismatch.

Do not optimistically replace verified preference values. The UI may show
`requested: on` beside `verified: off` while a task runs.

### Risk and confirmation

| Operation | Risk | Confirmation |
| --- | --- | --- |
| ordinary boolean preference | reversible | preview + `Enter` |
| exit-node selection | reversible | preview + `Enter` |
| route advertisement | reversible | preview complete set + `Enter` |
| disconnect | disruptive | explicit mnemonic after impact copy |
| logout | disruptive | explicit mnemonic after key invalidation copy |
| remove local account | destructive | type account display name or ID |
| accept lose-SSH risk | disruptive | explicit checkbox plus mnemonic |

`--read-only` and `read_only = true` disable mutation at both action resolution
and dispatch. Dispatch rechecks the lock even if the form opened before the mode
changed.

### Task history

Store action ID, redacted target, requested field names, redacted argv, timing,
exit status, verification result, and bounded stderr. Do not store environment
values or authentication URLs containing secrets.

### Tests

- Every valid/invalid state transition.
- Double-confirm or repeated key press dispatches once.
- Read-only mode introduced while a form is open blocks dispatch.
- Cancellation before dispatch versus after dispatch.
- Verification success, mismatch, and read failure.
- Mutation lock prevents conflicting concurrent local changes.

## 03.2 Preference read contract decision

Preference editing must not begin until this feature is complete.

### Decision output

Create `docs/decisions/0001-local-preferences-transport.md` containing:

- Tailscale version(s) and platforms inspected;
- exact current-value fields required by features 03.3–03.6;
- candidate supported surfaces and their maturity;
- selected transport and socket/discovery behavior;
- fixture source and compatibility policy;
- rejected alternatives;
- permission and error behavior.

### Allowed selection

Use one of:

1. documented structured CLI output that exposes the required current values;
2. the individually stable Tailscale LocalAPI `GetPrefs` method, after proving a
   robust transport/discovery path for every platform claimed by this phase.

Do not parse undocumented `tailscale debug prefs`. Do not bind to unrelated
LocalAPI methods. Do not add a fallback chain between old/new output forms.

### Required current values

The selected contract must distinguish at least:

- running/want-running state;
- accept DNS;
- accept routes;
- shields up;
- Tailscale SSH server;
- update check and automatic update when present;
- posture reporting;
- hostname and nickname where present;
- selected exit node and LAN access;
- advertised exit-node state;
- complete advertised route set;
- app-connector state;
- relay-server port/static endpoints where present.

Unknown/not returned and policy-managed/uneditable must not collapse into
`false`.

### Domain model

Represent each preference as:

```text
ObservedPreference<T>
  value: optional T
  editability: editable, policy_managed, permission_denied, unsupported, unknown
  source
  observed_at
```

### Phase blocker

If neither allowed surface provides a reliable contract on a target platform,
the implementation narrows platform support or disables the specific preference
with `unsupported`. It does not adopt debug output as a temporary workaround.

## 03.3 Connect and disconnect

### Invocations

Connect without changing settings:

```text
tailscale up
```

Disconnect:

```text
tailscale down
```

When the user has explicitly accepted loss of the current remote connection,
disconnect may use:

```text
tailscale down --accept-risk=lose-ssh
```

Never pass `--accept-risk=all`. Do not add any settings flags to `tailscale up`;
with flags, `up` requires the complete settings set, while no flags preserves
current preferences.

### Preview

Connect preview shows current state and that existing preferences are preserved.
Disconnect preview shows that Tailscale connectivity will stop and may terminate
the terminal session if Tale itself is being used over Tailscale.

The lose-SSH checkbox is off by default and shown only for disconnect. Tale does
not try to detect with certainty how the terminal is connected.

### Verification

- Connect succeeds only when fresh state becomes Running/Degraded.
- Disconnect succeeds only when fresh state is Stopped/disconnected.
- NeedsLogin after `up` opens the login flow choice; it is not reported as
  verified connected.
- Timeout or cancellation triggers a fresh status read before final outcome.

### Tests

- Exact argv with and without risk acceptance.
- Connect preserves settings by using no flags.
- Needs-login, CLI error, timeout with actual success, and verification mismatch.

## 03.4 Preference editor

### Invocation rule

Use `tailscale set` and pass only fields changed by the user. Every flag is a
separate argv value. Examples:

```text
tailscale set --accept-dns=true
tailscale set --accept-routes=false
tailscale set --shields-up=true
tailscale set --ssh=false
tailscale set --auto-update=true
tailscale set --update-check=true
tailscale set --report-posture=true
tailscale set --hostname=build-01
tailscale set --nickname=work
```

Do not combine unrelated changes unless the user edited them in the same form.
The preview lists every changed field as `old → new` and shows the redacted exact
argv. Unchanged or unknown fields are omitted from argv.

### Supported fields

| Preference | Form | Validation |
| --- | --- | --- |
| accept DNS | boolean | must be observed/editable |
| accept routes | boolean | must be observed/editable |
| shields up | boolean | warn that inbound connections are blocked |
| SSH server | boolean | capability and policy aware |
| automatic update | boolean | platform/client capability aware |
| update checks | boolean | capability aware |
| posture reporting | boolean | explain management-plane data reporting |
| hostname | text | non-empty, CLI remains authoritative |
| nickname | text | non-empty; account-scoped explanation |
| web client | boolean | explain port 5252 exposure to tailnet |

Operator changes are excluded. Tale reports missing operator permission but does
not offer `--operator` or sudo setup inside the TUI.

### Verification

Read fresh preferences and compare only submitted fields. If the daemon/policy
coerces a value, report `VerificationMismatch` with actual state. Do not retry.

### Tests

- Exact boolean/value argv.
- Multiple changed fields preserve deterministic flag order.
- Unknown current value blocks editing rather than assuming false/empty.
- Hostname containing spaces/shell syntax remains one argv and receives CLI
  validation.
- Policy-managed and unsupported fields are visible but disabled.

## 03.5 Exit-node selection

### Domain model

```text
ExitNodeCandidate
  device_id
  display_name
  tailscale_ips
  online
  path
  last_probe?
  selected

ExitNodeRequest
  selection: none, device, auto_any
  allow_lan_access
```

Candidates come from the local snapshot's exit-node-option data. Do not infer
eligibility from tags or names.

### Invocations

Select by stable display target chosen from candidate data:

```text
tailscale set --exit-node=<dns-name-or-ip> --exit-node-allow-lan-access=<bool>
```

Automatic selection:

```text
tailscale set --exit-node=auto:any --exit-node-allow-lan-access=<bool>
```

Clear:

```text
tailscale set --exit-node= --exit-node-allow-lan-access=false
```

The empty value must be an explicit argv string, not an omitted flag.

### UX

- Sort candidates by online state, latest probe latency, path, then name.
- Missing latency displays `not probed`; it is not sorted as zero.
- Offer a Phase-2 ping action before selection.
- Preview current and requested exit node plus LAN-access behavior.
- Explain that selection changes this local node only.

### Verification

Fresh status/preferences must identify the requested candidate or `auto:any`
state and LAN-access setting. Compare stable ID when available; do not accept a
same-name device heuristic.

### Tests

- Select, auto, clear, offline warning, unknown latency, duplicate display names,
  candidate removed during form, and verification mismatch.

## 03.6 Route, connector, and relay advertisement

### Route form

Represent advertised subnets as parsed `IpNet`-equivalent values, preserving no
user formatting after successful parse. Canonicalize network address/prefix,
deduplicate, sort IPv4 then IPv6 by network/prefix, and show the complete
resulting set.

Actions:

- add CIDR;
- remove selected CIDR;
- replace the complete set;
- clear all;
- toggle exit-node advertisement;
- toggle app-connector advertisement;
- configure supported relay-server port/static endpoints.

### Invocations

```text
tailscale set --advertise-routes=<comma-separated-complete-set>
tailscale set --advertise-routes=
tailscale set --advertise-exit-node=true|false
tailscale set --advertise-connector=true|false
tailscale set --relay-server-port=<port-or-empty>
tailscale set --relay-server-static-endpoints=<comma-separated-or-empty>
```

For a form changing multiple advertisement fields, one `tailscale set` command
may include exactly those changed flags in deterministic order.

Static endpoints are parsed as socket addresses before invocation. Port must be
0–65535 according to CLI semantics; the empty value disables, while `0` asks the
daemon to choose a port and must not be described as disabled.

### Safety and terminology

- Preview the entire resulting route set, not only the delta.
- Warn about CIDR overlaps detected locally; warning does not block apply and
  does not claim routing failure.
- State: “This device will advertise; a tailnet administrator may still need to
  approve the route.”
- Do not display or call an admin approval action in this phase.
- If the CLI requires `mac-app-connector` risk acceptance, add a specific
  explicit confirmation and only then pass that exact risk; never pass `all`.

### Verification

Fresh preferences must match the complete canonical set and submitted toggles.
Status can enrich whether routes appear, but preference read is authoritative
for the local requested advertisement.

### Tests

- IPv4/IPv6 parse/canonicalization, duplicates, overlaps, invalid host bits,
  empty clear, port 0 versus empty, IPv6 static endpoint brackets, deterministic
  argv, risk acceptance, and verification mismatch.

## 03.7 Account lifecycle

Account support is capability-gated because the checked `switch`, `switch
remove`, and `login` commands are alpha.

### List

Invoke:

```text
tailscale switch --list --json
```

Decode into:

```text
LocalAccount
  id
  tailnet_name?
  account_name?
  display_name?
  profile_name?
  active
```

Unknown fields are accepted. Account ID is opaque and is the action target.

### Switch

```text
tailscale switch <id>
```

Preview old/new account. After success, invalidate all local snapshots,
capabilities tied to the active profile, selections, and diagnostics. Refresh
status/preferences/accounts. Do not retain a selected peer from the old
tailnet.

### Add/login

Launch `tailscale login` through terminal handoff with no auth-key, client
secret, ID token, or other secret flags. Tale does not collect login credentials
in Phase 3.

After return, refresh accounts/status. A non-zero child exit is recorded without
trying to parse or replay the authentication URL.

### Logout

Invoke `tailscale logout` through terminal handoff because policies may require
interactive explanation. Preview explicitly says the current node key is
invalidated and future use requires reauthentication. Treat as disruptive.

### Remove local account

```text
tailscale switch remove <id>
```

Require typing the selected display name, falling back to ID when no display
name exists. Explain that this removes the local account profile and does not
delete the Tailscale account or user.

### Tests

- Versioned JSON fixtures, missing display fields, duplicate names, exact ID
  argv, active switch, switch failure, login/logout handoff, remove confirmation,
  and complete state invalidation after account change.

## 03.8 Interactive terminal handoff

### Required files

- `src/local/handoff.rs`
- `src/terminal.rs`
- `tests/interactive_handoff.rs`

### Handoff contract

For login/logout, SSH, and `nc`:

1. Refuse start while another interactive child owns the terminal.
2. Pause Tale input and rendering.
3. Mark periodic refreshes suspended; do not cancel completed snapshot state.
4. Restore cursor, paste, mouse, alternate screen, and raw mode.
5. Spawn the child directly with inherited stdin/stdout/stderr.
6. Forward supported termination/resize behavior without competing for input.
7. Wait for child exit.
8. Re-enter Tale terminal state.
9. Force a complete redraw.
10. Refresh affected sources and record exit status/timing.

If re-entering the TUI fails, leave the terminal in normal mode and return an
error. Never repeatedly attempt to enter raw mode.

### SSH

Initial SSH form accepts only optional username and selected host:

```text
tailscale ssh <host>
tailscale ssh <user>@<host>
```

Arbitrary trailing SSH arguments are excluded. Validate username as non-empty
and without `@`; host comes from selected device DNS name or Tailscale IP. The
combined `user@host` is one argv value.

### Netcat

```text
tailscale nc <host> <port>
```

Port is a parsed integer 1–65535. Tale does not send preconfigured input,
capture session contents, or interpret protocol data.

### Tests

PTY tests cover success, non-zero exit, child signal, Tale shutdown request while
child owns terminal, failed spawn, and failed TUI re-entry. After each case,
terminal mode and cursor are correct.

## 03.9 System policy inspection and reload

### Invocations

Read:

```text
tailscale syspolicy list --json
```

Reload:

```text
tailscale syspolicy reload --json
```

### UI

Add a System Policy section under Local showing effective setting, source, value,
and errors returned by the CLI. Do not conflate system/MDM policy with the
tailnet HuJSON access policy.

Reload is a reversible local action with preview and verification by a fresh
`list --json`. It is enabled only when the command exists. It does not edit any
policy.

### Client update boundary

Version/update availability may be displayed from Phase-2 version data. Running
`tailscale update`, selecting tracks, upgrades, and downgrades remain research-
gated and are absent from actions/help.

### Tests

- List/reload fixtures, effective source display, policy error, command missing,
  permission denied, reload non-zero, and fresh-list verification.

## 03.10 Local action IDs

Add at least these stable registry IDs when their feature ships:

```text
local.connect
local.disconnect
local.preferences.edit
local.exit_node.select
local.routes.edit_advertisements
local.account.switch
local.account.login
local.account.logout
local.account.remove
local.ssh.open
local.nc.open
local.syspolicy.reload
```

Do not bind destructive/disruptive actions directly. Suggested safe contextual
bindings may open a pre-filled form/action, but dispatch still flows through
preview and confirmation.

## 03.11 Phase verification and handoff

Run:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Required disposable/mock journeys:

1. Toggle each supported preference and verify fresh state.
2. Connect/disconnect, including explicit lose-SSH risk acceptance.
3. Select, auto-select, and clear an exit node.
4. Add/remove/clear subnet advertisements and toggle exit advertisement.
5. Exercise app connector/relay controls only on a supporting client.
6. Switch accounts and prove old-tailnet selection/state is cleared.
7. Complete/cancel/fail SSH and `nc` handoffs.
8. List and reload system policy.
9. Repeat mutations in global read-only mode and prove no child is spawned.
10. Induce command success plus verification mismatch.

Mutating integration tests require an explicit disposable local profile and are
never part of the default test command.

The phase handoff must report:

- the approved preference-read decision and supported platforms;
- exact preference/action argv and version fixtures;
- which mutations were exercised against a disposable real node;
- alpha account features actually verified;
- terminal handoff platform matrix;
- confirmation that no sudo, remote daemon control, Control API mutation, Serve,
  Funnel, or file-transfer action was introduced.
