# Specification 06 — Admin operator

- Implementation phase: 6
- JJ change description: `feat: add safe tailnet administration`
- Depends on: Specifications 01–05 complete
- Produces: verified device, route, DNS, and user administration

This specification adds daily Control API mutations to the read-only client
from Specification 05. It does not weaken profile read-only locks, secret
handling, endpoint ledgers, or source separation.

## 06.0 Phase contract

### User-visible result

An authorized operator can change supported device, route, DNS, and user state
through typed forms, complete previews, risk-based confirmations, per-target
outcomes, and authoritative post-write reads. Tale never reports a requested
state as applied merely because an HTTP request returned success.

### In scope

- shared Control API mutation lifecycle;
- device rename, tags, approval, key expiry, and removal;
- documented device IP/key operations after their semantics pass a decision
  gate;
- subnet-route and exit-node approval;
- MagicDNS, nameserver, search-path, and split-DNS editing;
- user approval, role changes, suspension, restoration, and deletion;
- safe batch execution and partial-failure review;
- audit-event correlation after verified mutations.

### Explicitly out of scope

- policy-file mutation or local policy evaluation;
- auth-key creation or any secret-returning endpoint;
- credential revocation;
- webhooks, log streaming, flow logs, settings/contact mutations;
- invitations, device sharing, Tailnet Lock, OAuth apps, billing, or IdP setup;
- local daemon operations on another device;
- automatic retries of any mutation.

### Required new ownership

```text
src/admin/mutation.rs
src/admin/device_mutations.rs
src/admin/route_mutations.rs
src/admin/dns_mutations.rs
src/admin/user_mutations.rs
src/domain/admin_mutation.rs
src/ui/components/batch_result.rs
docs/decisions/0002-device-ip-key-mutations.md
tests/fixtures/admin/mutations/
```

Extend the forms and confirmation components from Specification 03. Do not
create a second action, task, HTTP, or confirmation framework.

## 06.1 Mutation contract ledger

Extend `docs/contracts/control-api-2026-08-03.md` before adding each mutation.
Record method, path, scope, exact request content type and body, accepted success
statuses, error schema, response body, idempotency statement, and verification
read. A method is not implementable from a scope table alone; the request and
response must also be proven from current public API documentation.

The initial method inventory is:

| Operation | Method and path | Scope |
| --- | --- | --- |
| delete device | `DELETE /api/v2/device/{device_id}` | `devices:core` |
| set approval | `POST /api/v2/device/{device_id}/authorized` | `devices:core` |
| expire key | `POST /api/v2/device/{device_id}/expire` | `devices:core` |
| assign IP | `POST /api/v2/device/{device_id}/ip` | `devices:core` |
| key operation | `POST /api/v2/device/{device_id}/key` | `devices:core` |
| rename device | `POST /api/v2/device/{device_id}/name` | `devices:core` |
| replace tags | `POST /api/v2/device/{device_id}/tags` | `devices:core` |
| replace enabled routes | `POST /api/v2/device/{device_id}/routes` | `devices:routes` |
| replace nameservers | `POST /api/v2/tailnet/{tailnet}/dns/nameservers` | `dns` |
| set DNS preferences | `POST /api/v2/tailnet/{tailnet}/dns/preferences` | `dns` |
| replace search paths | `POST /api/v2/tailnet/{tailnet}/dns/searchpaths` | `dns` |
| update split DNS | `PATCH` or `PUT /api/v2/tailnet/{tailnet}/dns/split-dns` | `dns` |
| set user role | `POST /api/v2/user/{user_id}/role` | `users` |
| approve user | `POST /api/v2/user/{user_id}/approve` | `users` |
| suspend user | `POST /api/v2/user/{user_id}/suspend` | `users` |
| restore user | `POST /api/v2/user/{user_id}/restore` | `users` |
| delete user | `POST /api/v2/user/{user_id}/delete` | `users` |

Where the table lists alternative methods, the ledger must select the method
for the exact intended semantics. Do not implement both as fallbacks.

## 06.2 Admin mutation lifecycle

### State model

Use the shared task engine with an admin-specific typed request:

```text
AdminMutation<TTarget, TChange>
  mutation_id
  action_id
  profile
  target_id
  base_snapshot
  change
  risk
  preflight?
  state

AdminMutationState
  Editing
  Preflighting
  ConflictDetected
  AwaitingConfirmation
  Dispatching
  Verifying
  CorrelatingAudit
  Succeeded
  SucceededUnverified
  PartiallySucceeded
  Failed
  CancelledBeforeDispatch
  OutcomeUnknown
```

