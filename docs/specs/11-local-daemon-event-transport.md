# Specification 11 — Local daemon event transport

- Implementation phase: 11
- JJ change description: `refactor: replace local polling with daemon events`
- Depends on: Specifications 01–10 complete and the Specification 10 report triaged
- Produces: one typed, event-driven LocalAPI observation path for the local node

This phase replaces periodic `tailscale status --json` observation and the
one-off preferences transport with a shared LocalAPI client. The watcher tells
Tale when daemon state may have changed; authoritative LocalAPI reads still
produce snapshots. The Tailscale CLI remains the supported execution boundary
for local mutations and commands whose LocalAPI contract has not been approved.

## 11.0 Phase contract

### User-visible result

When a compatible local Tailscale daemon is available, Tale starts without
spawning `tailscale status`, reflects daemon changes promptly, survives daemon
restart, and continues to distinguish local observation from remote Control API
administration. Local read-only use does not require the `tailscale` executable.

### Non-negotiable architectural result

```text
                                   local daemon
                               UDS / named pipe HTTP
                                  ▲           │
                  status + prefs  │           │ watch-ipn-bus
                                  │           ▼
┌──────────────┐   typed effects  │   ┌──────────────────┐
│ App reducer  │──────────────────┴──▶│ LocalDaemonClient │
└──────┬───────┘                      └──────────────────┘
       │
       │ mutation/diagnostic effect
       ▼
┌──────────────────┐
│ LocalCliClient   │── explicit argv ──▶ tailscale executable
└──────────────────┘
```

LocalAPI observation and CLI execution are separate capabilities. A working
daemon socket is enough for observation. A working CLI is additionally required
only by actions that execute it. Neither capability is inferred from the other.

### In scope

- LocalAPI `status`, `prefs`, and `watch-ipn-bus` reads;
- Unix-domain-socket and Windows named-pipe transports;
- watcher bootstrap, event coalescing, reconnect, reconciliation, and source
  freshness;
- local socket configuration and CLI/socket alignment;
- independent daemon-read and CLI-execution capability reporting;
- removal of the obsolete status polling and manual preferences-only client;
- deterministic fake-daemon, lifecycle, performance, and no-process-spawn tests;
- architecture, configuration, support, troubleshooting, and UX updates.

### Explicitly out of scope

- writing undocumented LocalAPI endpoints;
- replacing supported CLI mutations, interactive handoffs, service commands,
  diagnostics, file transfer, certificate, account, or policy commands;
- parsing the `watch-ipn-bus` stream into optimistic domain mutations;
- a CLI status fallback when LocalAPI is unavailable;
- compatibility aliases for `local.refresh_interval`;
- installing, starting, stopping, elevating, or reconfiguring Tailscale;
- accessing the hosted Control API through the daemon;
- claiming support for macOS GUI-client loopback authentication without a
  stable, tested contract;
- weakening `--mock` isolation or performing a real tailnet mutation in tests.

## 11.1 Contract research and decision gate

Before implementation, add:

```text
docs/decisions/0004-local-daemon-event-transport.md
docs/contracts/localapi-<tailscale-version>.md
```

The decision must record:

- the exact Tailscale source tag or commit inspected;
- the LocalAPI capability header name and value construction;
- request paths and methods for status, preferences, and IPN watching;
- the `NotifyWatchOpt` mask names and numeric value used by Tale;
- the newline-delimited JSON framing contract;
- maximum request and event sizes;
- platform endpoint discovery and permission behavior;
- which decoded fields trigger which authoritative reads;
- reconnect, debounce, reconciliation, and cancellation rules;
- the supported versus experimental platform matrix;
- why CLI operations remain a separate adapter.

Use Tailscale source at the exact supported tag as the primary contract. Do not
derive protocol requirements from web UI behavior, error strings, or a running
daemon alone. The initial implementation must cover the exact client family in
Decision 0003. A newer family requires a new contract fixture and test, not a
permissive fallback parser.

For the currently researched contract, evaluate this watch mask:

```text
NotifyWatchEngineUpdates
| NotifyInitialState
| NotifyInitialPrefs
| NotifyInitialNetMap
| NotifyInitialHealthState
| NotifyRateLimit
| NotifyPeerChanges
```

Record the verified integer in Decision 0004. Do not copy a remembered value
without confirming it against the selected Tailscale source tag.

## 11.2 Module and ownership boundaries

The target structure is:

```text
src/local/
  daemon.rs       transport-neutral LocalAPI requests and responses
  ipn.rs          watch request, bounded framing, and typed invalidations
  client.rs       CLI-only typed operations
  dto.rs          CLI DTOs that remain after status removal
  process.rs      non-shell CLI process runner
```

