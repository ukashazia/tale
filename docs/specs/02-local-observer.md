# Specification 02 — Local observer

- Implementation phase: 2
- JJ change description: `feat: add local Tailscale observability`
- Depends on: Specification 01 complete
- Produces: a useful read-only local Tailscale monitor and diagnostic tool

This specification covers observation of the installed client and user-started
diagnostic traffic. It does not permit `up`, `down`, `set`, account changes,
Serve/Funnel changes, file transfer, or any Control API request.

Command shapes were checked against the installed Tailscale 1.98.9 CLI on
2026-08-03. The implementation must still version fixtures and reject unsupported
output; this observation does not define Tale's final compatibility range.

## 02.0 Phase contract

### User-visible result

Running `tale` attempts local integration and renders one of these states without
crashing or hiding other UI:

- executable missing;
- client version unsupported;
- daemon unavailable;
- local permission denied;
- logged out or authentication required;
- stopped/disconnected;
- running with a current node and peer collection.

When running, the user can inspect local identity and peers, filter/sort devices,
run Tailscale ping, run one-shot or live netcheck, inspect/query DNS, run whois,
and copy a redacted diagnostic summary.

### Required new module boundaries

- `src/local/mod.rs`
- `src/local/client.rs`
- `src/local/process.rs`
- `src/local/dto.rs`
- `src/local/diagnostics.rs`
- `src/domain/source.rs`
- `src/domain/device.rs`
- `src/domain/diagnostic.rs`
- `src/ui/views/local.rs`
- `src/ui/views/dns.rs`
- focused fixtures/tests under `tests/fixtures/tailscale/<version>/<platform>/`

Do not create Phase-3 mutation or Phase-5 admin modules.

## 02.1 Local executable resolution

### Resolution order

Resolve the executable once at startup in this order:

1. `--tailscale-path`;
2. `TALE_TAILSCALE_PATH`;
3. `[local].tailscale_path`;
4. executable name `tailscale` resolved through the process `PATH`.

An explicit path is never reinterpreted as a command string. It may contain
spaces. Do not use a shell, `which`, or `command -v`; resolve `PATH` entries with
platform-aware filesystem logic or attempt direct execution and classify
`NotFound` precisely.

### Domain result

```text
LocalExecutable
  path
  source              cli, environment, config, path
  version
  daemon_version
  capabilities
```

Do not canonicalize through symlinks for display. Retain the invoked path so
errors match what Tale actually executed.

### Tests

- Every precedence level.
- Absolute and relative explicit paths with spaces.
- Empty `PATH`, nonexistent file, non-executable file, and permission denied.
- Windows extension resolution through injected platform behavior.
- `--no-local` prevents resolution entirely.

### Acceptance criteria

- Startup remains usable in admin-disabled shell mode when resolution fails.
- No executable discovery error is reported as daemon failure.
- No child process is spawned in `--mock` or `--no-local` mode.

## 02.2 Bounded process runner

### Required files

- `src/local/process.rs`
- `tests/local_process.rs`
- a fake `tailscale` test executable or fixture-driven process harness

### Command contract

All local operations go through one runner accepting a typed specification:

```text
LocalCommand
  operation
  args: ordered OsString values
  stdin: closed or interactive
  stdout_mode: collect or lines
  stderr_mode: collect or lines
  timeout
  stdout_limit
  stderr_limit
  redactions
```

Phase 2 has no interactive commands. Stdin is closed for every invocation.

Default collected limits:

- stdout: 4 MiB for status/netcheck/DNS JSON;
- stderr: 256 KiB;
- streamed diagnostic retained detail: 256 KiB after structured samples are
  extracted.

Timeouts use `[local].command_timeout` unless a feature specifies another bound.
Timeout kills the owned child, waits for it, and returns a typed timeout result.
Cancellation and timeout are different results.

### Result contract

```text
LocalCommandResult
  operation
  exit_status
  stdout
  stderr
  started_at
  finished_at
  truncated_stdout
  truncated_stderr
```

The result's debug representation must not dump output. Task detail receives an
explicit bounded/redacted rendering.

### Tests

- Exact argv is recorded for every Phase-2 command.
- User input containing spaces, quotes, `$()`, semicolons, and leading hyphens
  remains one argv value and is never executed as syntax.
- Timeout and cancellation reap the child.
- Output caps cannot allocate unbounded memory.
- Non-UTF-8 output produces a typed decoding error with lossless bytes retained
  only long enough to render bounded hexadecimal context.

## 02.3 Version and capability probe

### Invocation

Run:

```text
tailscale version --json --daemon
```

If `--json` itself is unsupported, classify the client as unsupported for this
Tale build. Do not fall back to parsing human version text in Phase 2.

