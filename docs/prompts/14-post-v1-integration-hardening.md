# Invocation prompt — Specification 14

Copy the prompt below into a fresh Codex agent session opened at the Tale
repository root after Specifications 11–13 have passed.

```text
Implement Tale Specification 14, “Post-1.0 integration hardening,” end to end.

Repository and authority

- Read AGENTS.md first and obey it literally.
- Use Jujutsu only. Never invoke Git, modify history, or push.
- Inspect `jj status`. Prove Specifications 11, 12, and 13 and their decisions,
  tests, and exit gates are present. If a prerequisite is incomplete, stop and
  report exact evidence; do not silently implement the missing phase here.
- If the working copy is clean and no Phase 14 change exists, start:

    jj new -m "chore: harden post-v1 architecture and interface"

- Preserve unrelated work and stop before overlapping unknown changes.

Required reading

Read completely:

  AGENTS.md
  docs/specs/10-v1-release-audit.md
  docs/specs/11-local-daemon-event-transport.md
  docs/specs/12-interaction-shell-and-navigation.md
  docs/specs/13-semantic-theme-system.md
  docs/specs/14-post-v1-integration-hardening.md
  docs/architecture.md
  docs/product.md
  docs/ux.md
  docs/configuration.md
  docs/support.md
  docs/security.md
  docs/install.md
  docs/troubleshooting.md
  docs/release-checklist.md

Read every decision, contract, phase-gate, benchmark, terminal-evidence, and
acceptance document referenced by those specs. Obtain the maintainer-supplied
Specification 10 audit report if it is not in the repository; do not invent its
findings.

Implementation objective

Integrate, audit, fault-test, and harden the completed daemon transport,
interaction/navigation redesign, and semantic theme system as one product. Add
no new product domain.

Implement every invariant, failure-injection row, Journey A–H, performance and
resource budget, security/privacy check, platform/terminal matrix, obsolete-path
audit, documentation update, artifact dry run, and exit condition in
Specification 14. Fix every in-scope defect discovered. Do not merely write a
report when the spec authorizes and requires implementation.

Pay particular attention to cross-feature races:

- daemon events during prompts, help, history restoration, and mutation
  verification;
- stale watcher/read/completion generations;
- capability changes while transients/help are open;
- selection/confirmation identity after refresh/removal;
- theme/resize changes during every interaction mode;
- source isolation during concurrent local/admin failures;
- shutdown and terminal restoration while watcher/tasks/modals are active.

Removal is mandatory. Audit current implementation/docs/artifacts and remove
all obsolete polling, preference-only transport, old config, old picker,
route-stack/key behavior, style escape hatch, and LocalAPI fallback paths named
by Specification 14. Historical numbered specifications remain historical and
need not be rewritten.

Non-negotiable constraints

- Do not add new resources, API endpoints, mutations, customization systems,
  plugins, key remapping, or compatibility paths.
- Do not weaken Specifications 11–13 or relax budgets to fit results.
- Do not use unsafe, panic, unwrap, or expect.
- Do not publish, push, sign with user credentials, or change repository
  remotes/history.
- Do not mutate a real daemon, tailnet, Control API, keyring, clipboard,
  configuration, or service during automated/default validation.
- Optional real observation requires explicit maintainer authorization and is
  read-only under Specification 14. Absence is `NOT PROVEN`, not inferred pass.
- Do not claim a platform based on cross-compilation or another platform's
  transport evidence.

Working method

1. Capture the exact preflight/baseline and create the post-v1 phase-gate
   document with every gate initially `PENDING`.
2. Model and test the integrated transition invariants.
3. Build deterministic fault injection at adapter/runtime boundaries.
4. Implement and run Journeys A–H with fake isolated adapters and rendered
   buffer evidence.
5. Measure and fix performance, idle resource use, cancellation, and soak
   behavior.
6. Re-audit security, privacy, dependencies, terminal lifecycle, platforms, and
   artifacts.
7. Remove every obsolete current path and reconcile all current docs.
8. Run the required check sequence, benchmark suite, artifact comparison,
   package dry run, and available platform/manual matrices.
9. Inspect final JJ status/diff and complete the evidence document truthfully.

Do not use sleeps to hide races, unbounded queues, renderer mutation, snapshot
clones, or fallback behavior. Use deterministic time and fake transports.

Required validation

Run the exact ordered sequence in Specification 14, including at minimum:

  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features --locked
  cargo test --locked --test documentation
  cargo test --locked --test terminal_restore
  cargo test --locked --test acceptance
  cargo test --locked --test compatibility
  cargo test --locked --test security

Then run every documented benchmark, advisory, artifact, package, support-row,
manual terminal, and source-policy gate. Record exact commands, versions,
results, skips, hashes, and sanitized environment. Do not call missing mandatory
evidence a pass.

Final response

Use the exact report contract in Specification 14. Include:

- outcome and JJ change ID/description;
- fixes grouped by transport, interaction, theme, lifecycle, security, and docs;
- exact checks/results and benchmark/resource table;
- supported/experimental/not-proven platform matrix;
- release artifacts and SHA-256 values;
- unresolved findings with severity/evidence;
- explicit confirmation of no Git, history rewrite, push/publish, real mutation,
  or credential operation.

Do not stop at planning. Continue until all in-scope gates pass or a concrete
blocker requires authority or unavailable platform evidence. Be truthful about
every `NOT PROVEN` result.
```
