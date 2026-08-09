# Decision 0001: local preference transport

Status: accepted

Date: 2026-08-03

## Context

The local operator needs an authoritative, current preference read before it
can preview or verify any preference mutation. A failed or incomplete read
must remain distinguishable from a false preference value. The transport must
also be narrow enough that Tale does not depend on undocumented LocalAPI
surface or scrape human-oriented command output.

The local mutation contract is the Tailscale CLI documented for the 1.98.9
client family. Preference fixtures and compatibility checks therefore use
that version's wire field names and response shape. A version is not treated
as supported merely because its output happens to look similar.

## Decision

Tale uses the individually documented `local.Client.GetPrefs` contract from
the Tailscale client library as its preference-read transport:

```text
GET /localapi/v0/prefs
```

The request is made to the platform's documented local daemon transport, with
the fixed LocalAPI host `local-tailscaled.sock`. Tale implements only this
single read operation. It does not expose a generic LocalAPI client and does
not use any other LocalAPI endpoint.

The supported transport boundary is:

- Linux: the documented default Unix socket
  `/var/run/tailscale/tailscaled.sock`.
- Windows: the documented Tailscale named pipe
  `\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled`.
- macOS standalone Tailscale: the documented Unix socket
  `/var/run/tailscaled.socket`.

Tale does not claim support for the Mac App Store GUI transport. That client
variant discovers a per-user localhost port and proof token, which is a
different transport and is not a stable, individually documented preference
contract for this product. On that platform variant, preference controls are
reported as unavailable rather than trying another transport.

The request is bounded and read-only. Tale validates the HTTP status, decodes
the JSON object into a dedicated preference DTO, ignores additive unknown
fields, and rejects malformed or otherwise unsupported responses. The
implementation reports transport, permission, timeout, daemon, and decode
failures as distinct non-success outcomes. None of those outcomes is
converted into `false`, an empty route set, or an empty account state.

For the 1.98.9 compatibility fixture, Tale sends the capability header used
by that client family (`Tailscale-Cap: 138`) along with the fixed LocalAPI
host. Supporting a different capability version requires a new compatibility
fixture and an explicit contract review; there is no version-guessing or
fallback header behavior.

## Preference field mapping

The DTO maps only fields used by Tale's local preference controls:

| Operator field | LocalAPI preference field | Missing-value rule |
| --- | --- | --- |
| running intent | `WantRunning` | missing is unknown |
| logged-out/account state | `LoggedOut` | missing is unknown |
| accept DNS | `CorpDNS` | missing is unknown |
| accept routes | `RouteAll` | missing is unknown |
| shields up | `ShieldsUp` | missing is unknown |
| SSH | `RunSSH` | missing is unknown |
| update check | `AutoUpdate.Check` | missing is unknown |
| automatic update | `AutoUpdate.Apply` | missing or null is unknown |
| posture reporting | `PostureChecking` | missing is unknown |
| hostname | `Hostname` | missing is unknown |
| nickname | `ProfileName` | missing is unknown |
| web client | `RunWebClient` | missing is unknown |
| selected exit node | `ExitNodeID`, `ExitNodeIP`, `AutoExitNode` | each missing value remains unknown; the 1.98.9 string is empty when automatic selection is off and `any` when it is on |
| exit-node LAN access | `ExitNodeAllowLANAccess` | missing is unknown |
| advertised routes | `AdvertiseRoutes` | missing is unknown |
| advertised exit node | derived from both IPv4 and IPv6 `/0` routes | cannot be derived is unknown |
| app connector | `AppConnector.Advertise` | missing is unknown |
| relay port | `RelayServerPort` | missing is unknown; null is the documented disabled state |
| relay endpoints | `RelayServerStaticEndpoints` | missing is unknown |

`AdvertisesExitNode` is interpreted using Tailscale's documented preference
semantics: both the IPv4 and IPv6 default routes must be advertised. Route
parsing is canonicalized only after a successful preference read; invalid
route data is a verification/contract error, never silently discarded.

`AutoExitNode` is an exit-node expression rather than a boolean in the
1.98.9 wire contract. Tale exposes the required operator state as a boolean:
an empty expression means automatic selection is off, and the documented
`any` expression means automatic selection is on. Any other expression or JSON
type remains unknown; it is not used to choose a transport or to infer a live
daemon response shape.

## Mutation boundary

The LocalAPI is read-only in Tale. All writes use these exact typed CLI
commands:

- `tailscale up` with no flags to connect;
- `tailscale down`, or the explicit `--accept-risk=lose-ssh` form when the
  user confirmed that risk;
- `tailscale set` with only changed, supported fields in deterministic order.

After each write, Tale performs a fresh `GetPrefs` read and compares only the
submitted fields. It never treats the CLI result as authoritative and never
uses a partial `tailscale up` command as a preference writer.

## Rejected alternatives

- `tailscale debug prefs` is explicitly rejected. It is not the documented
  structured preference-read contract required by Tale.
- `tailscale status --json` is rejected for preference reads. It is a
  documented structured status surface, but it does not provide the complete
  preference set required by the local controls and its output is not a
  preference contract.
- `tailscale set` and `tailscale up` are mutation surfaces, not current-value
  reads.
- Generic or undocumented LocalAPI endpoints, including an ad hoc preference
  edit endpoint, are rejected. Only `GetPrefs` is bound.
- Human-oriented CLI output scraping is rejected, including scraping without
  a versioned fixture.
- Configuration-file inspection and guessed socket/port fallbacks are
  rejected because they cannot prove the live daemon's current preferences or
  the claimed platform behavior.

## Evidence

The contract and platform behavior were checked against the following
official sources on 2026-08-03:

- Tailscale CLI reference: https://tailscale.com/docs/reference/tailscale-cli
- Tailscale preference management: https://tailscale.com/docs/features/client/manage-preferences
- Stable `GetPrefs` client method and request shape:
  https://github.com/tailscale/tailscale/blob/main/client/local/local.go
- LocalAPI server route and local transport handling:
  https://github.com/tailscale/tailscale/blob/main/ipn/localapi/localapi.go
- Platform socket paths:
  https://github.com/tailscale/tailscale/blob/main/paths/paths.go
- Local socket and named-pipe transport implementation:
  https://github.com/tailscale/tailscale/blob/main/safesocket/safesocket.go
- 1.98.9 preference response fields:
  https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/ipn/prefs.go
- 1.98.9 LocalAPI client transport:
  https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/client/local/local.go

This decision authorizes preference reads for local mutation previews and
verification. It does not authorize preference mutation through LocalAPI,
control of another node, policy editing, Serve, Funnel, or transfers.
