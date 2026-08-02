# End-to-end feature plan

This is the implementation order for the complete Tale TUI. It turns the
feature catalog into independently usable vertical slices and is the source of
truth for what belongs in each release.

The plan does not authorize placeholder routes, unused abstractions, or
half-connected controls. A phase is complete only when its user journeys work
end to end against mock contracts and the real integration surface appropriate
to that phase.

## Implementation specifications

- [01 — TUI foundation](specs/01-tui-foundation.md)
- [02 — Local observer](specs/02-local-observer.md)
- [03 — Local operator](specs/03-local-operator.md)
- [04 — Local services](specs/04-local-services.md)
- [05 — Admin observer](specs/05-admin-observer.md)
- [06 — Admin operator](specs/06-admin-operator.md)
- [07 — Access, credentials, and audit security](specs/07-access-security.md)
- [08 — Operational depth](specs/08-operational-depth.md)
- [09 — Tale 1.0 hardening](specs/09-one-zero-hardening.md)

These are agent-ready contracts for every implementation phase. The roadmap
remains the ordering and scope index; when a specification is more precise
about its phase behavior, the specification controls.

## Delivery shape

| Phase | Milestone | Product result |
| --- | --- | --- |
| 0 | Product contracts | researched scope, architecture, UX, and configuration |
| 1 | TUI foundation | complete offline/mock application shell |
| 2 | Local observer | useful read-only Tailscale monitor and diagnostic tool |
| 3 | Local operator | safe control of the current Tailscale node |
| 4 | Local services | Serve, Funnel, transfers, drives, and certificates |
| 5 | Admin observer | authenticated read-only tailnet administration |
| 6 | Admin operator | device, route, DNS, and user mutations |
| 7 | Access and security | policy, credentials, and audit workflows |
| 8 | Operational depth | fleet intelligence, flows, webhooks, exports, and saved views |
| 9 | 1.0 hardening | performance, platform, packaging, accessibility, and security gates |

Phases 2–8 should each be releasable as a useful pre-1.0 milestone. Phase 9 is
the point at which the complete supported feature set becomes Tale 1.0.

## Dependency map

```text
Phase 0: contracts
    │
    ▼
Phase 1: TUI foundation
    ├─────────────── local plane ─────────────────┐
    │                                              │
    ▼                                              ▼
Phase 2: local observer ─▶ Phase 3: local operator ─▶ Phase 4: local services
    │
    └─────────────── admin plane ─────────────────┐
                                                   ▼
                          Phase 5: admin observer ─▶ Phase 6: admin operator
                                                   │
                                                   ▼
                                      Phase 7: access and security
                                                   │
                     Phase 4 + Phase 7 ────────────┘
                                                   ▼
                                      Phase 8: operational depth
                                                   │
                                                   ▼
                                      Phase 9: Tale 1.0
```

The numbered order is the default delivery order. The diagram also records the
actual technical dependencies: the admin client can begin after the TUI
foundation, but the default plan completes the local product first so Tale is
useful before credentials or Control API work exists.

## Rules for every vertical slice

Every feature slice includes all of the following or it is not complete:

1. A domain type and source-of-truth decision.
2. A typed local command or endpoint-specific API method.
3. Reducer state, events, effects, cancellation, and stale-result behavior.
4. Loading, empty, partial, stale, forbidden, unsupported, and error states.
5. Responsive rendering and contextual help.
6. Action capability and risk classification when it mutates anything.
7. Fictional fixtures with no real tailnet or secret data.
8. Unit, adapter-contract, and relevant UI snapshot tests.
9. Updated user-facing documentation.

No phase adds menu entries for later phases. Undelivered features remain in
documentation, not as disabled “coming soon” controls.

## Complete feature-to-phase map

