# Product definition

## Promise

Tale lets an operator answer three questions without leaving the terminal:

1. What is the state of my local Tailscale client and tailnet?
2. Why can or cannot a device reach a resource?
3. What administration action can I safely take next?

It should feel faster than repeatedly composing CLI commands and denser than the
web admin console, without hiding the source, scope, or consequences of an
operation.

## Target users

- A personal-tailnet owner who wants a fast peer list, exit-node control,
  Taildrop, Serve/Funnel management, and diagnostics.
- A homelab operator managing subnet routers, DNS, routes, services, tags, and
  expiring devices.
- An infrastructure or IT operator reviewing devices, approvals, users,
  policies, credentials, logs, and fleet health.
- A security operator investigating configuration changes, posture, traffic
  paths, version skew, and least-privilege policy behavior.

Tale assumes familiarity with terminal applications but does not require the
user to memorize Tailscale flags, API endpoints, or every keyboard shortcut.

## Product modes

### Local mode

Always attempted at startup when local integration is enabled. Daemon
observation requires the documented LocalAPI socket or named pipe, but does
not require the Tailscale executable. It presents the local node, reachable
peers, connection paths, and preferences. An installed, supported CLI is
additionally required for accounts, diagnostics, mutations, and other
process-backed actions.

### Admin mode

Enabled by selecting a configured profile. It uses the Control API to present
tailnet-wide resources and actions. It can be read-only either because the
profile is configured that way or because its credentials lack write scopes.

### Combined mode

The default when both sources are available. Local liveness enriches admin
inventory only when the same stable node identifier is present. Source badges
and timestamps remain visible in details.

Neither source is a prerequisite for rendering the other. A daemon outage must
not block admin work, and an expired API credential must not block local work.

## Product principles

1. **Truth before polish.** Every value has a source and freshness state.
2. **Capabilities, not assumptions.** Actions appear enabled only when their
   transport, platform, version, plan, and authorization are available.
3. **Observe first, mutate deliberately.** Browsing is immediate; mutations
   have a preview proportional to their risk.
4. **One mental model.** Lists, details, filters, actions, help, and task output
   behave consistently in every resource view.
5. **Progressive density.** Wide terminals gain an inspector; narrow terminals
   retain every operation through drill-down views.
6. **No hidden execution.** Tale shows the requested change in domain language
   and records its result. Local command execution can additionally show the
   exact argv with secrets redacted.
7. **No counterfeit policy engine.** Tailscale validation and preview results
   are authoritative.
8. **No accidental privilege escalation.** Tale does not run `sudo` and does
   not broaden credentials automatically.
9. **Terminal-native, not terminal-themed web UI.** Keyboard flow, immediate
   filtering, external-editor handoff, copy operations, and bounded history are
   first-class.
10. **Meaning before hue.** Semantic roles consistently identify hierarchy,
    focus, source, freshness, risk, task state, secrets, and redaction; labels
    and symbols preserve every distinction without color.

## Resource model

The UI presents these top-level resources:

| Resource | Meaning | Primary source |
| --- | --- | --- |
| Overview | actionable summary, alerts, recent tasks | composed |
| Local | current client, preferences, accounts, addresses | LocalAPI + CLI where needed |
| Profiles | the local client and each configured admin credential, and which is active | local config + credential store |
| Devices | nodes on the selected source's tailnet | LocalAPI, Control API, or both when they are one tailnet |
| Users | members, roles, status, owned devices | Control API |
| Routes | subnet routes, exit nodes, advertisers, approval state | LocalAPI + Control API |
| DNS | MagicDNS, resolvers, split DNS, search paths, query tool | LocalAPI + CLI diagnostics + Control API |
| Access | policy source, validation, tests, preview, change diff | Control API |
| Services | Serve, Funnel, Taildrive, declared and discovered services | local CLI; API where documented |
| Credentials | auth keys and supported trust credentials | Control API |
| Activity | Tale tasks, audit events, network-flow logs | local + Control API |
| Settings | Tale configuration and supported tailnet settings | local config + Control API |

### Domain identities

- `LocalNodeId` and `LocalPeerId` are values emitted by the local client.
- `DeviceId`, `UserId`, `KeyId`, `WebhookId`, and similar identifiers are
  opaque Control API values.
- `TailnetId` is the API identifier, not the display DNS name.
- Names, hostnames, IP addresses, and email addresses are labels, never primary
  keys.
