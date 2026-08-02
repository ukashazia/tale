# Specification 08 — Operational depth

- Implementation phase: 8
- JJ change description: `feat: add tailnet operational workflows`
- Depends on: Specifications 01–07 complete
- Produces: fleet findings, flow investigation, webhooks, log streaming, saved
  views, deterministic exports, Access Explorer, and optional mouse support

This phase makes Tale an operator workspace rather than a terminal copy of the
admin console. Derived conclusions must remain distinguishable from facts
returned by Tailscale.

## 08.0 Phase contract

### User-visible result

Operators can identify reproducible fleet risks, inspect bounded network-flow
windows, manage documented webhooks and log-stream destinations, save reusable
query/presentation state, export secret-free evidence, and ask structured
access questions answered by Tailscale's authoritative preview/test APIs.

### In scope

- deterministic fleet-health findings;
- network-flow log retrieval and aggregation;
- webhook inspection, create, edit, test, secret rotation, and deletion;
- supported configuration/network log-stream inspection and management;
- saved views for route/filter/sort/columns;
- deterministic JSON and CSV export;
- structured Access Explorer;
- opt-in mouse parity and large-data UI improvements.

### Explicitly out of scope

- packet capture or packet-content inspection;
- a SIEM, long-term log database, or background collector;
- speculative network topology/reachability graphs;
- local evaluation of policy or health claims from undocumented assumptions;
- notification delivery, webhook receiver hosting, or webhook-signature service;
- arbitrary export templates or user-provided scripts;
- general plugins, custom keybindings, or color themes;
- researched-but-uncontracted Services, sharing, Tailnet Lock, OAuth apps,
  billing, IdP, or client-update orchestration.

### Required new ownership

```text
src/domain/health.rs
src/domain/flow.rs
src/domain/webhook.rs
src/domain/log_stream.rs
src/domain/saved_view.rs
src/domain/export.rs
src/admin/flow_logs.rs
src/admin/webhooks.rs
src/admin/log_streaming.rs
src/admin/access_explorer.rs
src/health.rs
src/export.rs
src/saved_views.rs
src/ui/views/health.rs
src/ui/views/flows.rs
src/ui/views/webhooks.rs
src/ui/views/log_streams.rs
src/ui/views/access_explorer.rs
tests/fixtures/admin/flows/
tests/fixtures/admin/webhooks/
tests/fixtures/admin/log_streaming/
tests/fixtures/health/
tests/fixtures/export/
```

Reuse the Phase-7 ephemeral-secret view for rotated webhook secrets. Reuse
existing filtering, sorting, tasks, mutation, and confirmation machinery.

## 08.1 API contract additions

Extend the ledger before implementing:

| Resource | Method and path | Scope |
| --- | --- | --- |
| network flows | `GET /api/v2/tailnet/{tailnet}/logging/network` | `logs:network:read` |
| webhook list | `GET /api/v2/tailnet/{tailnet}/webhooks` | `webhooks:read` |
| webhook detail | `GET /api/v2/webhooks/{endpoint_id}` | `webhooks:read` |
| webhook create | `POST /api/v2/tailnet/{tailnet}/webhooks` | `webhooks` |
| webhook edit | `PATCH /api/v2/webhooks/{endpoint_id}` | `webhooks` |
| webhook delete | `DELETE /api/v2/webhooks/{endpoint_id}` | `webhooks` |
| webhook test | `POST /api/v2/webhooks/{endpoint_id}/test` | `webhooks` |
| webhook rotate | `POST /api/v2/webhooks/{endpoint_id}/rotate` | `webhooks` |
| stream config | `GET /api/v2/tailnet/{tailnet}/logging/{log_type}/stream` | `log_streaming:read` |
| stream status | `GET /api/v2/tailnet/{tailnet}/logging/{log_type}/status` | `log_streaming:read` |
| set stream | `PUT /api/v2/tailnet/{tailnet}/logging/{log_type}/stream` | `log_streaming` |
| delete stream | `DELETE /api/v2/tailnet/{tailnet}/logging/{log_type}/stream` | `log_streaming` |
| network-log settings | `GET`/`PATCH /api/v2/tailnet/{tailnet}/settings` | `logs:network:read`/`logs:network` |