`src/local/preferences.rs` is removed. Its supported preference decoding and
tests move to `daemon.rs`; its manual HTTP parsing and separate connection
ownership do not remain as compatibility code.

`LocalDaemonClient` owns:

- endpoint selection;
- HTTP request construction over the platform byte stream;
- required LocalAPI headers;
- bounded body collection for snapshot requests;
- streaming body access for the watcher;
- status and preference DTO decoding;
- protocol and transport error classification.

`LocalCliClient` owns only process-backed operations. Rename the current broad
`LocalClient` when needed so call sites cannot confuse daemon reads with CLI
commands. Do not introduce a generic repository or transport trait used by both
the local daemon and hosted API; their semantics are different.

Use a maintained HTTP/1 implementation over an `AsyncRead + AsyncWrite`
connection. Add direct dependencies only after checking existing dependency
types and features. Do not retain or create a handwritten HTTP response parser.
The implementation must handle informational details required by the selected
library, chunked responses, connection close, and protocol errors through that
library.

No `unsafe`, panic, `unwrap`, or `expect` is permitted in production or test
code. All spawned watcher tasks have explicit cancellation and join ownership.

## 11.3 Platform transport contract

### Endpoint resolution

Resolve exactly one endpoint at startup using this precedence:

1. `--tailscale-socket <PATH>`;
2. `TALE_TAILSCALE_SOCKET`;
3. `[local].socket_path`;
4. the documented platform default.

The platform defaults must be confirmed in Decision 0004. The expected
candidates to verify are:

| Platform | Candidate default | Transport |
| --- | --- | --- |
| Linux and standalone Unix clients | `/var/run/tailscale/tailscaled.sock` | Unix socket |
| macOS standalone client | `/var/run/tailscaled.socket` | Unix socket |
| Windows | Tailscale protected named-pipe path | named pipe |

Paths remain `PathBuf`/native platform values and are never round-tripped
through lossy UTF-8. A configured endpoint is used exactly; Tale does not probe
a list of alternatives. An absent, denied, or incompatible endpoint yields a
specific local-source state and remediation.

macOS App Store and GUI variants that expose an authenticated random loopback
port are unsupported until a separate stable contract is approved. Do not add
process inspection, credential scraping, or a guessed loopback fallback.

### CLI alignment

When a non-default socket is selected, every CLI invocation that supports the
global Tailscale socket option receives that endpoint as explicit argv. Verify
the exact option placement against the supported CLI. Commands that cannot
target that endpoint are disabled with a reason; they must not silently operate
on a different daemon.

### Request rules

Every LocalAPI request includes the verified capability header and a fixed Host
value required by the contract. Snapshot requests have:

- a 10-second connection/request deadline;
- a 32 MiB maximum decoded body;
- no automatic retry inside an individual request;
- cancellation that closes the active connection;
- a response error containing method, endpoint kind, HTTP status, and a
  redacted bounded message.

The watch request has the connection deadline but no idle timeout. Normal idle
time is not an error. Its cancellation closes the stream promptly.

LocalAPI paths, response bodies, and errors must never be logged with peer
names, addresses, tailnet identity, or preferences unless the existing
redaction contract has classified and removed those fields.

## 11.4 Snapshot authority

Add typed methods with equivalent contracts to:

```rust
async fn status(&self, cancellation: CancellationToken)
    -> Result<LocalStatusSnapshot, LocalDaemonError>;

async fn preferences(&self, cancellation: CancellationToken)
    -> Result<LocalPreferenceSnapshot, LocalDaemonError>;

async fn watch(&self, mask: NotifyWatchMask, cancellation: CancellationToken)
    -> Result<LocalWatchStream, LocalDaemonError>;
```

Exact Rust names may follow existing conventions, but the boundaries may not be
collapsed into untyped JSON values.

Status and preferences reads are authoritative. On success, convert their DTOs
to domain models at the adapter boundary and atomically replace the applicable
snapshot generation. On failure, preserve the last good snapshot, mark the
resource stale, and retain a typed error.

The watcher is an invalidation source. It must not directly edit a peer row,
preference, backend state, health result, or capability from partial `Notify`
content. This avoids an incomplete client-side replica of Tailscale's backend.

Decode only the top-level notification fields required to classify:

- state/status invalidation;
- netmap/peer/status invalidation;
- preferences invalidation;
- health invalidation;
- stream-level daemon error;
- a notification with no Tale-relevant change.

Unknown additive fields are ignored. A malformed required envelope is a stream
protocol error and triggers reconnect; it is not reinterpreted as another
schema.

## 11.5 Bootstrap and steady-state algorithm

Startup ordering is exact:

1. resolve the endpoint;
2. connect the watcher;
3. once the watch response is accepted, begin authoritative status and
   preferences reads concurrently;
