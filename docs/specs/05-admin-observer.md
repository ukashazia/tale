# Specification 05 — Admin observer

- Implementation phase: 5
- JJ change description: `feat: add read-only tailnet administration`
- Depends on: Specifications 01–04 complete
- Produces: authenticated, read-only Control API inventory and combined views

This is the Phase 5 contract from the
[end-to-end feature plan](../roadmap.md). The endpoint and scope inventory was
checked against Tailscale's public API and trust-credential documentation on
2026-08-03. The implementation must re-check the interactive API contract when
choosing DTO fields and commit fictional fixtures for the selected contract.

## 05.0 Phase contract

### User-visible result

An operator can configure a scoped admin profile and inspect the selected
tailnet's devices, users, routes, DNS configuration, policy source, credential
metadata, settings, and configuration audit events. The local source remains
independently usable when the profile is absent or the API fails.

### In scope

- profile selection and OS-keyring-backed credentials;
- OAuth client-credentials exchange and ephemeral access-token override;
- a bounded, endpoint-specific HTTPS client;
- independent admin resource snapshots and capabilities;
- read-only Devices, Users, Routes, DNS, Access, Credentials, Activity, and
  Settings content;
- exact-ID composition of local and admin device records;
- combined Overview queues derived from observed facts;
- admin-only operation through `--no-local`.

### Explicitly out of scope

- every Control API mutation;
- browser login, OAuth authorization-code flow, or OAuth apps;
- federated workload identity;
- Headscale or custom API base URLs;
- policy parsing, normalization, editing, validation requests, or preview;
- network-flow logs, webhooks, log streaming, saved views, and exports;
- credential creation, remote revocation, or secret display;
- console scraping and undocumented endpoints.

No Control API write method may exist in production code during this phase.
The HTTP test transport may accept arbitrary methods only inside tests.

### Required code ownership

Add only modules used by this phase:

```text
src/admin/mod.rs
src/admin/auth.rs
src/admin/client.rs
src/admin/dto.rs
src/admin/devices.rs
src/admin/users.rs
src/admin/routes.rs
src/admin/dns.rs
src/admin/policy.rs
src/admin/credentials.rs
src/admin/audit.rs
src/domain/user.rs
src/domain/dns.rs
src/domain/policy.rs
src/domain/credential.rs
src/domain/activity.rs
src/ui/views/users.rs
src/ui/views/routes.rs
src/ui/views/dns.rs
src/ui/views/access.rs
src/ui/views/credentials.rs
tests/fixtures/admin/
```

`admin/dto.rs` contains transport DTOs only. Endpoint modules perform DTO to
domain conversion. UI modules never receive access tokens, response bodies, or
HTTP client types.

## 05.1 Public-contract ledger

Before implementing an endpoint, add it to
`docs/contracts/control-api-2026-08-03.md` with:

- operation name, method, and path template;
- required scope;
- request query/header contract;
- success status and content type;
- response fixture source date;
- pagination or explicit lack of pagination;
- documented errors and rate-limit metadata;
- fields Tale consumes;
- link to the public Tailscale source.

The ledger is evidence, not generated client code. If the interactive API docs
and trust-scope table disagree, do not implement that endpoint until the
disagreement is resolved. Do not infer request bodies from the web console.

### Read endpoint inventory

The initial ledger must cover at least:

