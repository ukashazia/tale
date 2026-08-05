# Invocation prompt — Specification 12

Copy the prompt below into a fresh Codex agent session opened at the Tale
repository root after Specification 11 has passed.

```text
Implement Tale Specification 12, “Interaction shell and stack navigation,” end
to end.

Repository and authority

- Read AGENTS.md completely before doing anything else and obey it literally.
- Use only Jujutsu. Never invoke Git, modify history, or push.
- Inspect `jj status` before edits. Confirm Specification 11 is present and its
  exit gate is green. If it is not, stop and report that prerequisite.
- If the working copy is clean and no Phase 12 change already exists, start:

    jj new -m "feat: redesign interaction shell and navigation"

- Preserve all pre-existing changes. Stop on an overlapping unknown change.

Required reading

Read completely:

  AGENTS.md
  docs/specs/11-local-daemon-event-transport.md
  docs/specs/12-interaction-shell-and-navigation.md
  docs/architecture.md
  docs/product.md
  docs/ux.md
  docs/configuration.md
  docs/security.md

Inspect the action registry, App/reducer/event/effect types, route/view state,
filter and saved-view domains, all input handling, layout and overlay rendering,
footer/help/forms/confirmations, Services navigation, UI tests, snapshots, and
acceptance journeys. Search for `route_stack`, `ActionPicker`, `CopyPicker`,
`CommandPalette`, `FilterEditor`, q/Esc back behavior, and bracket bindings.

Implementation objective

Replace the centered command/filter/action/copy picker UX with the exact
bottom-anchored interaction grammar in Specification 12:

- inline `:` command prompt and `/` filter prompt;
- schema-aware Tab/Shift+Tab completion;
- direct transient `a` action and `y` copy key menus;
- contextual which-key-style bottom help on `?`;
- browser-style bounded view history on `[` and `]`;
- `q` for safe quit and Esc only for active interaction cancellation;
- centered modal rendering only for alerts and confirmations;
- `H`/`L` for Services sibling-section movement.

Implement the complete state machine, key precedence, editors, history
restoration, stable key sequences, action-registry validation, mouse/resize
parity, accessibility behavior, rendered-buffer assertions, and acceptance
journeys. Preserve all existing domain actions, typed forms, risk tiers,
confirmation phrases, tasks, source isolation, mutation verification, secret
lifecycle, and LocalAPI behavior.

Removal is part of the feature. Delete obsolete picker variants, state,
renderers, handlers, snapshots, and `route_stack`; do not leave old UI behind a
flag or compatibility path.

Non-negotiable constraints

- Do not implement Specification 13 or 14.
- Do not introduce the semantic palette beyond using the existing theme
  abstraction needed to keep this phase style-safe.
- Do not add key remapping, plugins, macros, shell commands, or a permanent
  sidebar.
- Do not copy jjui, k9s, or which-key.nvim source. Behavioral inspiration only.
- Do not alter action risk/verification semantics.
- Do not use unsafe, panic, unwrap, or expect.
- Do not contact or mutate a real daemon, CLI, tailnet, Control API, keyring,
  clipboard, remote, or release.
- Do not preserve the old centered pickers as fallback.

Working method

1. Baseline the current check suite and interaction snapshots.
2. Define the interaction mode and ViewHistory types/invariants.
3. Make the action registry the one runtime binding source and validate every
   contextual sequence.
4. Implement the bottom region, editors, completions, transients, and help.
5. Migrate route navigation and Services collisions.
6. Remove old states/components and update every handler/render path.
7. Add reducer/property/render/acceptance tests, including all breakpoints.
8. Update architecture, product, UX, configuration, and troubleshooting docs.
9. Run the complete exit gate and inspect the final JJ diff for obsolete paths.

Use stable resource identities, bounded history/candidate buffers, generations
for stale async results, and pure rendering. Test rendered cell coordinates; a
state-only test does not prove that a surface is bottom anchored.

Required validation

At minimum run:

  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features --locked

Run all focused action, reducer, filter, saved-view, Services, UI snapshot,
mouse, acceptance, documentation, terminal-restoration, and Specification 11
regression gates named by Specification 12.

Final response

Report:

- outcome and JJ change ID/description;
- the final interaction state machine and history behavior;
- old components/states removed and binding collisions resolved;
- rendered-buffer and acceptance evidence by viewport/input method;
- exact checks and results;
- any remaining issue or not-proven external evidence;
- confirmation that Specification 13/14 was not implemented and no Git,
  history, push/publish, or real-state mutation occurred.

Do not stop at a plan. Finish every in-scope requirement and gate unless a
specific blocker requires new maintainer authority.
```