### DTO and domain fields

Decode only the documented/current fields needed to obtain:

- CLI version;
- daemon version when available;
- build/commit information when present;
- update track/current upstream information only when returned without a
  network-upstream check.

Unknown JSON fields are accepted. Missing required version is
`UnsupportedOutput`. Do not run `--upstream` automatically because it performs
an additional external request.

Capability probing combines version/output knowledge with actual help/command
availability. Phase 2 needs capabilities for:

- status JSON;
- ping;
- netcheck JSON and JSON-line;
- DNS status/query JSON;
- whois JSON.

Do not run every `--help` command on every refresh. Probe once at startup and
after an explicit executable-path change.

### Tests

- Valid same CLI/daemon version.
- Missing daemon version.
- Different CLI/daemon versions shown as attention, not fatal.
- Unknown fields.
- Missing required version and malformed JSON.
- Command unavailable disables only its feature.

## 02.4 Daemon and authentication state classification

### Inputs

Classify state from the version result, status process result, status DTO, and
typed operating-system errors. Do not match a broad substring when an exit code,
JSON field, or OS error provides a stronger signal.

### Required states

```text
LocalState
  Disabled
  Mock
  ExecutableMissing
  ExecutableDenied
  UnsupportedClient { version, reason }
  DaemonUnavailable { detail }
  PermissionDenied { operation, detail }
  NeedsLogin { auth_url? }
  Stopped
  Running
  Degraded { health_messages }
```

`Degraded` is a running daemon with health messages; it retains the usable
snapshot. A status transport error is source failure, not `Stopped`.

### UI behavior

- Header: `local: running`, `local: logged out`, `local: stale`, or another
  concise state.
- Local view: exact executable/version/daemon/source timestamps and remediation.
- Overview: one source card; no generic red banner that blocks navigation.
- Permission errors name the operation and never suggest Tale will run sudo.

### Tests

Use fixture results for every state. Error detail must be redacted and bounded.

## 02.5 Status snapshot contract

### Invocation

Run exactly:

```text
tailscale status --json
```

Use `[local].command_timeout`, 4 MiB stdout limit, closed stdin, and current
generation. Do not use `--active`; Tale needs the complete peer snapshot. Do not
start status web mode.

### DTO policy

The status JSON format is explicitly subject to change. Keep all wire types in
`src/local/dto.rs` and label fixtures with the exact client version/platform.

- Required DTO fields are required only when Tale cannot identify a row/source
  without them.
- Optional fields remain optional through conversion.
- Unknown fields are accepted.
- Never deserialize directly into UI/domain structs.
- A map key from JSON is not assumed to be the display or stable device ID.

### Domain snapshot

```text
LocalSnapshot
  observed_at
  client_version
  backend_state
  health_messages
  current_tailnet
  magic_dns_suffix
  cert_domains
  self_node
  peers

LocalDevice
  id                   stable opaque local ID
  public_key?          diagnostic identity, never displayed by default
  display_name
  hostname
  dns_name?
  os
  owner_label?
  user_id?
  tags
  tailscale_ips
  advertised_routes
  current_endpoint?
  relay_region?
  path                 direct, derp, peer_relay, idle, unknown
  online?              tri-state
  active
  rx_bytes
  tx_bytes
  created_at?
  last_seen?
  last_handshake?
  exit_node
  exit_node_option
  ssh_host_keys_present
  shared
  capabilities         preserved named values used by UI
```

Do not fabricate timestamps, owners, booleans, zero byte counts, or path types
when fields are absent.

### Resource state

```text
LocalResource
  snapshot: optional last good LocalSnapshot
  status: never_loaded, loading, fresh, stale, failed
  last_attempt_at
  last_success_at
  failure?
  generation
  consecutive_failures
```

A failed refresh changes metadata only. It never replaces `snapshot` with an
empty collection.

### Tests

- Tagged fixtures for the current development client plus minimum supported
  fixtures selected during implementation.
- Self missing, peer map missing/empty, unknown OS, unknown path, missing user,
  shared/tagged nodes, IPv6 only, long names, and health messages.
- Malformed JSON and a required identity missing.
- Unknown fields remain accepted.
- Last-good snapshot remains byte-for-byte/domain-equal after refresh failure.

## 02.6 Local Overview

### Required files

- `src/ui/views/overview.rs`
- `src/ui/views/local.rs`
- `tests/ui_local_overview.rs`

### Overview content

Show, when known:

- local state and last success age;
- current node name and tailnet display name;
- Tailscale IPv4/IPv6;
- CLI/daemon version mismatch;
- peer totals: online, offline, active, direct, DERP, peer relay;
- health messages;
- active diagnostic task count.

