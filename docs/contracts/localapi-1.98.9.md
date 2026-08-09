# LocalAPI contract ledger: Tailscale 1.98.9

Status: pinned for Tale's local daemon observer

Source tag inspected: `v1.98.9`

## Requests

| Name | Method | Request target | Required headers | Response |
| --- | --- | --- | --- | --- |
| status | `GET` | `http://local-tailscaled.sock/localapi/v0/status` | `Host: local-tailscaled.sock`, `Tailscale-Cap: 138` | `200`, JSON `ipnstate.Status` |
| preferences | `GET` | `http://local-tailscaled.sock/localapi/v0/prefs` | `Host: local-tailscaled.sock`, `Tailscale-Cap: 138` | `200`, JSON `ipn.Prefs` |
| watch | `GET` | `http://local-tailscaled.sock/localapi/v0/watch-ipn-bus?mask=4495` | `Host: local-tailscaled.sock`, `Tailscale-Cap: 138` | `200`, JSON `ipn.Notify` followed by LF |

The endpoint accepts no browser `Origin` or `Referer` request. A read denied by
the LocalAPI is HTTP 403. Invalid host or browser-shaped requests are also
denied. Tale uses no LocalAPI write endpoint.

## Watch mask

The exact selected mask is `4495`:

| Symbol | Bit |
| --- | ---: |
| `NotifyWatchEngineUpdates` | 1 |
| `NotifyInitialState` | 2 |
| `NotifyInitialPrefs` | 4 |
| `NotifyInitialNetMap` | 8 |
| `NotifyInitialHealthState` | 128 |
| `NotifyRateLimit` | 256 |
| `NotifyPeerChanges` | 4096 |

The source's `Notify` object uses these top-level fields for Tale
classification:

- status invalidation: `State`, `NetMap`, `SelfChange`, `PeerChanges`,
  `Engine`, `LoginFinished`, and `ClientVersion`;
- preference invalidation: `Prefs`;
- health/status invalidation: `Health`;
- stream-level daemon error: `ErrMessage`.

Tale does not decode nested peer, preference, health, or engine values from
notifications. It performs authoritative reads after classification.

## Framing and bounds

`serveWatchIPNBus` writes with Go `json.Encoder.Encode`, and the tagged client
reads with `json.Decoder`. The observed wire contract is one JSON object plus
`\n` per notification. Tale's framing decoder accepts arbitrary HTTP body
chunks and multiple newline-delimited objects in one chunk. It rejects a
non-empty tail at clean stream close, malformed JSON, and any framed object
larger than 32 MiB. Snapshot response bodies are also capped at 32 MiB after
HTTP decoding. Error details are capped and redacted before storage.

## Platform endpoint evidence

The tagged `paths.DefaultTailscaledSocket` returns the Windows protected named
pipe, the macOS standalone Unix socket, and the Linux Tailscale Unix socket
when `/var/run` exists. Tailscale also has distro-specific paths and a
last-resort relative Unix path; Tale's documented matrix intentionally does
not probe those alternatives. A configured path is native `PathBuf` data and
is used verbatim.

Socket-only mode is required for Tale. The tagged Tailscale client otherwise
has a macOS GUI fallback that discovers a random loopback port and token. Tale
does not implement that unstable authenticated transport.

## Evidence links

- [`client/local/local.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/client/local/local.go)
- [`ipn/localapi/localapi.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/ipn/localapi/localapi.go)
- [`ipn/backend.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/ipn/backend.go)
- [`tailcfg/tailcfg.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/tailcfg/tailcfg.go)
- [`paths/paths.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/paths/paths.go)
- [`safesocket/safesocket.go`](https://raw.githubusercontent.com/tailscale/tailscale/v1.98.9/safesocket/safesocket.go)