- A composed device keeps both source records. Tale links them only through an
  exact stable identifier supplied by both sources.

### Capability state

Each view and action resolves to one of:

- `available` — the operation can run now;
- `read_only` — visible, but mutation is disabled by Tale configuration;
- `unauthenticated` — credentials are absent or expired;
- `forbidden` — credentials are valid but lack the required role or scope;
- `unsupported_client` — the installed client lacks the command or output;
- `unsupported_api` — there is no documented API contract;
- `plan_restricted` — Tailscale reports that the plan does not include it;
- `temporarily_unavailable` — a request or daemon operation failed;
- `unknown` — capability has not yet been probed.

The reason is always available in the action menu and help text.

## Feature catalog

Priority meanings:

- **Foundation:** required for the first useful end-to-end release.
- **Core:** required for Tale to satisfy its product promise.
- **Power:** valuable differentiation after the core is reliable.
- **Research:** public contract or product shape is not yet settled.

### Overview and navigation

| Feature | Priority | Contract |
| --- | --- | --- |
| Combined health overview | Foundation | local state, peer counts, admin freshness, approvals, expiring keys, route warnings, failed tasks |
| Inline command line | Foundation | bottom `:` prompt; route by resource name, alias, or saved view; schema completion; never execute arbitrary shell text |
| Contextual filter and sort | Foundation | immediate local filtering; AND across filter terms, OR within multi-values |
| Saved views | Power | named resource, filter, sort, and column selection; no credential data |
| Cross-resource jump | Core | follow owner, tag, route advertiser, audit target, or policy selector |
| View and command history | Foundation | 100-frame back/forward navigation plus 100 successful commands; process-local, bounded, and non-secret |

### Local client

| Feature | Priority | Contract |
| --- | --- | --- |
| Daemon and login state | Foundation | show missing binary, daemon unavailable, logged out, needs login, running, stopped |
| Local addresses and version | Foundation | IPv4, IPv6, DNS name, client version, update availability when exposed |
| Connect and disconnect | Core | preview impact; use `up` or `down`; preserve exact error output |
| Preferences | Core | accept routes/DNS, exit node, LAN access, shields up, SSH server, auto-update, posture reporting, hostname |
| Account switching | Core | list and switch; interactive login/logout/removal use terminal handoff |
| Advertised routing | Core | subnet CIDRs, exit-node advertisement, app connector, peer relay where supported |
| System policy inspection | Power | show applied policies and errors; reload only when supported |
| Client update | Research | platform/version gated; dry-run first; never silently downgrade |

### Devices and connectivity

| Feature | Priority | Contract |
| --- | --- | --- |
| Device inventory | Foundation | dense sortable list; source, liveness, owner/tags, OS, version, IP, last seen |
| Admin filters | Core | online, age, owner/tag, shared, approval, expiry, version, OS, route/exit/SSH/Funnel/signing properties |
| Device inspector | Foundation | identity, addresses, ownership, capabilities, routes, endpoints, posture, key state, source timestamps |
| Connectivity probe | Foundation | stream `tailscale ping` samples; show direct, DERP, peer relay, loss, min/avg/max |
| Netcheck | Core | current network conditions and DERP latency table; cancellable streaming mode |
| DNS query and whois | Core | query from Tale and attach output to the selected target |
| SSH and raw connection | Core | suspend alternate screen, run interactive child, restore terminal on every exit path |
| Rename and tags | Core | preview old/new values; API permission aware |
| Approve or revoke approval | Core | clearly distinguish authorization from Tailnet Lock signing |
| Key expiry operations | Core | distinguish disabling future expiry, expiring now, and reauthentication |
| Remove device | Core | destructive typed confirmation with a fresh preflight fetch |
| Tailnet Lock signing | Research | local CLI plus console-provided material; no recovery-secret handling until explicitly designed |

### Users

| Feature | Priority | Contract |
| --- | --- | --- |
| User inventory and inspector | Core | role, status, identity, device count, last activity where exposed |
| Approve, suspend, restore | Core | show affected devices and action semantics |
| Role change | Core | show current and requested role; require write capability |
| Delete user | Core | high-risk confirmation and fresh membership check |
| Invitations and sharing | Research | implement only after current endpoints and role rules are verified |

### Routes and DNS