Counts are derived from the current snapshot in one pure function. Unknown
liveness is not counted as offline. A stale snapshot labels all derived counts
as stale.

### Local view content

Show source/executable/version, current node identity, addresses, DNS name,
backend state, health, observed connection data, and read-only preference values
only if the selected Phase-2 status contract exposes them. Do not parse
undocumented debug preferences.

### Acceptance criteria

- Overview remains useful with zero peers.
- Missing optional self fields render as `not returned`, not empty strings.
- Health messages are inspectable without taking over the screen.

## 02.7 Devices collection and inspector

### Required files

- `src/ui/views/devices.rs`
- `src/ui/components/table.rs`
- `src/ui/components/inspector.rs`
- `src/domain/device.rs`
- `tests/ui_devices.rs`

### Standard columns

In priority order:

1. state/path marker;
2. name;
3. owner/tag summary;
4. OS;
5. last seen/active age;
6. path;
7. traffic summary when width permits.

Wide columns add Tailscale IP, version when returned, route summary, exit-node
state, and tags. Column disappearance follows a fixed width-priority table; it
does not depend on row contents.

### Inspector sections

1. Identity: stable ID, names, OS, owner/tags.
2. Addresses: Tailscale IPs, DNS name, current endpoint when returned.
3. Connection: path, relay, active/online, last handshake/seen, traffic.
4. Roles: exit node, advertised routes, SSH keys/capabilities, shared state.
5. Source: local observation time and freshness.

Raw keys and endpoints are copyable only through explicit fields. Public
endpoints are labeled potentially sensitive.

### Filter fields

Implement:

```text
online:true|false|unknown
owner:<text>
os:<text>
path:direct|derp|peer-relay|idle|unknown
tag:<tag>
lastSeen:<duration or >duration
property:exit-node|exit-node-option|subnet-router|ssh|shared
```

Free text searches name, hostname, DNS name, owner, tags, and IP strings.

### Sort fields

Name, liveness, owner, OS, last seen, path, RX, TX, and stable ID. Default sort:
self first if included, then online/active, then display name, then stable ID.

### Acceptance criteria

- Status order changes cannot move selection to a different stable ID.
- Unknown values have explicit sort/filter behavior.
- Rendering 5,000 fictional/current-shape devices remains responsive.
- No row wraps.

## 02.8 Ping diagnostic

### Invocation

Default Probe action invokes separate argv equivalent to:

```text
tailscale ping --c=10 --timeout=5s --until-direct=true <target>
```

`<target>` is the selected device's preferred DNS name, falling back to its
first Tailscale IP. It is one argv value. The UI displays which target will be
used before start.

### Streaming and parsing

Ping has no JSON mode in the checked CLI. Implement a version-fixtured line
parser for sample lines without depending on terminal colors or fixed columns.

```text
PingSample
  sequence
  observed_at
  latency?
  path: direct, derp, peer_relay, unknown
  endpoint_or_region?
  raw_line
```

- Render samples as they arrive.
- Preserve unknown lines in bounded task detail.
- Exit code 0 with no parsed samples is `SucceededWithUnparsedOutput`, visibly
  degraded but not converted into fake latency.
- Non-zero exit is failure with bounded stderr.
- User cancellation is Cancelled and does not mark the peer unhealthy.

### Summary

Compute loss only from expected/successful parsed samples when sequence/count is
known. Compute min/average/max over samples with latency. Show the last observed
path and whether a direct path was reached. Never retain the result as permanent
device truth after the task closes; it is a timestamped diagnostic observation.

### Tests

- Direct first sample, DERP-to-direct transition, peer relay, timeout, unknown
  line, mixed stderr, cancellation, and no parsed samples.
- Target names containing shell syntax remain one argv value.
- Summary math uses checked duration arithmetic and handles zero samples.

## 02.9 Netcheck diagnostic

### Invocations

One-shot:

```text
tailscale netcheck --format=json
```

Live:

```text
tailscale netcheck --format=json-line --every=2s
```

Live mode has no automatic timeout and remains cancellable. One-shot uses the
configured command timeout.

### Display

Show when returned:

- UDP availability;
- IPv4/IPv6 availability without exposing public addresses in the compact view;
- mapping-varies-by-destination and hairpinning;
- port-mapping mechanisms;
- nearest DERP;
- DERP region latency sorted ascending with region code/name;
- observation timestamp.

Unknown JSON fields and missing measurements are accepted. Public mapped
addresses are marked sensitive and excluded from the default copied summary.

### Tests

- One-shot and JSON-line fixtures.
- Partial measurements, no UDP, IPv6 only, unknown DERP fields, malformed line
  amid valid live lines, cancellation, and output cap.