Only the reducer advances state. The HTTP worker emits typed results and never
mutates a resource snapshot directly.

### Universal sequence

1. Capture target ID and the base resource version/hash or relevant fields.
2. Validate form values locally.
3. Fetch fresh server state immediately before confirmation.
4. Compare every field on which the proposed change depends.
5. If it changed, show base/fresh/requested values and require editing or an
   explicit new preview; never silently rebase.
6. Build the exact request body from the fresh state.
7. Show method-independent semantic preview and risk copy.
8. Recheck profile/global read-only state, scope, capability, and task conflict
   at dispatch.
9. Send exactly one mutation request.
10. Fetch the authoritative resource until verified, terminal mismatch, or the
    bounded verification deadline.
11. Refresh affected collections.
12. Search the audit window for a correlating event without blocking success on
    delayed delivery.

No mutation is automatically retried after a transport error, timeout, `429`,
or `5xx`. If the request may have reached the server, enter `OutcomeUnknown`,
perform safe verification reads, and tell the user what is and is not known.

### Locking

Serialize by `(profile, resource_kind, resource_id)`. DNS subresources share a
single tailnet DNS lock because their effects can interact. A device delete
conflicts with all mutations and reads that require that device. Unrelated
resources may proceed within the global task limit.

## 06.3 Risk and confirmation

Use the established tiers:

| Operation | Tier | Required confirmation |
| --- | --- | --- |
| device rename | 1 | complete old/new preview |
| replace tags | 2 | complete ordered/set difference and ownership warning |
| approve device | 2 | target, owner/tags, key and route context |
| revoke approval | 3 | typed device name or generated phrase |
| change key-expiry behavior | 2 | old/new and security consequence |
| expire key now | 3 | typed device name |
| delete device | 3 | typed device name and fresh ownership/route context |
| route approval replacement | 2 | every added/removed CIDR and advertiser |
| DNS mutation | 2 | complete old/new ordered configuration |
| approve/restore user | 2 | user identity and role |
| role change | 2, or 3 for privilege removal/elevation defined by policy | old/new role and affected access |
| suspend user | 3 | typed login name and affected-device count |
| delete user | 3 | typed login name and fresh owned-device list |

Tier 3 requires a fresh preflight no older than 30 seconds at dispatch. If it
expires while the overlay is open, fetch and preview again.

## 06.4 Device rename and tags

### Rename

Register `admin.device.rename`. Validate against the current API's documented
machine-name constraints. Do not assume DNS-label rules are identical. Preview
machine name, MagicDNS name when it will change, stable device ID, owner/tags,
and old/new value.

After dispatch, fetch the device by ID and verify the server-returned canonical
name. Update collection labels only from this read.

### Tags

Register `admin.device.tags.replace`. The form edits the complete desired tag
set, not incremental hidden operations. Canonicalize only according to the
public API (`tag:` prefix and documented syntax), remove exact duplicates, and
present added, removed, and retained tags.

Tagging can change device ownership semantics. The confirmation explains the
currently observed owner and resulting tagged identity without predicting
policy reachability. Verify the complete returned tag set.

## 06.5 Device approval and key expiry

### Approval

Register `admin.device.approve` and `admin.device.revoke_approval`. Use
Tailscale's current user-facing term “approval” even if the public endpoint
retains `/authorized`. The adapter domain request is `SetDeviceApproval(bool)`;
the path name must not leak into UI language.

Approval and Tailnet Lock signatures are independent. If a device also needs a
signature, show that as a separate unsupported capability; approval must not
claim to sign it.

### Key expiry

Register `admin.device.key_expiry.configure` only for a public operation whose
request and resulting state are documented. Register
`admin.device.key_expire_now` for the documented expire endpoint.

Expiring now is irreversible for the current key and may disconnect the device.
Do not call it “disable expiry.” Verify by fresh device state and show the
server timestamp/status. Never attempt automatic reauthentication.

## 06.6 Device IP/key decision gate and deletion

### Required decision

Before exposing IP assignment or device-key operations, create
`docs/decisions/0002-device-ip-key-mutations.md` documenting:

- exact public request and response contracts;
- valid input and conflict behavior;
- effect on current connectivity and MagicDNS;
- reversibility and recovery;
- verification read;
- risk tier and action copy.

If these cannot be proven, omit the actions and record them as unsupported. Do
not infer semantics from endpoint names.

### Deletion

Register `admin.device.delete`. Preflight device detail, routes, ownership/tags,
approval, online observation, and key state. The typed confirmation uses the
current machine name and shows the stable ID. A missing device after dispatch
verifies deletion. A timeout followed by `404` is verified success; a timeout
followed by an existing device is outcome unknown or failed according to the
observed state.

Do not delete owned user records, keyring records, routes on other advertisers,
or local files as side effects.

## 06.7 Route management

Register `admin.routes.replace_approvals`. The unit of mutation is one
advertising device because the documented endpoint replaces that device's
enabled route set.

The editor displays, separately:

- every currently advertised CIDR;
- every currently approved/enabled CIDR;
- selected desired approved set;
- exit-node capability as documented by the route response;
- routes present in the approved set but no longer advertised.

Only advertised routes may be newly approved. Removing approval is allowed for
currently enabled routes. Preserve approved entries outside the user's visible
filter; filters never narrow the replacement body.

Preview added, removed, and retained CIDRs in canonical form. Batch selection
across devices becomes one parent task with one independently preflighted child
per device. Never call local `tailscale set`; admin approval does not advertise
a route.

Verify each device by `GET /api/v2/device/{device_id}/routes` and compare the
complete enabled set. Refresh Devices and Routes afterward.

## 06.8 DNS management

### Shared rules

Register separate actions for MagicDNS/preferences, nameservers, search paths,
and split DNS. Each form begins from a fresh complete server value and submits
the replacement/update semantics documented for that endpoint.

Preserve ordered lists. Dragging is not required; provide move-up/move-down,
insert, edit, and remove actions with keyboard parity. A filtered display can
never become the replacement body.

Validate IP addresses, resolver forms, domains, and duplicates locally where
their syntax is documented. Server validation remains authoritative.

### Required actions

```text
admin.dns.preferences.edit
admin.dns.nameservers.replace
admin.dns.search_paths.replace
admin.dns.split.create
admin.dns.split.edit
admin.dns.split.remove
```

The preview shows complete old and new configuration for the affected
subresource, preserving order. Split-DNS removal names the suffix and resolver
set. Do not imply that an admin change has already reached every client.

After verification, refresh admin DNS and enqueue a separate local DNS status
refresh when local mode is available. Failure of the local refresh does not
roll back or fail the verified admin mutation.

## 06.9 User management

### Approval and role

Register `admin.user.approve` and `admin.user.role.change`. Populate role
choices only from the current public contract; do not invent a role hierarchy.
Show user ID, login/display name, current status, current/new role, and owned
device count.

If the acting profile cannot assign a role, preserve the server's forbidden
error as the action capability. Tale never tries a different role or endpoint.

### Suspend and restore

Register `admin.user.suspend` and `admin.user.restore`. Immediately before
suspend, fetch the user and all devices whose exact owner ID matches. Show the
device names, online observations, routes, and key-expiry context. Do not claim
that audit logs will contain every secondary credential/device effect.

Verify user status, then refresh users, devices, routes, credentials metadata,
and Overview queues independently.

### Delete

Register `admin.user.delete`. Preflight as for suspend and include the complete
currently observed owned-device list. Require the current login name. Submit
once, verify the user is absent or in the documented deleted state, and refresh
related resources.

Deletion must not silently remove local Tale profiles or keyring records even
when their credential owner appears related.

Invitations and external sharing remain unsupported.

## 06.10 Batch execution and partial failure

A batch preview lists every target and its exact requested change. Confirmation
applies to that immutable target list; changes in selection require a new
preview.

Use:

```text
BatchMutation
  parent_task_id
  action_id
  targets
  max_concurrency
  child_outcomes
```

Default mutation concurrency is four, lowered by server rate-limit metadata.
Each target receives its own preflight, dispatch, verification, and outcome.
Cancel stops undispatched children and verification waits, but does not claim to
cancel requests already accepted by the server.

The result view groups `VerifiedSuccess`, `SucceededUnverified`,
`FailedBeforeDispatch`, `OutcomeUnknown`, and `CancelledBeforeDispatch`.
Preserve successes.
“Retry failed” builds a new mutation from freshly fetched targets; there is no
blind retry-all action.