| Area | First useful phase | Completed capability |
| --- | --- | --- |
| Application shell | 1 | terminal lifecycle, routes, panes, overlays, tasks, help |
| Overview | 1 | mock source health; local health in 2; combined admin queues in 5; fleet findings in 8 |
| Local node | 2 | read state in 2; write preferences and accounts in 3 |
| Devices | 2 | local inventory in 2; admin enrichment in 5; admin mutations in 6 |
| Connectivity | 2 | ping, netcheck, DNS query, whois in 2; SSH and `nc` in 3 |
| Routes | 3 | local selection/advertisement in 3; admin read in 5; approval in 6; conflict analysis in 8 |
| DNS | 2 | local diagnosis in 2; admin read in 5; admin editing in 6 |
| Accounts | 3 | list, switch, login, logout, and remove with terminal handoff |
| Local services | 4 | Serve, Funnel, Taildrop, Taildrive, and certificate workflows |
| Admin profiles | 5 | config, keyring, OAuth exchange, access-token override, read-only locks |
| Users | 5 | inventory in 5; approval, role, suspend, restore, and delete in 6 |
| Access policy | 5 | source view in 5; edit, validate, tests, preview, diff, save in 7; explorer in 8 |
| Credentials | 5 | inventory in 5; auth-key creation and supported revocation in 7 |
| Activity | 1 | Tale task history in 1; configuration audit log in 5/7; network flows in 8 |
| Webhooks/log streaming | 8 | inspect, create, edit, test, rotate, and delete where documented |
| Fleet health | 8 | expiry, approvals, versions, route overlap, posture, relay-heavy observations |
| Saved views/export | 8 | persistent structured views and deterministic secret-free exports |
| Mouse and UI customization | 8/after 1.0 | optional mouse in 8; key/theme remapping only after action and style contracts stabilize |
| Console-only or alpha features | research track | added only after the public-contract gate passes |

## Phase 0 — product contracts

Status: complete.

### Outcome

Implementation can begin without inventing product semantics.

### Deliverables

- domain research and supported-surface boundary;
- product promise, modes, resource model, and feature catalog;
- responsive layout, navigation, action safety, and core user flows;
- Model–Update–View architecture and adapter boundaries;
- configuration, credential, and privacy contracts;
- this end-to-end feature plan.

### Exit gate

- Documents agree on route names, source labels, capability states, risk tiers,
  configuration names, and non-goals.
- Public-contract gaps are explicit.
- Local operations mean the installed CLI/current daemon; admin operations mean
  the HTTPS Control API.
- Remote tailnet administration is not described as remote control of another
  machine's local daemon.

## Phase 1 — TUI foundation

This phase produces a complete fictional/offline Tale. It proves the interaction
and runtime architecture before Tailscale output is allowed to shape the UI.

### User outcome

The user can launch Tale in mock mode, navigate every foundation interaction,
inspect fictional devices, filter and sort them, open help, and observe or
cancel background tasks. No Tailscale installation or network is used.

### Ordered slices

#### 1.1 Process and terminal lifecycle

- Parse `tale`, `--config`, `--view`, `--read-only`, `--no-local`, and
  `--tailscale-path`, and `--mock`.
- Install structured top-level error handling without panics.
- Enter raw mode and alternate screen through an RAII terminal session.
- Restore the terminal on normal exit, error, `Ctrl+c`, and supported signals.
- Handle resize events and minimum-size rendering.

#### 1.2 Event, update, and effect loop

- Add typed input, tick, task, and shutdown events.
- Implement the single `App::update` mutation boundary.
- Add bounded channels and render invalidation.
- Introduce cancellable effects and monotonically increasing generations.
- Prove that stale task results cannot replace newer state.

#### 1.3 Frame and navigation

- Render header, view title, collection, inspector, notification line, and
  contextual footer.
- Implement route stack, pane focus, overlay stack, and back behavior.
- Add `:`, `/`, `?`, `Tab`, `Esc`, `r`, `R`, `@`, `q`, and collection movement.
- Add command-palette completion for routes and aliases.
- Add responsive layouts for 60x18, 80x24, 110x30, and 160x45.

Only Overview, Devices, Activity, and Settings routes exist in this phase. Other
routes enter the registry when their first real slice ships.

#### 1.4 Action and help registry

- Define stable action IDs, labels, contexts, bindings, capability reason, and
  risk metadata.
- Generate footer hints and searchable full help from the registry.
- Add the contextual action and copy-field pickers.
- Prove that an unavailable action cannot be dispatched by a hidden binding.

#### 1.5 Task engine and mock source

- Add queued, running, cancelling, succeeded, failed, and cancelled tasks.
- Bound output and in-memory history.
- Render the Activity task list and task inspector.
- Add deterministic fictional local snapshots and delayed mock effects.
- Include a mock failure and stale-snapshot scenario.

#### 1.6 Configuration foundation

- Resolve config/state/cache paths.
- Implement the root, local, UI, and history fields used by this phase.
- Reject unknown fields and invalid duration/range values.
- Add `tale config path`, `tale config check`, and the non-secret portion of
  `tale doctor`.
- Do not rewrite a valid config merely to materialize defaults.

### Phase gate

- All foundation interactions work in mock mode using only the keyboard.
- Quit/cancellation behavior is deterministic with zero, one, or many tasks.
- Selection is stored by resource ID and survives filtering, sorting, and mock
  refreshes.
