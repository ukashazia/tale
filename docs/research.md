# Research and constraints

Research date: 2026-08-03.

This document records the evidence behind Tale's product and architecture. It
is a point-in-time analysis, not a substitute for contract tests against the
minimum Tailscale versions that each Tale release supports.

## Domain findings

A Tailscale installation has two materially different management surfaces:

1. The local client controls the current device, observes reachable peers, and
   performs diagnostics such as `ping`, `netcheck`, DNS queries, Serve, Funnel,
   Taildrop, account switching, and preference changes.
2. The Control API manages tailnet resources such as devices, users, routes,
   DNS, policy, keys, logs, webhooks, and selected feature settings.

The web admin console combines these concepts visually, but it is not a public
API. Tale must preserve the distinction so that it can explain whether data is
local, cloud-derived, stale, forbidden, plan-restricted, or unavailable.

## Supported surfaces

### Local CLI

The documented Tailscale CLI provides the broadest cross-platform local control
surface. Relevant command families include:

- `status`, `ip`, `whois`, `dns`, and `metrics` for inspection;
- `ping` and `netcheck` for connection diagnosis;
- `up`, `down`, `set`, `switch`, `login`, and `logout` for client state;
- `ssh` and `nc` for connections;
- `serve`, `funnel`, `file`, and `drive` for exposing or transferring data;
- `cert`, `bugreport`, `syspolicy`, `lock`, and `update` for specialized tasks.

Tale's pinned LocalAPI contract owns the authoritative status decoding boundary.
Captured status and preference fixtures remain labeled by exact Tailscale
version and platform; unknown fields are preserved by the decoder, and a
failed read never damages the last good snapshot.

### LocalAPI

Tailscale's Go `client/local` source says that methods without an explicit API
maturity note are unstable. The source currently marks status and preferences
methods stable, while the LocalAPI router itself still describes the `/v0/`
surface as an internal implementation detail.

Specification 11 binds only the individually reviewed read contract recorded
in Decision 0004: status, preferences, and `watch-ipn-bus` over the selected
local socket or named pipe. The official CLI remains responsible for
socket-aligned mutations, interactive operations, and commands without an
approved LocalAPI contract.

### Control API

The documented API and trust-credential scopes currently cover these important
resource groups:

| Domain | Read operations | Mutating operations |
| --- | --- | --- |
| Devices | list, details, posture attributes | authorize, expire, delete, rename, assign IP, tags, key operations |
| Routes | advertised and enabled routes | approve or revoke subnet routes and exit-node routes |
| Users | list and details | role, approve, suspend, restore, delete |
| DNS | nameservers, preferences, search paths, split DNS | replace or update each DNS resource |
| Policy file | get, validate, preview | replace policy |
| Credentials | list and inspect supported keys | create auth keys and revoke supported credentials |
| Device invites | list and inspect | revoke where documented; creation remains research |
| Webhooks | list and inspect | create, edit, test, rotate, delete |
| Logs | configuration audit and network-flow logs | network-log settings and stream configuration |
| Tailnet settings | contacts, feature settings, posture integrations | supported contact, feature, and integration updates |

API access tokens are fully privileged and expire within 90 days. OAuth clients
use client credentials, can be scoped, and mint one-hour access tokens. Tale
should prefer scoped OAuth clients for persistent profiles and allow a short-
lived access token for evaluation or emergency use.

### Public-contract gaps

The following console capabilities do not have a sufficiently clear public API
contract in the researched material and cannot be promised yet:

- billing, plan changes, and domain ownership;
- every organization authentication and identity-provider setting;
- every Tailscale Services and discovered-endpoint management operation;
- all Tailnet Lock administration and recovery flows;
- every sharing and user-invitation flow;
- browser-based SSH console behavior;
- parity with alpha features such as OAuth apps.

These remain visible in the product map as `unsupported` or `research`, not as
silent omissions. Tale never scrapes `console.tailscale.com` to fill a gap.

## Prior art