4. coalesce notifications received while those reads are in flight;
5. commit each successful snapshot by generation;
6. perform one targeted follow-up read for any invalidation that arrived after
   that resource's request generation began;
7. mark the source live only after status has succeeded and the watch stream is
   established.

This order closes the gap between an initial GET and watcher subscription.

During steady state:

- status, state, netmap, peer, engine, or health invalidations schedule status;
- preference invalidations schedule preferences;
- a notification affecting both schedules both;
- duplicate invalidations within 75 ms are coalesced;
- continuous invalidations are flushed no later than 250 ms after the first;
- no more than one read per resource is active;
- an invalidation during an active read records a dirty bit and causes exactly
  one follow-up generation;
- stale results are discarded using the existing generation mechanism;
- rendering never waits for the stream or a read.

Add a 30-second reconciliation read for status and preferences. This is a
safety reconciliation, not the primary refresh mechanism. It runs only while
local observation is enabled and is reset after a successful full resync.

## 11.6 Stream framing, bounds, and reconnect

The selected Tailscale contract emits one JSON notification followed by a
newline. The decoder must:

- accept a notification split across arbitrary HTTP body chunks;
- accept multiple notifications in one chunk;
- accept `\n` and a verified optional preceding `\r` only if the source
  contract permits it;
- reject a non-empty unterminated tail when the stream closes;
- cap one framed notification at 32 MiB;
- release consumed buffer capacity rather than retaining an unbounded peak;
- never block the reducer while decoding.

An oversized or malformed notification terminates that watcher generation,
updates source metadata, and enters reconnect. It must not crash Tale or discard
the last good snapshot.

Reconnect delays are:

```text
250 ms, 500 ms, 1 s, 2 s, then 5 s for every subsequent attempt
```

There is at most one reconnect timer. Jitter may be added only if deterministic
tests inject it and the upper bound remains documented. Reset the sequence
after the watcher has remained connected for 30 seconds or completed a full
resync, whichever occurs first.

After every reconnect, perform the complete bootstrap resync. Do not assume
notifications missed during disconnection can be reconstructed. Cancellation,
application shutdown, `--no-local`, or switching to mock mode ends reconnect
without emitting another attempt.

## 11.7 Configuration replacement

Replace the local configuration contract with:

```toml
[local]
enabled = true
tailscale_path = "tailscale"
socket_path = "/var/run/tailscale/tailscaled.sock"
reconcile_interval = "30s"
command_timeout = "10s"
```

Rules:

- `socket_path` is optional; absence selects the verified platform default;
- `reconcile_interval` accepts 5s–10m and defaults to 30s;
- remove `local.refresh_interval` from types, defaults, diagnostics, examples,
  and documentation;
- a config containing `local.refresh_interval` is an unknown-field error;
- there is no alias, warning-only migration, or fallback behavior;
- `tailscale_path` remains optional capability configuration for CLI actions;
- `--no-local` disables daemon observation, the watcher, reconciliation, and
  CLI-backed local actions;
- `--mock` constructs neither a daemon transport nor a process client;
- CLI, environment, TOML, and default provenance remains visible in Settings
  and `doctor` without exposing private endpoint components beyond the local
  path already supplied by the user.

Add `--tailscale-socket` and `TALE_TAILSCALE_SOCKET` to help, completions, man
page, configuration documentation, and doctor output. A CLI argument must be a
native path value, not a command string.

## 11.8 Capability and source presentation

Replace a single local availability implication with at least these explicit
capabilities:

| Capability | Proven by | Enables |
| --- | --- | --- |
| daemon observation | successful LocalAPI status and active/reconnecting watcher state | local status, peers, prefs, health |
| CLI execution | executable discovery plus supported version | mutations and process-backed actions |
| mutation verification | daemon observation plus action-specific CLI support | safe local changes |

The header and Sources sections distinguish:

- `local daemon · live`;
- `local daemon · reconnecting · last good 12s`;
- `local daemon · permission denied`;
- `local CLI · unavailable`;
- `admin API · live/stale/not configured`.

A missing CLI must not mark LocalAPI values unavailable. A missing daemon must
not hide admin views. A CLI action remains disabled when its result cannot be
verified through the daemon, unless its existing specification explicitly
defines a different verification source.

For local mutations:

1. read the current authoritative state;
2. execute the typed CLI command;
3. request an immediate targeted LocalAPI read rather than waiting for a watch
   event;
4. accept success only if the read verifies the intended result;
5. treat any later matching event as a normal invalidation.

## 11.9 Required implementation touchpoints

The implementing agent must inspect and update every affected use, including:

```text
Cargo.toml
Cargo.lock
src/cli.rs
src/config.rs
src/doctor.rs
src/app.rs
src/event.rs
src/effect.rs
src/runtime.rs
src/domain/source.rs
src/local/mod.rs
src/local/client.rs
src/local/dto.rs
src/local/preferences.rs        # remove
src/local/daemon.rs             # add
src/local/ipn.rs                # add
src/ui/components/header.rs
src/ui/views/local.rs
src/ui/views/settings.rs
tests/local_client.rs
tests/local_observer.rs
tests/local_operator.rs
tests/runtime.rs
tests/config.rs
tests/cli.rs
tests/compatibility.rs
tests/acceptance/
docs/architecture.md
docs/configuration.md
docs/product.md
docs/ux.md
docs/support.md
docs/troubleshooting.md
docs/cli/tale.1
completions/
```

This is a required audit list, not permission for blind edits. Update a file
only when its current contents are affected. Search the complete repository for
`refresh_interval`, `status --json`, `PreferenceClient`, the old status parser,
and local capability assumptions. Remove obsolete paths rather than leaving
dormant implementations.

## 11.10 Deterministic test harness

Build a fake LocalAPI server over the actual platform transport abstraction. On
Unix, use a temporary Unix socket under a validated temporary directory. On
Windows, use an isolated named pipe. It must support scripted:

- expected method, path, query, Host, and capability header;
- bounded status and preferences responses;
- chunked and content-length bodies;
- watcher events with arbitrary chunk boundaries;
- delayed reads, connection close, HTTP errors, malformed JSON, and oversized
  messages;
- daemon disappearance and restart at the same endpoint;
- request counters and unexpected-request failure.

Test at minimum:

1. watcher connects before initial reads;
2. initial status and preferences commit atomically per resource;
3. a startup event cannot be lost;
4. status-only, preference-only, and combined invalidations target correctly;
5. burst events coalesce at 75 ms and flush by 250 ms;
6. an event during a read causes one follow-up read;
7. stale generations cannot replace newer state;
8. 30-second reconciliation repairs a deliberately omitted event;
9. every documented reconnect delay and reset condition;
10. reconnect performs a full resync;
11. stream split, multi-event chunk, termination, malformed, and 32 MiB bound;
12. cancellation closes idle watcher and prevents reconnect;
13. last-good data survives every read/stream failure;
14. LocalAPI observation performs zero child-process spawns;
15. missing CLI with working daemon preserves read-only local views;
16. working CLI with missing daemon does not claim verified mutation support;
17. custom socket is passed to supported CLI operations;
18. `--no-local` and `--mock` construct no local external adapter;
19. unknown `local.refresh_interval` fails configuration;
20. a generated 5,000-peer status body decodes and renders within Phase 9
    budgets without unbounded buffering.

Use paused Tokio time for debounce, reconciliation, and backoff tests. Tests do
not connect to the operator's real daemon, invoke the real CLI, inspect a real
tailnet, or require network access.

## 11.11 Documentation and operational evidence

Update the architecture diagram so local observation points to LocalAPI and
local operations point to the CLI. Replace every phrase that says the local
source is inherently a CLI snapshot. Document:

- when Tale does and does not need the Tailscale executable;
- platform endpoint and permission requirements;
- separate local-daemon, local-CLI, and admin-API health;
- reconnect and last-good behavior;
- the socket/reconciliation configuration contract;
- unsupported macOS client distributions;
- how to diagnose an endpoint without printing tailnet data;
- why LocalAPI is not used as an undocumented mutation API.

The support matrix must not promote a platform based only on compilation.
Runtime transport, restart, cancellation, and first-frame evidence is required.

## 11.12 Exit gate

Phase 11 is complete only when all are true:

- LocalAPI is the sole non-mock source for local status and preferences;
- `watch-ipn-bus` drives prompt targeted invalidation;
- reconciliation and reconnect follow this specification exactly;
- local observation spawns no CLI process;
- CLI actions remain typed, direct-process calls and target the selected socket;
- daemon and CLI capabilities are independent in state and UI;
- obsolete status polling, status CLI parsing, `PreferenceClient`, and
  `local.refresh_interval` are absent;
- no compatibility fallback or hidden alternate endpoint exists;
- the fake-daemon suite proves framing, races, bounds, restart, and cancellation;
- mock and automated tests cannot contact a real daemon or tailnet;
- the platform support claims match collected evidence;
- all documentation and generated CLI artifacts describe the new contract;
- `cargo fmt --all -- --check` passes;
- `cargo clippy --all-targets --all-features -- -D warnings` passes;
- `cargo test --all-targets --all-features --locked` passes;
- the Phase 9 security, terminal-restoration, dependency, and release-artifact
  gates still pass.

Do not begin Specification 12 until this gate passes on every platform still
claimed Supported.