For each, record exact request/response bodies, supported log types and
destinations, secret fields, URL restrictions, event vocabulary, plan errors,
and response pagination. Private log-stream endpoints require additional scopes
only as documented; Tale must not request them silently.

## 08.2 Fleet-health engine

### Finding contract

Health is a pure function over immutable input snapshots and a supplied clock:

```text
Finding
  id
  rule_id
  severity = Info | Warning | Critical
  title
  observed_facts
  observed_at
  affected_resource_ids
  source_ids
  explanation
  suggested_action_ids
  derived = true
```

Finding IDs are deterministic hashes of rule ID and sorted affected stable IDs,
not names or row positions. Every rendered finding says `Derived by Tale` and
links to its facts. Suggested actions are existing typed action IDs and remain
subject to current capability checks.

### Required rules

Implement these exact first rules:

1. `device-key-expired`: Critical when a documented device key expiry is at or
   before the supplied clock.
2. `device-key-expiring`: Warning when expiry is after now and within seven
   days; the threshold is fixed for 1.0 and not configurable.
3. `device-approval-pending`: Warning for each device whose API state explicitly
   requires approval.
4. `user-approval-pending`: Warning for each user whose API state explicitly
   requires approval.
5. `source-stale`: Warning when an active resource exceeds three times its
   configured refresh interval; Critical only after ten times and at least one
   failed refresh.
6. `source-failed`: Warning for a current failed/forbidden/plan-restricted
   resource, with its actual class; do not call permission failure unhealthy
   infrastructure.
7. `route-overlap-review`: Info when CIDRs advertised by different stable device
   IDs overlap. Include exact CIDRs, advertisers, and approval states. Do not
   label intentional redundancy a conflict.
8. `client-version-skew`: Info when parseable stable client versions span more
   than two minor versions within the same major version. Compare only observed
   tailnet versions; never call a version vulnerable or vendor-unsupported.
9. `posture-observation-missing`: Info only when a posture integration is
   explicitly enabled and a successfully read device has no returned posture
   attributes. This is missing observation, not noncompliance.
10. `relay-heavy-local-peer`: Info when at least five samples in the current
    session exist for a peer and at least 80% report relay rather than direct.
    Clear the rolling samples on source identity change.

Offline age alone never creates a finding. Unknown, forbidden, and absent are
not interchangeable. Rules cannot parse policy to infer required posture,
reachability, or expected online schedules.

### Evaluation

Run evaluation off the UI thread after relevant snapshots change. Coalesce
generations, cap affected IDs per finding at 1,000 with an explicit truncated
count, and sort by severity then rule ID then stable ID. Fixtures supply the
clock and complete snapshots; tests must be deterministic.

Add a Health section to Overview and register
`overview.health.open_resource` / `overview.health.run_suggested_action`. Do not
add a new canonical route.

## 08.3 Network-flow retrieval

### Query

Add a Flow logs tab to Activity. The form requires an explicit UTC time window
and optional local filters. Do not add a new canonical route. Query:

```text
GET /api/v2/tailnet/{tailnet}/logging/network?start=<RFC3339>&end=<RFC3339>
```

The initial window is the previous hour. Start and end are inclusive and must
fall within the service's documented 30-day retention. A single request window
is capped at 24 hours to bound memory; users inspect longer periods as separate
windows. The researched API has no pagination and no documented maximum page
size. Do not add page parameters or call a complete 30-day fetch automatically.

Apply a 64 MiB response cap and 250,000 decoded-message cap. Crossing either
returns a partial-disabled result: do not render incomplete aggregates as
complete. Suggest a narrower time window.

### Domain model

Preserve:

- reporting `nodeId` and server `logged` time;
- node-recorded start/end times;
- embedded source/destination node IDs, names, addresses, OS, user/tags when
  returned;
- virtual, subnet, exit, and physical traffic as distinct classes;
- protocol number, source/destination address/port when present;
- transmit/receive packet and byte counters.