| Feature | Priority | Contract |
| --- | --- | --- |
| Route inventory | Core | advertised vs approved, type, CIDR, advertiser, availability, conflicts |
| Route approval | Core | batch preview grouped by device; never confuse advertising with approval |
| Exit-node selection | Core | local operation; show latency and direct/relay state before switch |
| CIDR overlap warnings | Power | deterministic local analysis; warning only, not a claim of failure |
| DNS overview | Core | MagicDNS, nameservers, search paths, split-DNS suffixes, source freshness |
| DNS editing | Core | validate addresses and domains; show complete resulting ordered lists before replace-style API calls |
| DNS diagnostic trail | Power | query result, selected resolver, latency, and related local configuration in one task record |

### Access controls

| Feature | Priority | Contract |
| --- | --- | --- |
| Read-only HuJSON viewer | Core | preserve comments and source text exactly |
| External-editor workflow | Core | secure temporary file, remote-change check, server validation, diff, explicit apply |
| Validation results | Core | structured locations/messages where supplied; raw server response otherwise |
| Policy tests | Core | display built-in test results before save |
| Permission preview | Core | use Tailscale preview endpoint; do not calculate permissions independently |
| Policy history link | Core | correlate audit events and full diffs |
| ACL-to-grants assistance | Research | only if Tailscale exposes a supported conversion contract; never rewrite policy speculatively |
| Access explorer | Power | question-driven wrapper over preview/tests, for example “Can group X reach service Y on 443?” |

### Services and sharing

| Feature | Priority | Contract |
| --- | --- | --- |
| Serve inventory and edit | Core | preview listener, protocol, path, and target; status/reset supported |
| Funnel inventory and edit | Core | mark public exposure prominently; high-risk enable confirmation |
| Taildrop send/receive | Core | progress, conflict choice, cancellation, and explicit destination |
| Taildrive shares | Power | share, rename, unshare, and list local directories |
| HTTPS certificate request | Power | never display private key; user chooses destination path and ownership remains explicit |
| Discovered endpoints | Research | read or manage only through documented surfaces |
| Tailscale Services | Research | model declared service identity separately from a device and verify current API coverage |
| Device sharing | Research | role, invite, revoke, quarantine, and recipient semantics require endpoint verification |

### Credentials, settings, and activity

| Feature | Priority | Contract |
| --- | --- | --- |
| Secure profile credential store | Core | keyring by default; secrets prohibited in config and logs |
| Auth-key creation | Core | display secret exactly once; never persist or place it in task history |
| Credential inventory/revocation | Core | type- and scope-aware; typed confirmation for revocation |
| Configuration audit log | Core | time/action/actor/target filtering and policy diff rendering |
| Network flow log | Power | plan aware; aggregate without implying packet contents are available |
| Webhook management | Power | endpoint/events/status; secret rotation shown once and never logged |
| Log streaming configuration | Power | preview destination and log type; validate before replace |
| Fleet health | Power | version skew, expiring/expired keys, approval queue, stale nodes, relay-heavy peers, route overlaps, posture gaps |
| Export | Power | JSON/CSV to an explicit path; redact secrets; include query and observation timestamp |

## Deliberate non-goals

- Reimplementing Tailscale's coordination server, WireGuard data plane, policy
  compiler, or authentication provider.
- Scraping or automating the admin-console web UI.
- Storing API secrets in TOML, shell history, logs, crash reports, or exports.
- Running commands from user-supplied strings or adding a general plugin shell
  before Tale's typed action model is mature.
- Automatic `sudo`, background privilege helpers, or daemon installation.
- Inferring a live network topology from a list of nodes.
- Billing and plan purchasing.
- Backward-compatibility shims for obsolete Tale configuration or old
  Tailscale output. Each Tale release declares and tests its supported contract.

## Success criteria

Tale is a credible web-admin alternative when an operator can perform every
documented CLI/API-backed daily workflow from Tale, unsupported console-only
features are explicitly enumerated, and the following are true:

- a local-only user gets value without creating credentials;
- an admin can audit before mutating and can see the result afterward;
- no mutation can be triggered by an ambiguous keypress;
- stale and partial data cannot masquerade as current complete data;
- every failure explains the failing source and the next safe action;
- the interface remains complete at 80x24 using drill-down views;
- core workflows require neither a mouse nor a Nerd Font.