| Resource | Method and path | Required scope |
| --- | --- | --- |
| devices | `GET /api/v2/tailnet/{tailnet}/devices` | `devices:core:read` |
| device | `GET /api/v2/device/{device_id}` | `devices:core:read` |
| posture | `GET /api/v2/device/{device_id}/attributes` | `devices:posture_attributes:read` |
| routes | `GET /api/v2/device/{device_id}/routes` | `devices:routes:read` |
| users | `GET /api/v2/tailnet/{tailnet}/users` | `users:read` |
| user | `GET /api/v2/user/{user_id}` | `users:read` |
| nameservers | `GET /api/v2/tailnet/{tailnet}/dns/nameservers` | `dns:read` |
| DNS preferences | `GET /api/v2/tailnet/{tailnet}/dns/preferences` | `dns:read` |
| search paths | `GET /api/v2/tailnet/{tailnet}/dns/searchpaths` | `dns:read` |
| split DNS | `GET /api/v2/tailnet/{tailnet}/dns/split-dns` | `dns:read` |
| policy source | `GET /api/v2/tailnet/{tailnet}/acl` | `policy_file:read` |
| credential list | `GET /api/v2/tailnet/{tailnet}/keys` | credential-specific read scope or `all:read` |
| credential detail | `GET /api/v2/tailnet/{tailnet}/keys/{key_id}` | credential-specific read scope |
| settings | `GET /api/v2/tailnet/{tailnet}/settings` | applicable feature read scope |
| contacts | `GET /api/v2/tailnet/{tailnet}/contacts` | `account_settings:read` |
| audit events | `GET /api/v2/tailnet/{tailnet}/logging/configuration` | `logs:configuration:read` |

Do not request `all` or `all:read` automatically. A narrow profile may expose
only part of this table; partial capability is expected.

## 05.2 Profile and credential model

### Configuration

Implement the existing `[profiles.<name>]` schema exactly. A `Profile` has:

```text
name
tailnet_id
read_only
credential_reference
```

Profile names and precedence follow `configuration.md`. `tailnet_id` is the API
identifier or `-`, never a console display label. The header shows the selected
profile, tailnet ID, profile read-only state, and admin freshness.

Register `profile.select` and `profile.clear`. Switching profile cancels every
request owned by the previous profile, increments all admin generations, drops
its in-memory token, and preserves its last snapshot only in profile-keyed
state. Results from the old generation cannot enter the active view.

### Credential records

The OS keyring service is `tale`; the account is the configured credential
reference. Store one versioned record containing either:

```text
OAuthClientRecord
  version = 1
  client_id
  client_secret
  requested_scopes

AccessTokenRecord
  version = 1
  access_token
```

Secret-bearing values must use a reviewed secrecy/zeroizing container and must
not implement revealing `Debug`, `Display`, serialization, equality failure
messages, or cloning without a documented reason. Configuration contains only
the reference and non-secret profile values.

### Environment override

`TALE_ACCESS_TOKEN` applies only to the selected profile for the process
lifetime. It outranks the keyring record, is never persisted, is redacted from
diagnostics, and is dropped on profile change and shutdown. With `--mock`, the
variable is an argument error. With no selected profile, it is an argument
error rather than an implicit `-` profile.

## 05.3 Authentication commands

### `tale auth add PROFILE`

The command runs outside the TUI and reads secrets from a non-echoing terminal
prompt. It accepts `oauth_client` or `access_token`; secrets are never accepted
as command-line flags.

Required transaction:

1. Parse and validate the intended profile and config without writing.
2. Prompt for credential kind and secret fields.
3. Validate the credential against Tailscale.
4. Write the versioned keyring record.
5. Atomically write the profile configuration.
6. If step 5 fails, remove only the record written in step 4.
7. Clear secret buffers and print a non-secret result.

Validation must use the selected tailnet and a read endpoint permitted by the
requested scopes. A valid narrow credential is not rejected because unrelated
endpoints return `403`.

### `tale auth status [PROFILE]`

Report profile existence, credential kind, requested OAuth scopes, keyring
availability, and a live authentication result. Never print the client secret,
access token, bearer header, or token response.

### `tale auth remove PROFILE`

The preview separately names removal of the keyring record and removal of the
profile configuration. Neither operation revokes a remote credential. Missing
keyring records are reported precisely and do not cause unrelated records to be
deleted.

Add contract tests for success and failure at every transaction step.

## 05.4 OAuth token lifecycle

Use the client-credentials flow against:

```text
POST https://api.tailscale.com/api/v2/oauth/token
Content-Type: application/x-www-form-urlencoded
```

Send the client ID, client secret, `grant_type=client_credentials`, and the
space-delimited requested scopes as separate encoded fields. Use TLS defaults;
do not follow a redirect that changes origin while an authorization secret is
present.