Node-recorded fields are observations and may be inaccurate or spoofed. The UI
must identify the reporting node and never present a resolved current name in
place of the raw stable ID. Missing exit-traffic destinations remain missing;
do not infer them.

Flow logs describe metadata and counters, not packet contents. State this in
the route header and help.

### Filtering and aggregation

Support structured filters for time, reporting/source/destination node ID or
resolved label, traffic class, protocol, address, port, and minimum byte count.
Resolve current devices through exact stable IDs while retaining raw values.

Provide raw-message and aggregate modes. Aggregate only by explicitly selected
dimensions and sum counters with checked arithmetic; overflow is an error, not
saturation. Display clock-skew caveats when node times fall outside the query
window while server logged time is inside.

Filtering/aggregation runs as cancellable generation-owned work. Rendering uses
virtualized rows and never clones the full result per frame.

## 08.4 Webhook inventory and editor

### Domain and list

`WebhookEndpoint` contains stable endpoint ID, HTTPS URL, destination type,
subscribed categories/events, status/last result when documented, creation and
update metadata, and source freshness. Secret values never appear in inventory.

The editor populates event categories and event identifiers returned by the
current API contract. Preserve unknown subscriptions so an edit cannot silently
remove a newly introduced event. Categories subscribe to future category events
by server semantics; state that consequence in preview.

### Validation

Accept only endpoint URLs permitted by the public contract: HTTPS and documented
ports. Reject embedded credentials, fragments, control characters, and invalid
hosts. Do not preflight the destination from Tale; the server's test endpoint is
the supported test.

### Mutations

Register:

```text
admin.webhook.create
admin.webhook.edit
admin.webhook.test
admin.webhook.rotate_secret
admin.webhook.delete
```

Create/edit are Tier 2 with full old/new URL, destination, category, and event
preview. Test is Tier 1 and reports only server result. Rotation and deletion
are Tier 3 with typed endpoint label/ID. No mutation is retried.

Create and rotation may return a signing secret. Move it directly into the
Phase-7 `SecretResult`; never log, persist, or include it in the mutation task.
If secret decoding fails, do not retry. Closing makes it unrecoverable.

After create/edit/delete, verify list/detail. Test delivery is asynchronous;
render the API acknowledgement and refreshed status without claiming the
destination processed the event unless the status endpoint says so.

## 08.5 Log-stream configuration

Register the `log_streams` subsection under Activity or Settings rather than a
new top-level route. Keep `configuration` and `network` log types distinct.

### Inventory

Fetch stream configuration and status independently for each documented log
type. Domain values include destination kind, non-secret destination identity,
enabled/configured state, health/status, last observation, and fields omitted
because they are write-only secrets.

### Editor

Build typed forms only for destination kinds completely described in the
ledger. Do not offer a raw JSON body. Secret inputs live only in form state,
use non-revealing types, and are destroyed on cancel/dispatch. An unchanged
write-only secret is represented by `KeepExisting` only when the API documents
partial preservation; otherwise require an explicit replacement.

Private endpoints display the additional policy/device-invite scope and
configuration prerequisites documented by Tailscale. Tale does not edit policy
as a side effect.

`admin.log_stream.replace` is Tier 2; `admin.log_stream.delete` is Tier 3.
Preview complete replacement semantics, destination, log type, secret-field
actions (`keep`, `replace`, never the value), and impact. Submit once and verify
through config/status reads.

Network-flow collection settings are a separate `admin.network_logs.settings`
action using the documented tailnet-settings fields. Never replace unrelated
tailnet settings; require a ledger-proven partial PATCH body.

## 08.6 Saved-view storage and schema

### State file

Store saved views in the platform state directory as `saved-views.toml`, not in
the main credential/profile configuration. Use an atomic same-directory write
and user-only permissions where supported.

Initial document:

```toml
version = 1

[[views]]
name = "production-linux"
route = "devices"
wide_columns = false
columns = ["name", "owner", "version", "last_seen"]

[[views.filters]]
field = "tag"
operator = "equals"
value = "tag:production"

[[views.filters]]
field = "os"
operator = "equals"
value = "linux"

[[views.sort]]
field = "last_seen"
direction = "descending"
```

### Domain rules