- ASCII/no-color mode conveys every state.
- Snapshot tests pass at all required viewport sizes.
- No application or test-support path uses `unsafe`, panic, unwrap, or expect.
- The executable has no local-process or HTTP integration yet.

## Phase 2 — local observer

This is the first release useful against a real Tailscale installation. It is
strictly read-only apart from diagnostic traffic initiated by the user.

### User outcome

The user can see the current node and peers, understand source freshness, filter
a large device list, inspect connection paths, and run focused diagnostics
without creating an API credential.

### Ordered slices

#### 2.1 Local capability discovery

- Resolve the configured `tailscale` executable without a shell.
- Read the exact client version.
- Distinguish missing executable, daemon unavailable, permission denied, logged
  out, stopped, unsupported version, and running.
- Record command availability without trying legacy command forms.
- Show remediation through copyable text; never install, start, or elevate.

#### 2.2 Status contract

- Invoke `tailscale status --json` with bounded output and timeout.
- Decode the self node, peers, users, stable IDs, addresses, OS, liveness,
  connection path, traffic counters, routes, and optional metadata used by the
  UI.
- Convert DTOs into a `LocalSnapshot` atomically.
- Preserve the last good snapshot after a failed refresh and display its age.
- Add fixtures labeled by exact Tailscale version and platform.

#### 2.3 Local Overview and Devices

- Replace fictional Overview source cards with daemon/login/client state.
- Render local address, DNS name, version, peer counts, direct/relay counts, and
  last successful refresh.
- Render Devices collection and inspector with local source labels.
- Add stable sorting by name, state, owner, OS, path, traffic, and last seen.
- Add free-text and the first structured filters: `online`, `owner`, `os`,
  `path`, `tag`, `lastSeen`, and device properties present locally.

#### 2.4 Ping workflow

- Add `Probe connection` to eligible devices.
- Stream typed ping samples into a cancellable task.
- Show path transitions, loss, current/minimum/average/maximum latency, and the
  final process result.
- Preserve raw bounded output only in the current task inspector.
- Never change peer health state solely because a user-cancelled probe ended.

#### 2.5 Network diagnostics

- Add Netcheck with JSON/JSON-line parsing and a DERP latency table.
- Add local DNS status and query.
- Add Whois for an address or selected peer.
- Connect diagnostic results to the selected device without merging them into
  long-lived inventory fields.
- Add a redacted copyable diagnostic summary.

#### 2.6 Refresh and degradation

- Refresh local status on the configured interval without overlap.
- Pause polling during interactive overlays that require stable input.
- Show refresh progress only when it exceeds a short perceptual threshold.
- Back off after repeated daemon failures while keeping manual refresh active.
- Return immediately to normal cadence after a successful manual refresh.

### Views completed in this phase

- Overview: local source health and peer summary.
- Local: read-only current-node identity and observed preferences where a stable
  documented command exposes them.
- Devices: local collection, inspector, filtering, sorting, and diagnostics.
- DNS: local status and query tool.
- Activity: real local diagnostic tasks.
- Settings: local executable and refresh settings.

### Phase gate

- Tale is useful with no config file and no Control API credential.
- No local mutation command is reachable.
- Local commands use explicit argv and never a shell.
- Output changes fail at the DTO boundary and cannot clear the last good state.
- A 5,000-row fictional device fixture remains responsive while filtering and
  refreshing.
- Ping, netcheck, DNS, and whois cancellation leave the terminal functional.
- Real-integration tests are opt-in and read-only by default.

## Phase 3 — local operator

This phase safely controls the machine on which Tale is running. It does not
claim to change the local preferences of other devices.

### User outcome

The user can connect or disconnect the local node, change supported preferences,
select an exit node, advertise routes, switch accounts, and open remote sessions
without memorizing Tailscale flags.

### Ordered slices

#### 3.1 Mutation framework

- Implement typed forms, domain-language previews, risk-tier confirmations,
  mutation locks, and post-action verification reads.
- Add requested-state task presentation without optimistically replacing the
  verified value.
- Store redacted argv, timing, and result status in task history.
- Make `--read-only` and the global config lock disable every mutation.

#### 3.2 Connection and preferences

- Before building preference forms, record the exact current-value contract:
  use documented CLI output or the individually stable LocalAPI preferences
  method after a focused transport decision. Do not adopt undocumented
  `tailscale debug` output merely because another TUI parses it.
- Connect and disconnect the current client.
- Read and edit accept-routes, accept-DNS, shields-up, Tailscale SSH server,
  auto-update preference, posture reporting, hostname, and supported forwarding
  settings.
