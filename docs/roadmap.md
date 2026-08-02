# Delivery plan

The project grows through complete vertical slices. Each phase must remain a
usable product and must not land disabled architecture for a later phase.

## Phase 0 — research and contracts

Deliverables:

- product, UX, architecture, configuration, and research documents;
- explicit supported-surface boundary;
- feature priorities and open research questions.

Exit criteria:

- the documents agree on routes, capability states, safety tiers, sources, and
  configuration names;
- implementation can start without inventing product behavior;
- public-contract gaps are named.

## Phase 1 — local observer

This is the smallest useful end-to-end Tale.

Scope:

- terminal lifecycle and responsive frame;
- Model–Update–View loop, effects, tasks, contextual help;
- local executable/version/status detection;
- Overview and Devices collection/inspector;
- source freshness and last-good-snapshot behavior;
- filter, stable sort, navigation, copy picker;
- one cancellable `tailscale ping` task with streamed result summary;
- mock mode and versioned fictional fixtures;
- config path/check with only the fields used by this phase.

Acceptance criteria:

- useful without a config file or network credential;
- missing binary, daemon unavailable, permission denied, logged out, and running
  render as distinct states;
- selection survives refresh and sorting by stable ID;
- no shell is used for CLI execution;
- cancelling ping leaves input and terminal state functional;
- 60x18, 80x24, 110x30, no-color, and ASCII snapshots pass;
- no application path uses panic, unwrap, expect, or unsafe.

## Phase 2 — local operator

Scope:

- local preferences and connect/disconnect;
- exit-node selection with latency context;
- route advertisement editing;
- DNS status/query, whois, and netcheck;
- account list/switch and explicit interactive handoff flows;
- SSH and `nc` terminal handoff;
- Serve and Funnel status/edit/reset;
- Taildrop transfer and progress;
- task history and exact redacted argv preview.

Acceptance criteria:

- every mutation has a typed preview and post-action verification read;
- Funnel enable is treated as public exposure and risk tier 2;
- interactive children restore the TUI after success, non-zero exit, and signal;
- Tale never prompts for or invokes sudo;
- unsupported client commands are capability states, not missing UI.

## Phase 3 — read-only admin

Scope:

- profile configuration and OS-keyring credentials;
- scoped OAuth-client exchange and temporary access-token override;
- Control API client, pagination, source metadata, and typed errors;
- read-only Devices, Users, Routes, DNS, Access, Credentials, and configuration
  audit views for documented endpoints;
- combined local/admin device inspector using exact stable IDs only;
- explicit read-only mode and missing-scope explanations;
- Overview queues for approvals, expiry, stale source data, and version skew.

Acceptance criteria:

- local mode continues when admin authentication fails and vice versa;
- secrets never appear in URLs, logs, debug output, task history, or config;
- `403`, expired credential, plan restriction, rate limit, and transport failure
  are distinct;
- each API DTO has an HTTP contract fixture;
- no mutation endpoint is called in this phase.

## Phase 4 — admin operator

Scope, in this order:

1. device rename, tags, approval, key expiry, and removal;
2. route approval and revocation;
3. DNS editing;
4. user approval, suspension, restore, role, and deletion;
5. auth-key creation and supported credential revocation;
6. policy external edit, remote-change protection, validate, preview, tests,
   diff, and save.

Acceptance criteria:

- action availability follows scopes and read-only locks;
- tier-3 actions require typed confirmation and a fresh preflight fetch;
- secret results are view-once and absent from history after close;
- batch operations report per-target outcomes;
- policy source comments/formatting survive unchanged when the user does not
  change them;
- policy save is blocked after a concurrent remote change;
- each mutation is followed by resource verification and can link to an audit
  event when one becomes available.

## Phase 5 — operational depth

Scope:

- fleet-health findings and saved views;
- network flow logs and aggregations where the plan permits;
- webhook and log-streaming management;
- Taildrive and certificate workflows;
- export with source timestamp/query metadata;
- question-driven Access Explorer backed only by Tailscale preview/tests;
- CIDR overlap and route-advertiser analysis;
- optional mouse support.

Acceptance criteria:

- every finding exposes observed facts and timestamps;
- derived warnings are visually distinct from Tailscale-reported failures;
- exports are secret-free and deterministic;
- flow-log UI never implies packet contents are available.

## Research gates

Features move from `research` only after their public contracts are verified:

- Tailscale Services and discovered endpoints;
- device sharing and invitations;
- Tailnet Lock administration and signing;
- OAuth apps acting on behalf of users;
- client update orchestration;
- identity-provider and organization settings.

For each feature, add the primary-source contract, error/permission model,
fixtures, UX flow, safety tier, and phase acceptance criteria before code.

## Deferred customization

Custom keybindings, themes, aliases, and external actions are intentionally
deferred until core action IDs and views are stable. When considered:

- keymaps bind to action IDs, never implementation functions;
- aliases resolve only to Tale routes and structured filters;
- external actions receive explicit, non-secret context fields and never run by
  default;
- configuration changes replace the current schema directly—no compatibility
  layers or migrations.

## Release definition

A phase is complete only when its entire scope works end to end, documentation
matches behavior, and verification passes. Partially wired menus, placeholder
views, unused abstractions, and “coming soon” controls do not count as progress
and should not be merged into the working product.