A saved view contains only:

- unique user-chosen name;
- canonical route;
- structured filter clauses using registered field/operator/value types;
- stable sort clauses;
- registered column IDs and wide-column preference.

It never stores selected resource IDs, rows, source snapshots, tailnet/profile,
credentials, time-relative resolved values, overlay state, or task state.

Register create, replace, rename, delete, and apply actions. Replace/delete are
Tier 1 with previews. Applying validates every route/field/operator/column
against the current registry. An invalid document or removed field fails with a
precise error; do not migrate, alias, drop, or reinterpret it.

The command palette lists saved views as `view:<name>`. Duplicate names are
errors; case sensitivity matches profile naming.

## 08.7 Deterministic export

### Supported resources

Export only active collections with explicit schema implementations: Devices,
Users, Routes, DNS, Credentials metadata, Audit, Health findings, and Flow logs.
Policy source/diff, secret overlays, tasks with output, settings, forms, and raw
HTTP data are not exportable.

Register `collection.export`. The form selects JSON or CSV and an explicit
path. Parent must exist and be writable. Existing targets require Tier 2
overwrite confirmation. Write to a same-directory temporary file, flush, then
atomically replace only after successful serialization.

### Query contract

Export the exact active filtered/sorted collection generation. If its source
changes before dispatch, require a refreshed preview. Include:

- schema name and integer version;
- Tale version;
- source identities and observation timestamps;
- canonical route;
- structured active filter and sort;
- truncation/completeness status;
- deterministic rows.

JSON uses a top-level metadata object and `rows` array with a fixed field order
from the schema implementation. CSV uses fixed columns beginning with
`_row_kind`, `_schema`, `_observed_at`, `_sources`, `_filter`, and `_sort`.
Write one metadata row (`_row_kind=metadata`) even for an empty collection, then
data rows. Document this Tale CSV dialect.

Serialize timestamps as UTC RFC3339, IDs as strings, address/CIDR values in
canonical form, and nested sets/lists as compact deterministic JSON strings in
CSV. Sort map/set values before serialization. Use checked counters.

### Redaction

Each export schema is an allowlist. Never recursively serialize domain structs.
Exclude secrets, policy source, raw old/new audit bodies, authorization headers,
private keys, webhook/log-stream write-only fields, and unreviewed unknown
fields. Audit export includes only explicitly redacted typed fields.

Tests serialize the same snapshot repeatedly and require byte-identical output
except where the fixture supplies a different explicit export timestamp.

## 08.8 Access Explorer

Register `access_explorer` as a subsection of Access. A question contains only
fields supported by the public preview/test contract:

```text
AccessQuestion
  source_selector
  destination_selector
  protocol_or_port?
  ssh_user?
  application_capability?
  policy_source = CurrentRemote | ActiveCandidate
```

Unavailable dimensions are omitted from the form rather than evaluated
locally. Translate the question into one documented preview or validation/test
request. Associate results with policy hash, source input, request time, and
server limitations.

Render `Allowed`, `Denied`, or `Indeterminate` only when those states are
expressly supported by the server response. Show matched rules/capabilities and
locations when returned. Do not turn an empty/malformed/forbidden result into a
deny. Do not build a speculative full graph or probe live destinations.

Phase-7 policy workflow may open Explorer against its current candidate, but
changing the candidate invalidates the result.

## 08.9 Mouse and UI depth

Mouse remains disabled unless `[ui].mouse = true`. When enabled, support:

- focus on click;
- row selection on click;
- wheel scrolling in the focused scrollable region;
- scrollbar/section selection where unambiguous;
- activation only through the same action IDs used by keyboard input.

Click selects; double-click or an explicit action affordance opens. Mouse input
cannot bypass capability, risk, or confirmation. Every mouse journey has a
keyboard equivalent and help lists keyboard bindings first.

Add column selection only through registered standard/wide column IDs and saved
views. Large collections use viewport rendering and background filter/sort
generations. Improve help search and task filtering without adding key or theme
configuration.

## 08.10 Required action IDs

