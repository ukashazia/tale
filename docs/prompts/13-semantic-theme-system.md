# Invocation prompt — Specification 13

Copy the prompt below into a fresh Codex agent session opened at the Tale
repository root after Specification 12 has passed.

```text
Implement Tale Specification 13, “Tailscale-inspired semantic theme system,”
end to end.

Repository and authority

- Read AGENTS.md first and obey it literally.
- Use Jujutsu only. Never invoke Git, rewrite history, or push.
- Inspect `jj status` and prove Specification 12 is present and green before
  editing. If it is not, stop with evidence.
- If clean and no Phase 13 change already exists, start:

    jj new -m "feat: add Tailscale-inspired theme system"

- Preserve unknown existing work and stop if it overlaps this phase.

Required reading and research

Read completely:

  AGENTS.md
  docs/specs/12-interaction-shell-and-navigation.md
  docs/specs/13-semantic-theme-system.md
  docs/architecture.md
  docs/product.md
  docs/ux.md
  docs/configuration.md
  docs/support.md
  docs/security.md

Inspect every UI component/view and every style-producing call. Search all Rust
sources for `Color::`, `.fg(`, `.bg(`, `Style::default()`, and modifier usage.

Before palette implementation, research the official public Tailscale sources
required by the spec, pin retrieval dates/artifacts, and create Decision 0005
plus the complete token ledger. Verify numeric palette facts from primary
sources; do not rely on memory or scrape at build/runtime.

Implementation objective

Build a semantic theme system that completely describes Tale's visual
hierarchy and meaning. Implement:

- `tailscale-dark`, `tailscale-light`, and `terminal` built-ins;
- truecolor, ANSI-256, ANSI-16, and no-color projections;
- exhaustive semantic roles for surfaces, structure, text, focus, selection,
  operational state, source, risk, tasks, diffs, secrets, and redaction;
- typed, documented style composition precedence;
- full migration of every widget/view to semantic roles;
- configuration/provenance and Settings preview/apply/cancel behavior;
- contrast, distinguishability, source-policy, rendered-buffer, and supported
  platform visual evidence.

Treat colors as semantic state. Pending is never green; local/admin provenance
is distinct from health; public exposure is risk-emphasized; focus and selection
remain different; no-color retains every meaning through symbols, labels,
borders, and modifiers.

After migration, literal Ratatui colors may exist only inside the theme module
and theme tests. Add the required source-policy test and remove obsolete color
helpers instead of aliasing them.

Non-negotiable constraints

- Do not implement Specification 14.
- Do not add custom theme files, arbitrary RGB config, Base16 import, theme
  plugins, automatic terminal appearance probing, animation, or fallback names.
- Do not claim exact private admin-console reproduction or endorsement.
- Do not change interaction, route, domain, source, risk, or mutation semantics.
- Do not use unsafe, panic, unwrap, or expect.
- Do not accept snapshots mechanically; inspect the rendered meaning.
- Do not contact or mutate real local/remote state, credentials, clipboard,
  repository remotes, or release systems.

Working method

1. Baseline checks and capture current style inventory.
2. Create the research decision and token ledger with contrast/projection data.
3. Implement immutable role/palette/projection types.
4. Migrate foundational layout and components, then every view.
5. Add configuration and Settings session preview/apply/cancel.
6. Enforce the no-literal-colors source policy.
7. Run the full 3 theme × 4 capability × viewport render matrix and manual
   evidence available on each claimed Supported platform.
8. Update architecture, product, UX, configuration, support, security, and
   troubleshooting documentation.
9. Run the exit gate and audit the final JJ diff for style escape hatches.

Keep theme lookup allocation-free on the render path and theme switching free
of I/O, adapter restart, history changes, or source clones.

Required validation

At minimum run:

  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-targets --all-features --locked

Also run every contrast, source-policy, theme matrix, UI buffer, config,
reducer, acceptance, documentation, accessibility, terminal-restoration, and
Specifications 11–12 regression gate required by Specification 13.

Final response

Report:

- outcome and JJ change ID/description;
- official source provenance and decision/token-ledger files;
- final themes, projections, semantic roles, and composition rules;
- widget/view migration and old literal/helper removal;
- contrast and render matrix results, including platform evidence not proven;
- exact checks and results;
- any remaining issue;
- confirmation that Specification 14 was not implemented and no Git, history,
  push/publish, or real-state mutation occurred.

Continue through implementation and verification; do not return only a plan.
Stop only for a concrete requirement needing maintainer authority.
```