- Show policy-managed or permission-denied preferences as disabled with reasons.
- Verify each requested change from fresh daemon state.

#### 3.3 Exit nodes and route advertisement

- List eligible exit nodes with current path and observed latency.
- Select or clear the local exit node and configure LAN access.
- Configure exit-node advertisement, subnet CIDRs, app connector, and peer relay
  only where the installed client documents support.
- Validate CIDRs and show the entire resulting advertised set before apply.
- Explain that advertisement is local and approval is an admin-plane action.

#### 3.4 Account lifecycle

- List local profiles and identify the active one.
- Switch profiles through a non-interactive typed command when supported.
- Add/login, logout, and remove a local profile through explicit terminal
  handoff when interaction is required.
- Warn that removing a local profile is not deleting a Tailscale user or
  tailnet.
- Refresh all local snapshots after handoff.

#### 3.5 Interactive connections

- Add SSH and `nc` actions to eligible devices.
- Restore the normal terminal before the child takes stdin/stdout.
- Forward supported signals and restore Tale after success, non-zero exit,
  cancellation, or child signal.
- Use the selected stable device address/name and optional remote username as
  separate argv values.

#### 3.6 Local policy and maintenance inspection

- Show applied system policy and errors where documented.
- Support non-destructive reload only when the client exposes it and the action
  is unambiguous.
- Show client-update availability where exposed.
- Keep installation, automatic sudo, and update/downgrade execution behind the
  research gate.

### Views completed in this phase

- Local: full supported preference editor and account entry point.
- Routes: local exit-node choice and advertisements.
- Devices: SSH/`nc` actions.
- Activity: mutation previews, verification, and interactive-child results.

### Phase gate

- Every mutation has preview, confirmation appropriate to its risk, and a fresh
  verification read.
- A daemon mismatch is reported as failure; requested state never survives as
  if verified.
- Tale never invokes `sudo` or asks for a sudo password.
- Interactive child PTY tests pass for every exit path.
- Admin-only concepts such as route approval do not appear as local actions.

## Phase 4 — local services and transfers

This phase completes the supported local CLI surface that benefits from a
structured TUI.

### User outcome

The user can expose a local service privately or publicly, transfer files,
manage Taildrive shares, request certificates, and capture support diagnostics
with clear destination and exposure semantics.

### Ordered slices

#### 4.1 Services view and model

- Add the Services route with separate Serve, Funnel, Taildrop, Taildrive,
  Certificates, Metrics, and Bug report sections.
- Model a local listener, protocol, port, path, target, background state, and
  public/private exposure without treating it as an admin API service.
- Show local source freshness and unsupported-client capability states.

#### 4.2 Serve

- Read and render Serve status.
- Create/update HTTPS, HTTP, TCP, and path mappings supported by the installed
  client.
- Preview the complete listener-to-target mapping before apply.
- Replace an individual mapping only when the installed CLI exposes a
  documented precise operation; reset the complete Serve configuration with an
  appropriately scoped confirmation.

#### 4.3 Funnel

- Read and render Funnel status separately from Serve.
- Create/update/reset supported public mappings.
- Treat enabling public internet exposure as risk tier 2.
- Display the public URL and copy/open actions only after verification.

#### 4.4 Taildrop

- Select an explicit source file and destination device.
- Show transfer progress when the CLI exposes it, cancellation, and final
  destination/result.
- Receive into an explicit destination with `skip`, `overwrite`, or `rename`;
  do not promise a waiting-file inventory without a documented stable surface.
- Never overwrite a local file without the user's selected conflict policy.

#### 4.5 Taildrive and certificates

- List, add, rename, and remove local Taildrive shares through the documented
  alpha CLI after an explicit per-run opt-in.
- Validate share names and local paths.
- Request HTTPS certificates to explicit user-selected paths.
- Never render, copy, or record private-key contents.
- Make renewal responsibility visible for file-based certificates.

#### 4.6 Support and metrics

- Add bounded client-metrics inspection where documented.
- Add a bug-report action that explains what it generates and records only the
  resulting identifier.
- Do not upload Tale logs or diagnostic bundles automatically.

### Phase gate

- Serve and Funnel cannot be confused visually or in action copy.
- Public exposure always requires a deliberate confirmation.
- File paths are passed as argv/path values, never shell fragments.
- Transfer cancellation and conflict behavior are deterministic.
- Secrets and private key contents are absent from UI history and logs.

## Phase 5 — admin observer

This phase introduces the Control API and remains read-only. It proves auth,
pagination, source composition, and permission behavior before any remote
mutation exists.

### User outcome