- A malformed live line adds task detail but does not discard prior valid
  observations; final non-zero exit still fails the task.

## 02.10 DNS status and query

### Invocations

Status:

```text
tailscale dns status --json
```

Query:

```text
tailscale dns query --json <name> <type>
```

Allowed query types in Phase 2: `A`, `AAAA`, `CNAME`, `MX`, `NS`, `PTR`, `SRV`,
and `TXT`. Default is `A`. Normalize the type to uppercase. Validate that the
query name is non-empty and contains no whitespace; the CLI remains authoritative
for DNS-name validity.

### DNS view

Status sections:

- local forwarder enabled state;
- MagicDNS enabled state/suffix/current node DNS name;
- resolver order;
- split DNS routes;
- certificate domains;
- status source and age.

Query result shows question, record type, answers, resolver(s), latency when
returned, and raw bounded detail for unknown record fields. It is a task result,
not merged into DNS configuration.

### Tests

- Status with system resolver fallback, ordered resolvers, split routes, extra
  records, and missing optional data.
- Query fixtures for every allowed type, NXDOMAIN/empty answer, malformed JSON,
  invalid local form input, and non-zero CLI exit.
- Argument injection strings remain data.

## 02.11 Whois

### Invocation

```text
tailscale whois --json <ip-or-ip:port>
```

An optional protocol selector adds `--proto=tcp` or `--proto=udp`. Validate the
IP/port with Rust IP/socket types before invocation. A selected device action
uses a Tailscale IP, not its public endpoint.

### Result

Show machine ID/name/addresses/tags, user identity, and capabilities when
returned. Treat whois identity as a timestamped result. Link to an existing
local device only on exact stable ID, never name or IP heuristic.

### Tests

- IPv4, bracketed IPv6 with port, IPv6 without port, TCP/UDP, tagged machine,
  user-owned machine, unknown identity, and malformed result.

## 02.12 Redacted diagnostic copy

### Required files

- `src/domain/redaction.rs`
- `src/ui/components/copy_picker.rs`
- `tests/redaction.rs`

Phase 2 does not add an OS clipboard dependency. The copy picker may use the
terminal's supported selection/OSC mechanism only if Phase 1 already selected a
safe implementation; otherwise it renders selectable text. It must never shell
out to `pbcopy`, `xclip`, or another command.

Default diagnostic summary includes:

- Tale and Tailscale versions;
- platform/OS class;
- local state and health categories;
- peer stable pseudonym, OS, path type, and timing summary;
- netcheck booleans and DERP latency;
- DNS query type/result class;
- timestamps and whether data is stale.

Default redaction replaces:

- device/user/tailnet names with stable per-report labels;
- emails;
- Tailscale and public IP addresses;
- filesystem paths;
- public endpoints;
- command output not explicitly mapped above.

The redacted report must be deterministic within one generation and cannot be
used to infer the original values from reversible encoding.

## 02.13 Refresh scheduling and backoff

### Rules

- Status refresh interval comes from `[local].refresh_interval`.
- Never overlap two status refreshes.
- Manual `r` supersedes/cancels an older read and increments generation.
- `R` behaves the same as `r` until multiple sources exist.
- First failure waits the normal interval.
- Subsequent consecutive failures use
  `min(refresh_interval * 2^(failures - 1), 60s)` with checked/saturating math.
- Manual refresh remains available during backoff.
- A successful refresh resets the failure count and normal cadence.
- Do not back off user-started diagnostics based on status failures.

Refresh selection preservation occurs after filtering/sorting the new snapshot
by stable ID. A removed selected device chooses the nearest previous visible
position; an empty result has no selection.

### Tests

Use paused Tokio time for normal cadence, failure sequence, cap, manual refresh,
success reset, cancellation, and stale result. Tests must not sleep in wall time.

## 02.14 Phase verification and handoff

Run:

```text
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Required manual/PTY journeys:

1. Start without `tailscale` on PATH.
2. Start with an executable whose daemon is unavailable.
3. Start against a running logged-in daemon.
4. Filter/sort a populated local device list during refresh.
5. Run/cancel ping and live netcheck.
6. Run DNS status/query and whois.
7. Copy/render the redacted diagnostic report and inspect it for identifiers.
8. Stop the daemon while Tale is open and verify last-good stale behavior.
9. Restore the daemon and verify cadence/state recovery.
10. Quit during a diagnostic and confirm terminal restoration.

The phase handoff must include:

- exact Tailscale versions/platforms represented by fixtures;
- exact commands invoked and their timeouts/output bounds;
- local states actually exercised against a real installation;
- parser limitations for human ping output;
- confirmation that no mutating CLI command or Control API request exists.