Record access token and server-provided expiry only in memory. OAuth access
tokens currently expire after one hour. Refresh five minutes before expiry,
coalesce simultaneous refresh requests per profile, and allow a single
authentication replay of an idempotent read only when the original token is
expired and a newly minted token was obtained. This is not a general request
retry.

An invalid client, denied scope, malformed token response, clock anomaly, or
keyring failure becomes a distinct authentication state. Never continue using
a token after its known expiry.

## 05.5 HTTP client foundation

### Request ownership

Use the fixed origin `https://api.tailscale.com`. URL path segments and query
values must be encoded structurally. Send access tokens only as
`Authorization: Bearer <token>`. The User-Agent identifies Tale and its version
without device/user data.

Every endpoint method declares:

- HTTP method and path builder;
- accepted success status codes and content type;
- typed response decoder;
- timeout and response-body byte cap;
- required scope;
- pagination contract;
- whether a read retry is safe;
- redaction policy.

The domain layer cannot call a generic public `request_json(method, path,
body)` function. Generic mechanics may be private inside `admin/client.rs`.

### Response metadata

Return safe metadata with each decoded response:

```text
ResponseMeta
  request_id?
  observed_at
  status
  rate_limit?
  page_count
```

Capture the documented Tailscale request identifier and rate-limit fields when
present. Absence is valid.

### Error classes

Classify, without relying only on message text:

```text
Unauthenticated
Forbidden
PlanRestricted
NotFound
ValidationFailed
Conflict
RateLimited { retry_at? }
ServerFailure
Transport
TimedOut
Cancelled
UnexpectedStatus
DecodeFailed
BodyTooLarge
```

Store at most 64 KiB of a redacted error body. HTML errors render as bounded
plain text, never as markup. A `403` changes only the endpoint capability that
observed it.

### Reads and retries

Retry idempotent reads at most twice for transport failure, `429`, or documented
transient server responses. Honor server retry metadata, otherwise use capped
exponential backoff with jitter. Cancellation interrupts the wait. Do not retry
`401`, `403`, validation, decode, or body-cap errors.

### Pagination

Pagination is endpoint-specific. Implement only the continuation token, cursor,
link, or page parameter documented in the contract ledger. Do not send generic
pagination parameters to an endpoint that does not document them. Enforce a
maximum of 100 pages and 50,000 decoded records per refresh. A repeated cursor
is a protocol error.

Tests must prove multi-page success, cancellation between pages, duplicate-ID
handling, repeated-cursor rejection, limit enforcement, and the explicit
single-response behavior of non-paginated endpoints.

## 05.6 Admin resource state

Each resource owns independent state:

```text
AdminResource<T>
  profile
  generation
  state = Idle | Loading | Ready | Stale | Forbidden | PlanRestricted |
          Unsupported | Failed
  snapshot?
  observed_at?
  error?
```

Successful decoding atomically replaces that resource. A failed refresh keeps
the last successful snapshot as stale. An authentication failure marks all
resources requiring that token unauthenticated but does not alter local state.
A forbidden endpoint does not prevent other admin refreshes.

The admin refresh interval defaults to the configuration contract. Manual `R`
refreshes the current view's sources; `r` refreshes the selected resource.
Superseded requests are cancelled and stale generations discarded.

## 05.7 Device inventory and composition

### Domain model

Decode documented fields into `AdminDevice`, including when available:

- stable device/node ID;
- machine and DNS names;
- Tailscale addresses;
- OS and client version;
- creation, last-seen, key-expiry, and online observations;
- owner/creator identity or tags;
- approval/authorization state using current API terminology;
- ephemeral, update, sharing, and key-expiry behavior;
- advertised/approved routes;
- posture attributes fetched from their endpoint.

Do not make undocumented fields required. Preserve timestamps in UTC and format
only in the UI.

### Fetch plan

Fetch the tailnet device list first. Fetch details, routes, or posture lazily
when the inspector or a filter needs them, using a bounded concurrency of eight
requests. Cache only for the resource refresh generation. Avoid an automatic
N-plus-one full-tailnet fanout on every timer tick.