An operator can add a scoped profile and inspect the tailnet's devices, users,
routes, DNS, policy, credential metadata, settings, and configuration audit
events. Local mode continues independently.

### Ordered slices

#### 5.1 Profiles and credentials

- Implement profile parsing and selection.
- Add OS-keyring records for scoped OAuth clients and API access tokens.
- Implement `tale auth add`, `remove`, and `status` with transactional writes.
- Exchange OAuth client credentials for one-hour access tokens and refresh them
  before expiry.
- Support `TALE_ACCESS_TOKEN` only as an ephemeral override.
- Add `--profile` selection and profile/read-only state to the header.

#### 5.2 HTTP client foundation

- Add HTTPS requests, endpoint-specific methods, timeouts, cancellation,
  pagination, retry metadata, request IDs, and bounded error bodies.
- Separate unauthenticated, forbidden, plan-restricted, validation, rate-limit,
  transport, timeout, and unsupported responses.
- Never retry a mutation; this phase contains none.
- Add a deterministic fake HTTP server for contract tests.

#### 5.3 Device inventory and composition

- Read tailnet devices and device details.
- Add admin-only fields: creator/owner, tags, approval, key expiry, advertised
  routes, posture attributes, sharing state, update state, and endpoints where
  documented.
- Compose local and admin records only through an exact shared stable ID.
- Show source and freshness for each field group.
- Add the complete documented device-filter vocabulary supported by returned
  data.

#### 5.4 Users, routes, and DNS

- Add Users collection and inspector.
- Add admin route inventory with advertised versus approved state.
- Add admin DNS nameservers, preferences, search paths, and split-DNS suffixes.
- Add cross-resource jumps among user, owned devices, tags, routes, and DNS
  resources.

#### 5.5 Access, credentials, settings, and audit reads

- Render the exact remote HuJSON policy source read-only.
- Show credential metadata that the configured scopes permit without displaying
  secret values.
- Show supported tailnet feature/contact settings read-only.
- Read configuration audit events and link known target IDs to resources.
- Render policy diffs returned in audit events.

#### 5.6 Combined Overview and capability model

- Add admin source health independently from local source health.
- Add queues for pending approvals, expired/soon-expiring keys, route approvals,
  stale source data, and client version skew.
- Resolve capabilities per endpoint/action from configured read-only state and
  observed responses.
- Keep forbidden and unsupported operations discoverable in help without adding
  executable mutation controls.

### Views completed in this phase

- Overview: combined local/admin source status and actionable read-only queues.
- Devices: combined inventory and inspector.
- Users: read-only inventory and inspector.
- Routes: admin advertisement/approval state alongside local configuration.
- DNS: admin configuration alongside local diagnostics.
- Access: exact read-only policy and audit diffs.
- Credentials: metadata inventory.
- Activity: configuration audit log plus Tale tasks.
- Settings: profiles and read-only tailnet settings.

### Phase gate

- Tale is valuable in `--no-local` admin-only mode.
- Local mode remains fully usable when admin auth or transport fails.
- API secrets never appear in config, URLs, traces, task output, error details,
  or debug formatting.
- `403` disables only the affected capability.
- Pagination and cancellation contract tests exist for every list endpoint.
- No Control API mutation method is callable.

## Phase 6 — admin operator

This phase handles the daily resource mutations that make Tale a practical
admin-console alternative.

### User outcome

An authorized operator can manage devices, route approvals, DNS, and users with
typed previews, risk-based confirmations, per-target outcomes, and verification.

### Ordered slices

#### 6.1 Admin mutation protocol

- Extend the action registry with endpoint and scope requirements.
- Fetch a fresh target immediately before tier-3 actions.
- Prevent concurrent mutations of the same resource ID.
- Do not automatically retry any mutation.
- Re-fetch every affected resource after completion.
- Correlate the resulting configuration audit event when it becomes available,
  without treating delayed audit delivery as mutation failure.

#### 6.2 Device management

- Rename a device.
- Replace device tags with a complete old/new preview.
- Authorize or revoke authorization, using Tailscale's exact terminology.
- Modify supported key-expiry behavior and expire a key immediately.
- Apply supported device IP/key operations only after their semantics are
  separately documented in the action copy.
- Remove a device through typed confirmation.

Device approval and Tailnet Lock signing remain separate concepts and actions.

#### 6.3 Route management

- Approve or revoke advertised subnet routes.
- Approve or revoke exit-node route capability.
- Support batch selection grouped by advertising device.
- Show CIDR, advertiser, previous approval, and requested approval in preview.
- Never offer route advertisement as an admin action; that remains local to the
  advertising machine.

#### 6.4 DNS management

