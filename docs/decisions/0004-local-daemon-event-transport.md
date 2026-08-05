# Decision 0004: local daemon event transport

Status: accepted for Specification 11

Date: 2026-08-05

## Context

Specification 11 replaces periodic CLI status reads and the preferences-only
transport with one read-only LocalAPI observation path. The contract must be
pinned to the exact client family represented by Decision 0003. Tailscale's
LocalAPI is not a general-purpose compatibility promise, so the request shape,
watch mask, framing, and platform connection rules are recorded here before
the implementation uses them.

## Contract decision

Tale targets the Tailscale source tag `v1.98.9`. It uses a maintained HTTP/1
client over the selected Unix-domain socket or Windows named pipe and issues
only these LocalAPI reads:

| Operation | Method | Path | Success |
| --- | --- | --- | --- |
| status | `GET` | `/localapi/v0/status` | `200` JSON status |
| preferences | `GET` | `/localapi/v0/prefs` | `200` JSON preferences |
| IPN watch | `GET` | `/localapi/v0/watch-ipn-bus?mask=4495` | `200` newline-delimited JSON |

Every request uses host `local-tailscaled.sock` and the capability header
`Tailscale-Cap: 138`. The value is the tagged source's
`tailcfg.CurrentCapabilityVersion`, not a value inferred from a running
daemon. The watch mask is:

```text
NotifyWatchEngineUpdates     1 << 0
NotifyInitialState           1 << 1
NotifyInitialPrefs           1 << 2
NotifyInitialNetMap          1 << 3
NotifyInitialHealthState     1 << 7
NotifyRateLimit              1 << 8
NotifyPeerChanges            1 << 12
                               --------
                               4495
```

The source writes each watch notification with `json.Encoder.Encode`, which
produces one JSON value followed by `\n`. Tale therefore accepts LF-framed
notifications, accepts arbitrary HTTP body chunk boundaries and multiple
frames per chunk, and rejects a non-empty unterminated tail. The source does
not define CRLF framing for this endpoint; Tale does not treat CRLF as a
second protocol.

## Platform boundary

The selected endpoint is resolved once with CLI, environment, TOML, and
platform-default precedence. Tale uses the configured endpoint exactly and
does not probe alternatives. The supported endpoint candidates are:

| Platform | Default | Transport |
| --- | --- | --- |
| Linux/standalone Unix | `/var/run/tailscale/tailscaled.sock` | Unix socket |
| macOS standalone client | `/var/run/tailscaled.socket` | Unix socket |
| Windows | `\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled` | named pipe |

The tagged Tailscale source also contains distro-specific Unix defaults and a
last-resort `tailscaled.sock`; those are not silently probed or claimed by
Tale. macOS GUI/App Store loopback authentication is explicitly unsupported:
Tale selects socket-only operation and does not inspect processes, scrape
proof tokens, or guess a loopback endpoint. Unix permission errors and LocalAPI
read-denied responses are reported as permission failures; Windows named-pipe
open/connect failures remain endpoint-specific transport failures.

## Observation rules

Status and preferences are authoritative snapshots. The watch stream only
classifies invalidations from the top-level `State`, `NetMap`, `SelfChange`,
`PeerChanges`, `Engine`, `Health`, `LoginFinished`, `Prefs`, and `ErrMessage`
fields. `ErrMessage` is a stream-level daemon error. Unknown additive fields
are ignored and malformed JSON or a malformed required object envelope ends the
watch generation for reconnect.

Status-affecting fields schedule a status read; `Prefs` schedules a preference
read; a notification containing both schedules both. Duplicate invalidations
coalesce for 75 ms and continuous invalidations flush no later than 250 ms
after the first. A resource has at most one read in flight. An invalidation
arriving during a read sets one dirty follow-up generation. A 30-second
reconciliation reads both resources and repairs missed notifications.

The watcher is connected before bootstrap reads begin. Once its response is
accepted, status and preferences start concurrently. Bootstrap and every
reconnect perform authoritative reads; the source is live only after status
succeeds and the watcher is established. Reconnect delays are 250 ms, 500 ms,
1 s, 2 s, then 5 s, with one timer. The sequence resets after a connected
watch has lasted 30 seconds or a full resync succeeds. Cancellation closes
active HTTP bodies and prevents future reconnects.

Snapshot request and notification bounds are both 32 MiB. Snapshot requests
have a 10-second connection/request deadline and no per-request retry. Watch
connections have the same establishment deadline but no idle timeout. Errors
retain only a bounded, redacted protocol detail and never include peer,
tailnet, address, or preference data.

## CLI separation

The LocalAPI adapter is read-only and independent of the process adapter. The
typed direct-process CLI adapter remains responsible for mutations and other
commands without an approved LocalAPI contract. When a non-default socket is
selected, supported CLI invocations receive the root `--socket PATH` option
before the subcommand. A mutation is accepted only after a targeted
authoritative LocalAPI verification read. No CLI status fallback exists.

## Primary sources

- LocalAPI client and request paths: [Tailscale `client/local/local.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/client/local/local.go)
- LocalAPI handlers, capability response, watch mask parsing, and newline encoding: [Tailscale `ipn/localapi/localapi.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/ipn/localapi/localapi.go)
- Watch mask values and notification fields: [Tailscale `ipn/backend.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/ipn/backend.go)
- Capability value `138`: [Tailscale `tailcfg/tailcfg.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/tailcfg/tailcfg.go)
- Platform default socket selection: [Tailscale `paths/paths.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/paths/paths.go)
- Socket and named-pipe connection boundary: [Tailscale `safesocket/safesocket.go` at `v1.98.9`](https://github.com/tailscale/tailscale/blob/v1.98.9/safesocket/safesocket.go)