### Composition

Compose `LocalDevice` and `AdminDevice` only when both expose the exact same
stable node ID. Names, addresses, hostnames, and users are never identity
fallbacks. A combined record preserves field-level source and observation time.
Unmatched records remain visible.

### Views

Extend Devices rather than creating an Admin Devices route. Add source-aware
columns and filters for approval, key expiry, owner/tag, route role, OS, client
version, sharing, and posture presence only when returned data supports them.
Unknown is distinct from false.

## 05.8 Users and routes

### Users

Register the `users` route with collection and inspector. `AdminUser` includes
stable ID, login/display name, role, status, creation/last-seen observations,
and ownership relationships when documented. Never infer a user's role from
their actions or email domain.

Cross-resource jumps select devices by exact owner ID. Missing device
permission produces a partial view, not an empty ownership claim.

### Routes

Register the `routes` route. For each device inspected through the documented
routes endpoint, distinguish:

- advertised subnet routes;
- approved/enabled subnet routes;
- exit-node advertisement;
- exit-node approval;
- device/source freshness.

The route domain value validates CIDRs but preserves the server's canonical
string. Never call a local advertisement an admin approval. Route inventory may
be incomplete until device route details have been loaded; label that state.

## 05.9 DNS

Register the `dns` route with separate admin configuration and local diagnostic
sections. Fetch nameservers, preferences, search paths, and split DNS
independently so a forbidden feature does not erase the others.

Domain values preserve server order and distinguish:

- global nameservers;
- restricted/split nameserver mappings;
- search paths;
- MagicDNS/override preferences only when present in the documented response.

Do not convert nameserver strings into IP addresses when the API permits a
documented non-IP resolver form. Render exact configured values with type and
source. Phase 5 contains no edit form.

## 05.10 Policy source and credentials

### Access view

Register the `access` route and request the policy in its documented HuJSON
representation. Preserve the exact response bytes, line endings, comments, and
trailing newline. Store:

```text
PolicySnapshot
  source_bytes
  content_type
  fetched_at
  content_hash
```

Render bounded syntax-colored text without parsing or reserializing it. Copy is
explicit and includes the full source only after a privacy notice; policy
source is excluded from persisted tasks and logs.

### Credentials view

Register `credentials`. Fetch only the credential kinds allowed by the
profile's scopes. Show non-secret metadata such as type, ID, description/owner,
scopes/tags, creation, expiry, revocation state, and last-used data when the API
documents it. Unknown fields remain unknown.

The API's `all:read` capability to list all access tokens must not cause Tale to
request that broad scope. With narrower scopes, show the resource subsets that
can be read and an explicit partial-inventory message.

## 05.11 Settings, contacts, and audit events

### Settings

The Settings route gains profile details, requested scopes, observed endpoint
capabilities, tailnet settings, contacts, and posture-integration metadata when
permitted. It remains read-only. Do not expose undocumented organization,
billing, domain, or identity-provider settings.

### Configuration audit

Activity gains an Admin audit tab. Query:

```text
GET /api/v2/tailnet/{tailnet}/logging/configuration?start=<RFC3339>&end=<RFC3339>
```

The initial default window is the previous 24 hours ending at refresh start.
The user may select a window within the service's documented 90-day retention.
`start` and `end` are explicit UTC RFC3339 values and inclusive.

Decode actor, action, target, event-group identity, timestamp, and old/new data
or policy diff when documented. Events are rendered in server order with a
stable tie-breaker. Read-only activity is absent by server design; say so in
help. Audit delivery can be delayed and is never treated as complete real-time
state.

Bound a refresh to 50,000 events and the configured task body cap. The
researched endpoint returns the requested window rather than generic pages; do
not add page parameters unless the ledger later documents them.

## 05.12 Combined Overview and capabilities

Overview displays Local and Admin as separate source cards. Admin queues are
pure derivations over current snapshots:

- devices awaiting approval;
- users awaiting approval;
- expired and soon-expiring device keys;
- advertised routes not approved;
- failed, forbidden, plan-restricted, and stale resources;
- observed client-version groups.