| Product | Useful pattern | Limit to avoid or outgrow |
| --- | --- | --- |
| jjui | graph-first navigation, target pickers, previews, contextual which-key footer, searchable help, command history | its revision graph is domain-specific; Tale must not invent a network graph unsupported by data |
| K9s | command mode, resource views, live filtering, aliases, responsive tables, read-only mode, context-aware actions | very broad customization and plugin surfaces would be premature for Tale |
| Lazygit | focused panels, visible command log, contextual help, predictable list/detail navigation | Git-specific staging concepts do not transfer to network administration |
| Neuralink `tsui` | polished local preference editing, exit-node latency, debug information, login flows | local-client only and last released in 2024 at the time of research |
| `tailTUI` | live latency, command previews, local route staging, event history, CLI adapter | intentionally dense and local-centric; requires a Nerd Font for its intended presentation and performs sudo flows |

Tale's differentiator is not another peer list. It is a coherent operator
workspace spanning local diagnostics and the documented administration plane,
with explicit capability boundaries and safer change workflows.

## Design implications

- Do not merge local and admin records heuristically. Link them only when both
  sources expose the same stable node identifier.
- Never infer that an offline device is unhealthy; offline may be expected.
- Never infer reachability by reimplementing the Tailscale policy engine. Use
  policy validation, preview, and policy tests provided by Tailscale.
- Keep the last successful snapshot visible when refreshes fail, and show its
  age and source.
- A `403` is a capability result, not a generic application failure.
- Plan-restricted, version-restricted, unauthenticated, unsupported, and
  temporarily failed states must render differently.
- Never run a generated command through a shell. Use typed argument vectors.
- Never invoke `sudo` inside Tale. Explain the exact permission problem and let
  the user choose how to resolve it outside the application.
- Do not require icons, TrueColor, or a particular font for comprehension.

## Open research before the relevant phase

These questions are intentionally deferred until their implementation phase:

1. Capture exact pagination, rate-limit, and retry semantics for every Control
   API list endpoint Tale adopts.
2. Establish the minimum supported Tailscale versions from real JSON fixtures,
   including macOS, Linux, and Windows differences.
3. Verify whether policy replacement supports an optimistic concurrency token;
   until verified, Tale must re-fetch and compare before save.
4. Confirm current public API coverage for Services, endpoint collection,
   sharing, invitations, Tailnet Lock, and alpha OAuth apps.
5. Evaluate maintained HuJSON libraries before selecting one. Tailscale's API
   remains the authority for policy validation.
6. Test terminal restore and child-process handoff on all supported platforms.

## Primary sources

- [Tailscale CLI reference](https://tailscale.com/docs/reference/tailscale-cli)
- [Tailscale API](https://tailscale.com/docs/reference/tailscale-api)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [OAuth clients](https://tailscale.com/docs/features/oauth-clients)
- [Tailnet concepts](https://tailscale.com/docs/concepts/tailnet)
- [Device filters](https://tailscale.com/docs/features/access-control/device-management/how-to/filter)
- [Grants syntax](https://tailscale.com/docs/reference/syntax/grants)
- [Tailnet policy files](https://tailscale.com/docs/features/tailnet-policy-file)
- [Configuration audit logs](https://tailscale.com/docs/features/logging/audit-logging)
- [Network flow logs](https://tailscale.com/docs/features/logging/network-flow-logs)
- [Tailscale LocalAPI client source](https://github.com/tailscale/tailscale/blob/main/client/local/local.go)
- [Tailscale LocalAPI router source](https://github.com/tailscale/tailscale/blob/main/ipn/localapi/localapi.go)
- [jjui](https://github.com/idursun/jjui)
- [K9s commands](https://k9scli.io/topics/commands/)
- [K9s custom views](https://k9scli.io/topics/columns/)
- [Ratatui application patterns](https://ratatui.rs/concepts/application-patterns/)
- [Neuralink tsui](https://github.com/neuralink/tsui)
- [tailTUI](https://pkg.go.dev/github.com/Phundahl/tailtui)
