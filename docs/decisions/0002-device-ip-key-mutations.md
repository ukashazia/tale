# Decision 0002: device IP and key mutations

Status: accepted for key-expiry operations; device IP assignment remains unsupported

Date: 2026-08-04

## Context

Specification 06 lists device IP assignment and key operations as possible
Control API mutations. Endpoint names are not sufficient evidence for a safe
operator action. Tale needs an exact request and response contract, valid-input
and conflict behavior, connectivity effects, recovery, and an authoritative
verification read before exposing either operation.

The contract was checked against the current interactive schema returned by
`https://api.tailscale.com/api/v2?outputOpenapiSchema=true` on 2026-08-04 and
the linked official Tailscale documentation.

## Device IP assignment: rejected

The public schema proves this request:

```text
POST /api/v2/device/{deviceId}/ip
scope: devices:core
Content-Type: application/json
body: {"ipv4": "100.80.0.1"}
success: 200 with an empty response body
documented errors: 404, 500, 504, each with the common JSON error body
```

The schema describes the value as a new IPv4 address and the official API
description says that the address may come from the CGNAT range or an IP pool.
It also explicitly says that changing the address breaks existing connections,
requires reconnecting with the new address, and may require flushing the DNS
cache. `GET /api/v2/device/{deviceId}?fields=all` returns the device address
list and is the only documented verification read available to Tale.

That evidence is not enough to expose the action safely:

- the public schema does not declare an IPv4 format or the valid tailnet/IP-pool
  membership rule;
- no documented conflict status or conflict body explains what happens when
  the address is already assigned, outside the pool, or otherwise unavailable;
- no documented recovery guarantee says that assigning the previous address is
  always accepted; and
- the empty mutation response does not carry the assigned address.

Because valid input, conflict behavior, and recovery cannot all be proven,
`admin.device.ip.assign` is not registered. Tale performs no IP assignment and
does not infer behavior from `/ip`.

## Key-expiry configuration: accepted

The public schema proves:

```text
POST /api/v2/device/{deviceId}/key
scope: devices:core
Content-Type: application/json
body: {"keyExpiryDisabled": true|false}
success: 200 with an empty response body
documented errors: 404, 500, 504, each with the common JSON error body
verification: GET /api/v2/device/{deviceId}?fields=all
```

The documented semantics are precise. `true` disables expiry while retaining
the original expiry time; `false` re-enables expiry at that original time, and
the device must be re-authenticated if that time has already passed. The
verification read compares `keyExpiryDisabled` and displays the server-returned
`expires` timestamp. The operation is reversible only while the original key
has not already expired; reauthentication is a user/device workflow outside
this action. Risk tier 2 shows the old/new setting and that consequence.

## Expire-now: accepted

The public schema proves:

```text
POST /api/v2/device/{deviceId}/expire
scope: devices:core
Content-Type: not sent; no body
success: 200 with an empty response body
documented errors: 404, 500, 504, each with the common JSON error body
verification: GET /api/v2/device/{deviceId}?fields=all
```

The official operation description says that the device's node key is marked
expired and the device must re-authenticate to connect again. This is
irreversible for the current key, can disconnect the device, and has no
automatic recovery. Tale uses a fresh typed device-name confirmation, performs
one request, verifies the returned expiry state/timestamp, and never tries to
reauthenticate or retry.

## Evidence

- [Interactive Tailscale API schema](https://api.tailscale.com/api/v2?outputOpenapiSchema=true)
- [Tailscale API](https://tailscale.com/docs/reference/tailscale-api)
- [Device approval and management](https://tailscale.com/docs/features/access-control/device-management/device-approval)
- [Remove a device](https://tailscale.com/docs/features/access-control/device-management/how-to/remove)
- [Tags](https://tailscale.com/docs/features/tags)
- [Auth keys and key expiry](https://tailscale.com/docs/features/access-control/auth-keys)
- [Trust credential scopes](https://tailscale.com/docs/reference/trust-credentials)
- [Configuration audit logging](https://tailscale.com/docs/features/logging/audit-logging)