These are read-only queues. Offline age is informational, not a health error.
Version skew is a distribution until Phase 8 defines a finding policy.

Capability is evaluated per action/endpoint from configured scopes, global and
profile read-only locks, authentication, observed endpoint responses, and
selected resource. A configured scope expresses intent; only a successful or
denied request establishes observed availability. Help may list later mutation
concepts as unavailable explanations, but no executable mutation action is
registered.

## 05.13 Required action IDs

Register at minimum:

```text
profile.select
profile.clear
admin.refresh.current
admin.refresh.all
view.users
view.routes
view.dns
view.access
view.credentials
users.open.devices
routes.open.device
dns.open.local_diagnostics
access.copy_source
activity.select_window
activity.open_actor
activity.open_target
settings.inspect_capabilities
```

All are navigation, read, or explicit copy actions. None has a Control API
mutation effect.

## 05.14 Verification specification

### Unit tests

Cover configuration precedence, profile switching generations, secret-safe
traits, OAuth expiry/refresh coalescing, URL encoding, error classification,
rate-limit parsing, pagination limits, independent resource states, exact-ID
composition, route distinctions, ordered DNS conversion, policy-byte
preservation, audit-window construction, and Overview derivations.

### HTTP contract tests

Use a deterministic local fake HTTP server injected through test-only
construction. For every endpoint assert method, encoded path, query, headers,
absence of secret data in URL/log/error, accepted status, body cap, decoder,
scope metadata, cancellation, and pagination behavior. Fixtures use reserved
domains, documentation address ranges, fictional IDs, and fixed timestamps.

Add negative fixtures for `401`, endpoint-specific `403`, plan restriction,
`404`, `429` with and without retry metadata, `5xx`, malformed JSON, unexpected
content type, oversized body, timeout, and cancellation.

### Keyring and CLI tests

Use an in-memory fake keyring. Test every failure point in `auth add` rollback,
status redaction, remove choices, missing records, environment override,
non-echoing prompt cancellation, and `--mock` exclusion. No test touches the
operator's keyring.

### Reducer and UI tests

Snapshot all completed views at 60x18, 80x24, 110x30, and 160x45 with full,
partial, empty, stale, forbidden, plan-restricted, unauthenticated, and
admin-only states. Prove local views remain usable during every admin failure.

### Required commands

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

Also run the production-source scan required by earlier specifications and a
secret canary test that fails if a planted token appears in any trace, task,
error, snapshot, or persisted file.

### Manual acceptance journeys

Using fake keyring and HTTP transports:

1. Add a narrow OAuth profile transactionally and start in `--no-local` mode.
2. Observe token exchange, reuse, refresh, and secret-free task detail.
3. Browse devices and lazily enrich one with routes and posture.
4. Compose one exact-ID local/admin device while leaving two unmatched.
5. Browse users, routes, DNS, policy, credential metadata, settings, and audit.
6. Deny DNS with `403` while Devices and Local continue to refresh.
7. Cancel a multi-page list and retain its previous snapshot as stale.
8. Switch profiles while requests are running and reject old-profile results.
9. Remove a Tale keyring record without claiming remote revocation.
10. Run with an ephemeral environment token and prove it is never persisted.

## 05.15 Exit gate

Phase 5 is complete only when:

- Tale is useful in authenticated `--no-local` mode;
- every resource has independent freshness, permission, and failure state;
- local operation survives all admin authentication and transport failures;
- endpoint methods, paths, scopes, fields, and pagination are ledger-backed;
- tokens and client secrets cannot enter URLs, rendering, logs, tasks, errors,
  debug output, configuration, or fixtures;
- exact stable IDs are the only local/admin composition key;
- no Control API mutation can be constructed or dispatched;
- all verification and acceptance journeys pass.

### Primary contract sources

- [Tailscale API](https://tailscale.com/docs/reference/tailscale-api)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [OAuth clients](https://tailscale.com/docs/features/oauth-clients)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
- [Tailnet policy file](https://tailscale.com/docs/features/tailnet-policy-file)