- Edit MagicDNS preference where the API permits it.
- Replace nameserver and search-path lists with ordered complete previews.
- Add/update/remove split-DNS suffix mappings.
- Validate IP addresses and domain names locally, then rely on server
  validation as authoritative.
- Refresh local DNS diagnosis separately after an admin DNS change.

#### 6.5 User management

- Approve users.
- Change supported roles.
- Suspend and restore users with affected-device context.
- Delete users through typed confirmation and a fresh membership/device check.
- Treat invitations and external sharing as research-gated, not implied by user
  administration.

#### 6.6 Batch and partial failure UX

- Show every target before dispatch.
- Use one task with per-target child outcomes when the API requires separate
  calls.
- Preserve successful targets and make failed targets selectable for review.
- Never offer “retry all” for a non-idempotent action without a new preflight.

### Phase gate

- Profile and global read-only locks disable every mutation at dispatch time as
  well as in the UI.
- Tier-3 actions require typing the target name or generated phrase.
- Device, route, DNS, and user contract tests assert exact methods, paths,
  headers, bodies, error classes, and verification reads.
- Batch actions cannot collapse partial failure into a global success message.
- No policy or secret-creation mutation ships in this phase.

## Phase 7 — access, credentials, and audit security

This phase completes the high-risk administrative workflows around policy and
credential material.

### User outcome

An authorized operator can safely edit and validate access policy, preview its
effect, run policy tests, create auth keys, revoke supported credentials, and
review the audit trail without exposing secrets.

### Ordered slices

#### 7.1 Secure external-editor workflow

- Write the exact remote HuJSON to a mode-0600 temporary file.
- Suspend Tale and launch `$VISUAL`, falling back to `$EDITOR`, through explicit
  argv.
- Restore Tale and preserve candidate contents after editor failure.
- Keep the base remote document hash and fetch the remote source again before
  apply.
- Block save after a remote change; do not auto-merge or normalize.

#### 7.2 Policy validation and tests

- Send the candidate to the documented validation endpoint.
- Render structured locations/messages where supplied and bounded raw detail
  otherwise.
- Run declared policy tests and group failures by test/source/destination.
- Keep the editor flow open after invalid content.
- Require a successful current validation immediately before save.

#### 7.3 Permission preview and diff

- Use the documented policy preview endpoint rather than a local evaluator.
- Present user/device selectors, destinations, ports, posture, routing, SSH, and
  application capabilities returned by Tailscale.
- Render a textual source diff that preserves comments and formatting.
- Show the base and candidate observation timestamps.

#### 7.4 Policy save and audit correlation

- Require the full diff and explicit confirmation.
- Re-check the remote base, validate again, then submit once.
- Fetch the saved policy and compare it with the candidate.
- Link the later configuration audit event and its server-recorded diff.
- Retain reconciliation files only until the user explicitly closes the failed
  workflow.

#### 7.5 Auth-key creation

- Collect tags, reusable/ephemeral/preauthorized properties, and expiry.
- Preview every requested property and credential scope.
- Display the returned secret exactly once in a dedicated ephemeral view.
- Allow copy, then zeroize/drop the value when the view closes.
- Store only non-secret metadata and result status in task history.

#### 7.6 Credential revocation

- Revoke supported auth/API/OAuth/federated credentials only when the endpoint
  and current credential type permit it.
- Show type, owner, scope, created/expiry dates, and known dependents before
  typed confirmation.
- Keep “remove Tale keyring record” and “revoke remote credential” as separate
  actions.

#### 7.7 Audit investigation

- Add time, actor, action, and target filters.
- Cross-link device, user, route, DNS, credential, and policy events.
- Render old/new values with secret redaction.
- Export a selected audit window only through the phase-8 export system; do not
  add an ad hoc exporter here.

### Phase gate

- Policy comments and formatting survive when the user does not change them.
- A concurrent remote change always blocks save.
- Tale never claims to evaluate reachability independently.
- Secret-result values cannot be reopened from history.
- Debug and display traits for secret-bearing types cannot reveal contents.
- Keyring removal and remote revocation have distinct confirmation copy.
- Policy and credential mutation tests include cancellation and failure at every
  step of the workflow.

## Phase 8 — operational depth

This phase adds the features that make Tale more than a terminal copy of the
admin console.

### User outcome

Operators can find fleet risks quickly, investigate traffic and configuration
changes, save operational perspectives, export evidence, and answer focused
access questions.

### Ordered slices

#### 8.1 Fleet-health engine

- Add deterministic findings for expired and soon-expiring keys, pending
  approvals, client version skew, failed/stale sources, CIDR overlap, missing
  posture data, and relay-heavy observed connections.
