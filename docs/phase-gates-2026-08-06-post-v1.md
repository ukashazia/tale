# Post-v1 integration hardening evidence — 2026-08-06

Status: `NOT READY`. In-scope hardening and deterministic checks are complete,
but dependency policy, the maintainer audit input, full fault/soak evidence, and
real platform/terminal evidence remain blocking.

## Preflight

- Workspace: Tale repository root (private absolute path omitted from evidence).
- JJ change: `mokstorvnwqw`, `chore: harden post-v1 architecture and interface`.
- Parent: `vootmlnymwtz`, `feat: add Tailscale-inspired theme system`.
- Host: macOS 26.6, Apple Silicon, `aarch64-apple-darwin`.
- Timezone: PKT (`UTC+05:00`).
- Rust: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6.
- Cargo: 1.97.1. JJ: 0.43.0. cargo-deny: 0.20.2.
- Installed Tailscale CLI: 1.98.9, commit `6c167d40fa37aeb51afa7ff336730670ea4762bf`.
- Pinned LocalAPI contract: Tailscale 1.98.9, capability 138, watch mask 4495.
- `Cargo.lock` SHA-256:
  `b0fabe31a4ef06c58221d2bec8e2fdcc3c7fc66cbaf68b5ac2339234f23400b5`.
- Support claims: no Supported rows; Linux x86_64/aarch64 and macOS aarch64
  are Experimental; macOS x86_64 and Windows x86_64 are Omitted.
- Specification 10 maintainer audit report: `NOT PROVEN` because no report was
  supplied or stored. The maintainer explicitly authorized continuing without
  it; no findings are inferred.
- Optional real-daemon observation: not authorized and not performed.

## Pre-hardening baseline

The existing Specification 13 working change was checked before this Phase 14
child change was created:

| Command or measurement | Baseline result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo test --all-targets --all-features --locked` | pass |
| `cargo check --all-targets --locked` | pass |
| `cargo doc --no-deps --locked` | pass |
| `cargo deny check` | fail: committed policy rejects BSL-1.0, Zlib, and CDLA-Permissive-2.0 transitive licenses; maintainer decision required |
| filter 5,000 devices | 42.596–43.312 us |
| stable sort 5,000 devices | 1.4486–1.4693 ms |
| aggregate 250,000 flow messages | 79.863–80.733 ms |
| derive 5,000 health inputs | 89.577–90.075 us |
| prepared 160x45 frame | 760.72–765.26 us |
| prepared 80x24 frame | 516.75–522.70 us |
| theme switch plus 160x45 frame | 809.41–822.11 us |
| input dispatch to render request | 13.113–13.215 us |
| mock startup to first frame | 2.9345–2.9661 ms |

The Criterion baseline used the release profile, 100 samples, and fictional
deterministic data on the host above. It is not reference-runner or
cross-platform evidence.

## Exit-gate ledger

| Gate | Status | Evidence |
| --- | --- | --- |
| Integrated data/event invariants | PASS | Watcher/resource generations, shared read serialization, reconnect, selection, last-good, mutation verification, and reducer tests pass |
| Integrated interaction invariants | PASS | Editor ownership, stable IDs, completion generation, resize, history, transient/help, redaction, and theme matrix tests pass |
| Complete failure-injection matrix | NOT PROVEN | Deterministic coverage is broad, but the explicit rows below are not all proven as integrated timeout/cleanup cases |
| Deterministic Journeys A–H | PASS | `tests/acceptance/journeys.md` maps each scripted journey to fake-adapter, reducer, buffer, and cleanup tests |
| Performance budgets | PARTIAL | All measured host latency budgets pass; notification/reconnect/cancellation/shutdown timing was not measured as p95 on the reference runner |
| Idle resource and bounded-soak budgets | NOT PROVEN | Bounded structures and idle behavior are tested; no 30-minute soak, CPU, RSS, or allocation capture was available |
| Security and privacy re-audit | PASS | Four security tests, exact LocalAPI headers, bounded frames, redaction canaries, non-shell argv, and fictional artifact review pass |
| Dependency/advisory/license policy | FAIL | Advisories, bans, and sources pass; licenses fail for three unapproved license families |
| Terminal lifecycle and restoration | PARTIAL | Automated setup/error/handoff/PTY and active-interaction restoration pass; named-terminal/manual evidence is unavailable |
| Supported platform/client rows | N/A | The support matrix claims no Supported rows |
| Experimental/omitted platform accuracy | PASS | Claims remain limited to Experimental/Omitted and identify missing evidence |
| Obsolete-path semantic and source-policy audit | PASS | Runtime wiring inspected; machine-readable source policy and generated-artifact comparison pass |
| Current documentation agreement | PASS | Current README/architecture/security/support/release documents reconciled; documentation tests pass |
| Generated help/completions/man-page comparison | PASS | Fresh temporary generation is byte-identical to committed artifacts |
| Package and reproducibility dry run | PASS | Locked isolated install passed; two fixed-epoch archives and checksum files are byte-identical |
| Specifications 11–13 remain green | PASS | Full all-targets/all-features locked suite passes |
| Ordered required check sequence | PASS | All eight commands passed in the required order |
| Final JJ status/diff review | PASS | Only the 21 intentional Phase 14 paths listed by `jj status` are changed |
| No real daemon/tailnet/credential/clipboard mutation | PASS | Validation used fixtures, fake sockets, memory clipboard contracts, and temporary artifact/install directories only |

## Required command ledger

| Command | Status | Duration/result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | 0.65 s |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS | 0.49 s Cargo result |
| `cargo test --all-targets --all-features --locked` | PASS | 42 unit tests plus every integration, binary, and benchmark target; no failures |
| `cargo test --locked --test documentation` | PASS | 3 passed |
| `cargo test --locked --test terminal_restore` | PASS | 4 passed, including real PTY |
| `cargo test --locked --test acceptance` | PASS | 4 passed |
| `cargo test --locked --test compatibility` | PASS | 2 passed |
| `cargo test --locked --test security` | PASS | 4 passed |
| `cargo deny check` | FAIL | Exit 4: advisories/bans/sources pass; licenses fail |
| artifact generation and byte comparison | PASS | Bash, Zsh, Fish, and man-page outputs match committed bytes |
| isolated install/package dry run | PASS | `cargo install --locked --path .`; two deterministic archive runs match |

## Failure-injection coverage

| Fault | Result | Evidence |
| --- | --- | --- |
| socket absent / permission denied | PASS | Typed daemon classification and capability/remediation tests; no fallback path |
| watcher closes before initial reads | PASS | Fake Unix daemon closes/reconnects; generations remain monotonic and full reads repeat |
| status/prefs asymmetric failure | PASS | Independent resource freshness and last-good reducer tests |
| malformed/oversized notification | PASS | Bounded newline decoder and reconnect classification tests |
| daemon loss during verification | PASS | Mutation cannot become success without verified LocalAPI state |
| repeated daemon restart | PARTIAL | Bounded backoff and reconnect-generation units pass; integrated repeated-restart cleanup timeout is not separately scripted |
| CLI missing/timeout/cancel | PASS | Daemon-only observation, disabled actions, child timeout/cancel/reap tests |
| concurrent Control API failure | PASS | Independent admin/local last-good resources and combined-source tests |
| stale completion / selected removal / invalid forward frame | PASS | Generation discard, stable-ID repair notice, and latest-snapshot history tests |
| resize during prompt/modal | PASS | Four viewport buffer matrix and active editor preservation tests |
| theme config failure before terminal | PASS | Strict config resolution precedes runtime terminal acquisition |
| render failure after watcher starts | PARTIAL | Runtime restores and cancellation ownership is tested, but the injected test does not assert a live fake socket is closed |
| panic in injected worker | NOT PROVEN | No controlled worker-panic lifecycle test exists |
| channel full/receiver closed | PARTIAL | Backpressure/coalescing test passes; receiver-closed typed cleanup is not an explicit integrated case |

No arbitrary sleep or real endpoint is used by these cases. The PARTIAL and
NOT PROVEN rows prevent the complete matrix from passing.

## Benchmark and resource evidence

Release-profile Criterion, 100 samples, fictional deterministic data on the
preflight host:

| Measurement | Result | Gate |
| --- | ---: | --- |
| filter 5,000 devices | 44.939–53.045 us | Phase 9 latency pass |
| stable sort 5,000 devices | 2.0072–2.1645 ms | Phase 9 latency pass |
| aggregate 250,000 flows | 114.75–127.48 ms | Phase 9 latency pass |
| derive 5,000 health inputs | 93.273–104.96 us | Phase 9 latency pass |
| prepared 160x45 / 80x24 frame | 839.81–915.47 / 548.40–556.32 us | pass |
| theme switch plus 160x45 frame | 859.29–878.66 us | ≤33 ms pass |
| input dispatch to render request | 13.779–14.524 us | pass |
| mock startup to first frame | 3.3860–3.4088 ms | pass |
| completion over 100 candidates | 705.21–716.55 us | ≤16 ms pass |
| transient / help open | 2.3039–2.3706 / 2.2490–2.4575 us | ≤16 ms pass |
| history back/forward, 5,000 rows | 4.6883–4.7534 us | ≤16 ms pass |
| CPU, peak RSS, allocations, 30-minute soak | NOT PROVEN | tooling/reference run unavailable |

Criterion reported broad variance/regressions relative to the prior same-host
sample, including unchanged operations. Absolute budgets pass, but this is not
substituted for the required reference-runner and resource evidence.

## Platform and manual matrix

| Row | Status | Phase 14 evidence |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | NOT PROVEN | Experimental: fixtures/fakes only; no runner or real daemon |
| `aarch64-unknown-linux-gnu` | NOT PROVEN | Experimental: fixtures/fakes only; no ARM64 runner |
| `aarch64-apple-darwin` | PARTIAL | Experimental host build/test/package pass; no real LocalAPI/keyring/named terminal |
| `x86_64-apple-darwin` | NOT PROVEN | Omitted; no target evidence |
| `x86_64-pc-windows-msvc` | NOT PROVEN | Omitted; no Windows named-pipe/runtime evidence |
| Named terminal/manual visual matrix | NOT PROVEN | `TERM=dumb`; deterministic buffers and PTY do not prove an emulator |
| Optional real LocalAPI observation | NOT PROVEN | Not authorized; no real endpoint contacted |

## Release artifacts

Temporary dry-run output (sanitized directory basename `tmp.9bXoEtPOve`):

| Artifact | SHA-256 |
| --- | --- |
| `tale-aarch64-apple-darwin.tar` | `b2a29dfb51bbaceac0c5af2d04ecf81f1d943cf2108da9b882ddd43e0bc28181` |
| `tale-aarch64-apple-darwin.sha256` | `cd077399f874be380e40ddc4247f6225bf7b77d9fd82658f6e9b750d7ca7ccd7` |
| isolated installed `tale` binary | `8fb102eb28482e5d0c033141c972a45a45236e0982167f68155b430655c366a8` |

Both archive runs and both checksum files compare byte-for-byte. Membership
includes the binary, license/notice/readme, current release/support/security/
install/troubleshooting docs, man page, and Bash/Zsh/Fish completions.

## Findings

| ID | Severity | Status | Finding |
| --- | --- | --- | --- |
| PH14-001 | BLOCKER | OPEN | The Specification 10 audit report is unavailable, so its findings cannot be triaged. |
| PH14-002 | BLOCKER | OPEN | `cargo deny check` rejects transitive BSL-1.0, Zlib, and CDLA-Permissive-2.0 licenses; the committed policy reserves acceptance for a maintainer/legal decision. |
| PH14-003 | HIGH | OPEN | Complete integrated worker-panic, receiver-close, live-socket render-failure, and repeated-restart cleanup evidence is unavailable. |
| PH14-004 | HIGH | OPEN | Reference-runner latency and 30-minute CPU/RSS/allocation/steady-growth evidence is unavailable. |
| PH14-005 | BLOCKER | OPEN | No named terminal or complete real platform/client row has current evidence; support remains Experimental/Omitted. |