```text
overview.health.open_resource
overview.health.run_suggested_action
activity.flows.select_window
activity.flows.aggregate
activity.flows.open_device
admin.webhook.create
admin.webhook.edit
admin.webhook.test
admin.webhook.rotate_secret
admin.webhook.delete
admin.log_stream.replace
admin.log_stream.delete
admin.network_logs.settings
saved_view.create
saved_view.replace
saved_view.rename
saved_view.delete
saved_view.apply
collection.export
access_explorer.ask
access_explorer.open_rule
```

## 08.11 Verification specification

### Unit tests

Cover every health rule at exact thresholds, deterministic IDs/sorting,
CIDR-overlap edge cases, version parsing, relay sample reset, flow query bounds,
checked aggregation, raw-ID resolution, webhook URL/event preservation,
log-stream secret state, saved-view strict decoding, registry validation, export
field allowlists/order/CSV escaping, Access Explorer indeterminate behavior,
and mouse-to-action equivalence.

### Contract tests

Assert all ledger fields for flow, webhook, stream, status, and settings
endpoints. Include plan restriction, forbidden subresource, 64 MiB/record caps,
no-pagination flow behavior, webhook secret create/rotate canaries, asynchronous
test status, unknown event subscriptions, write-only stream secrets, partial
PATCH body isolation, mutation timeouts, and verification reads.

### Performance tests

Use deterministic fixtures with 5,000 devices, 50,000 findings/rows where
appropriate, and a representative 250,000-message flow boundary. Measure
filter/sort/aggregate work separately from rendering and assert cancellation and
bounded queues/memory using stable thresholds documented for the test host.
Avoid wall-clock assertions so tight that ordinary CI jitter causes failures.

### Export/filesystem tests

Use temporary directories. Test JSON/CSV golden files, empty metadata row,
special characters/newlines, existing-file confirmation, short writes/failure,
atomic replacement, stale generation, redaction canaries, and byte determinism.

### UI tests

Snapshot Health, raw/aggregate Flows, Webhooks, Log streams, saved-view picker,
export preview, Access Explorer, and mouse-disabled/enabled help at all reference
sizes. Include large-result truncation and plan/permission limitations.

### Required commands

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Run every earlier forbidden-pattern, secret-canary, terminal, and contract test.

### Manual acceptance journeys

With fictional snapshots and API fixtures:

1. Open each deterministic finding and inspect its exact observed facts.
2. Query one hour of flows, filter raw IDs, aggregate bytes, and cancel a newer
   generation.
3. Hit the flow body cap and receive a narrower-window instruction rather than
   an incomplete aggregate.
4. Create a webhook, copy its secret once, test it, edit subscriptions while
   preserving an unknown event, rotate, then delete with typed confirmation.
5. Replace and remove a typed log stream without exposing secret input.
6. Save, apply, rename, and delete a structured Devices view.
7. Reject a saved view whose registered field was removed.
8. Export filtered Devices and an empty Health view as deterministic JSON/CSV.
9. Ask an access question and render only the server-authoritative answer.
10. Complete representative navigation with mouse off, then with opt-in mouse,
    proving identical action checks.

## 08.12 Exit gate

Phase 8 is complete only when:

- every finding is deterministic, source-linked, and labeled derived;
- flows never imply packet contents and remain bounded/cancellable;
- webhook and log-stream mutations are ledger-backed and secret-safe;
- rotated/created webhook secrets use Phase-7 view-once guarantees;
- saved views contain query/presentation state only and receive no migrations;
- exports are allowlisted, deterministic, metadata-bearing, and secret-free;
- Access Explorer reports only Tailscale-authoritative results;
- all keyboard functionality remains available without mouse;
- representative maximum fixtures remain responsive;
- all verification and acceptance journeys pass.

### Primary contract sources

- [Network flow logs](https://tailscale.com/docs/features/logging/network-flow-logs)
- [Webhooks](https://tailscale.com/docs/features/webhooks)
- [Logging, streaming, and events](https://tailscale.com/docs/reference/logging-streaming-events)
- [Log streaming](https://tailscale.com/docs/features/logging/log-streaming)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [Manage tailnet policies](https://tailscale.com/docs/features/tailnet-policy-file/manage-tailnet-policies)