- Include observed facts, timestamps, affected IDs, severity, and typed
  suggested actions.
- Treat offline age as informational unless a future explicit expectation says
  otherwise.
- Distinguish derived findings from Tailscale-reported errors.

#### 8.2 Network flow logs

- Read plan-permitted flow logs for an explicit time window.
- Resolve node IDs through current device snapshots without replacing raw IDs.
- Filter and aggregate source, destination, protocol/port, time, and byte counts.
- Make the absence of packet contents explicit.
- Keep large result sets paged/windowed and cancellable.

#### 8.3 Webhooks and log streaming

- Inspect configured webhook endpoints, subscribed events, and status.
- Create, edit, test, rotate, and delete where documented.
- Display rotated secrets once with the phase-7 ephemeral-secret component.
- Inspect and manage supported configuration/network log-stream destinations.
- Preview destination, log type, and complete replacement semantics.

#### 8.4 Saved views

- Persist route, structured filter, stable sort, and selected column set under a
  user-chosen name.
- Add a saved-view picker to command mode.
- Do not persist selected resource IDs, source data, or credential information.
- Reject saved views referencing removed fields after a breaking config change;
  do not migrate or silently reinterpret them.

#### 8.5 Export

- Export the active supported collection as JSON or CSV to an explicit path.
- Include source, observation timestamp, active filter, sort, and schema name.
- Produce deterministic field order and redact all secret-capable fields.
- Never overwrite an existing file without explicit confirmation.

#### 8.6 Access Explorer

- Accept a structured question: source selector, destination selector, protocol
  or port, and optional SSH/application capability.
- Translate it into documented preview/test requests.
- Present the authoritative result, matched rules/capabilities where returned,
  and limitations.
- Do not render a speculative full network graph.

#### 8.7 UI depth

- Add opt-in mouse focus, selection, scrolling, and action activation with full
  keyboard parity.
- Add user-selected standard/wide columns per saved view.
- Improve searchable help, task history filtering, and cross-resource jumps.
- Keep semantic colors and symbols fixed; general theme and key remapping remain
  deferred until after the 1.0 action/style contracts are reviewed.

### Phase gate

- Every health finding is reproducible from a documented snapshot fixture.
- Flow-log UI and export never imply access to packet contents.
- Exports are deterministic and secret-free.
- Saved views contain presentation/query state only.
- Rotated secrets use the same view-once guarantees as auth keys.
- Access Explorer results come exclusively from Tailscale preview/tests.
- All new large-data views remain responsive against representative maximum
  fixtures.

## Phase 9 — Tale 1.0 hardening

No new product domain enters this phase. It turns the supported Phase 1–8
feature set into a reliable release.

### Platform and compatibility

- Declare and test the exact supported Tailscale client version range.
- Capture fixtures for each supported macOS, Linux, and Windows client family.
- Verify executable discovery, path handling, permissions, signals, terminal
  restoration, keyring behavior, and interactive children per platform.
- A platform is listed as supported only after its full core-flow matrix passes.
- Reject unsupported client output clearly; add no legacy fallback paths.

### Performance and resilience

- Measure input-to-render latency with 5,000 devices and large log windows.
- Ensure filtering/sorting does not block input for perceptible periods.
- Bound every channel, output buffer, task list, log window, and cache.
- Verify refresh cancellation, backoff, and last-good-state behavior during
  sustained daemon/API failures.
- Verify no mutation is duplicated after timeout, reconnect, or process resume.
- Profile and remove avoidable clones in hot paths.

### Security review

- Trace every secret from input through storage, HTTP/process invocation,
  rendering, copy, task history, logging, error handling, and destruction.
- Verify shell execution is absent.
- Verify `sudo` invocation and privilege-helper code are absent.
- Verify TLS defaults, redirect behavior, keyring service/account names, temp
  permissions, and export redaction.
- Run dependency/license/advisory review and document accepted risk.

### Accessibility and terminal matrix

- Complete keyboard-only journeys for every core action.
- Verify ASCII, `NO_COLOR`, ANSI16, ANSI256, and TrueColor presentation.
- Verify 60x18 minimum handling and complete 80x24 drill-down workflows.
- Test common terminals and tmux for width, mouse opt-in, clipboard, resize, and
  alternate-screen restoration.
- Ensure every color state also has text or a stable symbol.

### Packaging and operations

- Produce reproducible release binaries for supported platform/architecture
  pairs.
