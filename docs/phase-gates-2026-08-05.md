# Phase 1–8 exit-gate verification

Reviewed before Phase 9 edits and rechecked by the full locked test suite on
2026-08-05. The Phase 8 working-copy change was the coherent prerequisite
change; no unrelated working-copy changes were present. Phase 9 started in a
new JJ change with `chore: harden Tale for 1.0`.

| Phase | Automated exit-gate evidence | Result |
| ---: | --- | --- |
| 1 | TUI foundation, actions, config, task, runtime, terminal, and mock tests | Pass |
| 2 | local observer/parser/process/fixture and stale-state tests | Pass |
| 3 | local operator, mutation truth, preference, account, and handoff tests | Pass |
| 4 | Serve/Funnel/Taildrop/Taildrive/certificate/metrics service tests | Pass |
| 5 | Control API contracts, auth, scopes, resource isolation, and read-only tests | Pass |
| 6 | admin mutation, verification, conflict, audit, route, DNS, and user tests | Pass |
| 7 | credential, policy, temporary-file, redaction, one-time secret, and terminal tests | Pass |
| 8 | health, flows, webhooks, log streams, saved views, exports, Access Explorer, and UI tests | Pass |

The pre-Phase 9 baseline passed:

```text
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
```

The post-Phase 9 locked suite re-runs all prior integration tests and the new
Phase 9 gates. Real Tailscale, platform, named-terminal, and maintainer-keyring
evidence remains governed by `docs/support.md` and is not implied by these
fictional/fake-adapter tests.