## 06.11 Audit correlation

After verification, query a bounded audit window beginning shortly before
dispatch. Match only documented stable fields: target ID, action class, actor or
credential identity when returned, and time. Store zero, one, or multiple
candidate event IDs.

Audit correlation is supplementary:

- delayed/missing audit never changes verified success to failure;
- an ambiguous match is shown as ambiguous;
- Tale does not fabricate an event ID;
- polling stops after two minutes or cancellation;
- secondary effects absent from audit logs are not claimed.

## 06.12 Action capability and dispatch checks

Every action declares endpoint, scope, selection cardinality, risk, form,
preview, effect, verification read, and resource lock. Capability combines:

- global and profile read-only flags;
- authenticated profile;
- configured and observed scope permission;
- plan/endpoint availability;
- fresh selected resource;
- decision-gate status;
- conflicting tasks.

Recheck all conditions in the reducer immediately before dispatch. A forged
event, stale binding, or visible old overlay cannot bypass them.

Required additional IDs include:

```text
admin.device.rename
admin.device.tags.replace
admin.device.approve
admin.device.revoke_approval
admin.device.key_expiry.configure
admin.device.key_expire_now
admin.device.delete
admin.routes.replace_approvals
admin.dns.preferences.edit
admin.dns.nameservers.replace
admin.dns.search_paths.replace
admin.dns.split.create
admin.dns.split.edit
admin.dns.split.remove
admin.user.approve
admin.user.role.change
admin.user.suspend
admin.user.restore
admin.user.delete
batch.review_outcomes
batch.retry_selected
```

IP/key action IDs are added only if Decision 0002 approves them.

## 06.13 Verification specification

### Unit tests

Cover every form validator, semantic old/new diff, risk tier, confirmation
phrase, preflight comparison, immutable batch target set, resource lock,
verification predicate, outcome-unknown transition, and capability dispatch
guard.

### Contract tests

For each adopted endpoint assert exact method, path, query, headers, body bytes
or canonical body value, accepted status, response decoding, required scope,
no automatic retry, and verification read. Include changed preflight,
permission loss between preview/dispatch, `409`, `412` if documented, `429`,
timeout before/after body write, server error, malformed success, and delayed
verification fixtures.

Use a fake audit endpoint for zero, exact, delayed, and ambiguous correlation.

### Reducer/UI tests

Test editing, validation failure, preflighting, conflict, confirmation,
dispatching, verifying, unknown outcome, partial success, forbidden, read-only,
and stale states at all four reference terminal sizes. Snapshot every Tier 3
confirmation and representative full-replacement DNS/route previews.

### Required commands

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Run the production forbidden-pattern scan, secret canary suite, and all previous
phase tests.

### Manual acceptance journeys

Using fictional HTTP fixtures:

1. Rename and retag a device, verifying canonical server state.
2. Approve a device that still needs a Tailnet Lock signature and see the
   distinction.
3. Cancel key expiration at confirmation, then apply it with typed input.
4. Delete a device after a changed-preflight conflict and a second preview.
5. Replace route approvals on three advertisers with one child failure.
6. Replace ordered nameservers and refresh local DNS independently.
7. Create, edit, and remove a split-DNS mapping with full previews.
8. Approve, change role, suspend, and restore a user.
9. Delete a user only after reviewing the refreshed owned-device set.
10. Time out a mutation, verify the resource, and show either verified or
    outcome-unknown truthfully.
11. Activate `--read-only` after opening a form and prove dispatch is blocked.

## 06.14 Exit gate

Phase 6 is complete only when:

- device, route, DNS, and user mutations are ledger-backed and verified;
- no mutation has an automatic network retry;
- Tier 3 actions use fresh state and typed confirmation;
- batch partial failures remain per-target and recoverable;
- read-only locks are enforced at presentation and dispatch;
- audit delay never controls mutation truth;
- unsupported IP/key semantics remain absent rather than guessed;
- policy and secret-creation writes do not exist;
- all verification and acceptance journeys pass.

### Primary contract sources

- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [Tailscale API](https://tailscale.com/docs/reference/tailscale-api)
- [Device management](https://tailscale.com/docs/features/access-control/device-management)
- [DNS in Tailscale](https://tailscale.com/docs/reference/dns-in-tailscale)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