- Support `cargo install` if the dependency and asset model permits it.
- Add shell completions and a man page for the Tale CLI.
- Complete `tale doctor` with a redacted support bundle.
- Document install, update, uninstall, config paths, credential setup, minimum
  Tailscale version, and recovery from a damaged terminal session.
- Add a release checklist and signed artifact process before publishing.

### 1.0 acceptance journeys

The release candidate must pass these complete journeys:

1. Launch with no config and monitor a local tailnet.
2. Diagnose a direct/relay peer problem and copy a redacted report.
3. Change an exit node and verify the resulting local state.
4. Configure and remove a private Serve mapping.
5. Enable Funnel with public-exposure confirmation, then disable it.
6. Add a read-only admin profile and inspect all supported resources.
7. Approve a device and route, then locate their audit events.
8. Edit DNS and verify the resulting ordered configuration.
9. Suspend and restore a user.
10. Edit policy, catch a failing test, fix it, preview, save, and inspect the
    audit diff.
11. Create an auth key, copy it once, close it, and prove it cannot be reopened.
12. Investigate a fleet-health finding and export the filtered evidence.
13. Lose the daemon while admin mode remains usable.
14. Lose API authentication while local mode remains usable.
15. Cancel an interactive or streaming task and exit with the terminal intact.

## Research-gated feature tracks

These features are part of the domain plan but not assigned to a shipping phase
until Tailscale exposes and Tale verifies a sufficient public contract.

### Tailscale Services and discovered endpoints

Required before planning:

- list/detail/mutation endpoints or a documented local contract;
- stable service identity and endpoint models;
- permissions, approval, health, and failure semantics;
- distinction from local Serve/Funnel configuration.

### Device sharing and invitations

Required before planning:

- create/list/revoke endpoints and recipient states;
- quarantine and cross-tailnet semantics;
- role requirements and audit events;
- clear separation between a user invitation and device sharing.

### Tailnet Lock

Required before planning:

- supported status/signing inputs without console scraping;
- trusted-node and signature lifecycle;
- secure handling design for disablement/recovery material;
- platform restrictions and audit behavior.

Tale will not store or guide recovery-secret operations until a separate threat
model is approved.

### OAuth apps

Required before planning:

- alpha status removed or explicitly accepted;
- authorization-code callback UX suitable for a terminal application;
- per-user scopes, refresh/revocation behavior, and audit identity;
- clear benefit over scoped OAuth clients for Tale's operator model.

### ACL-to-grants assistance

Required before planning:

- a supported Tailscale conversion or transformation contract;
- preservation rules for comments, tests, SSH rules, and mixed ACL/grant files;
- server validation and permission-preview equivalence checks;
- a reviewable source diff with no automatic apply.

Tale will not implement its own policy translator from documentation examples.

### Client update orchestration

Required before planning:

- platform-specific support matrix and privilege behavior;
- stable dry-run and result contracts;
- downgrade and interrupted-update recovery rules.

### Organization, identity provider, billing, and domain settings

Required before planning:

- documented API endpoints and role model;
- separation of technical administration from purchasing/billing;
- safe validation and reversal semantics.

## Post-1.0 customization candidates

These are considered only after real usage establishes stable action and visual
contracts:

- custom keybindings bound to stable action IDs;
- named color themes over semantic roles;
- route/filter aliases;
- custom table columns beyond saved standard/wide selections;
- explicitly scoped external actions receiving non-secret structured context.

There is no general shell-plugin system in the 1.0 plan.

## Global definition of done

A phase is complete only when:

- every scoped user journey works end to end;
- all new source data has freshness and failure semantics;
- all new actions have capability, risk, preview, cancellation, and verification
  behavior;
- mock and adapter fixtures contain fictional data only;
- focused tests, full unit/contract tests, formatting, strict Clippy, and
  documentation checks pass;
- docs match shipped routes, bindings, configuration, and limitations;
- no later-phase placeholder UI or unused abstraction was introduced;
- the JJ change contains only the phase's intentional work and is ready for user
  review without pushing or rewriting history.

## First implementation sequence

When implementation begins, take these slices in order and finish each before
starting the next:

1. Phase 1.1 terminal lifecycle.
2. Phase 1.2 event/update/effect loop.
3. Phase 1.3 responsive frame and navigation.
4. Phase 1.4 action/help registry.
5. Phase 1.5 task engine and fictional source.
6. Phase 1.6 configuration foundation.
7. Review the complete offline/mock product.
8. Start Phase 2.1 local capability discovery.

The implementation should not begin with API authentication, policy parsing,
plugins, theming, or the full module tree. Tale first becomes a small working
TUI, then a useful local product, then an admin product, and only then a broader
operations platform.
