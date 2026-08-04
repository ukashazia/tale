# Tale

Tale is a keyboard-first Tailscale terminal application built with Rust and
Ratatui. Its goal is to combine local-client operation, tailnet administration,
and network diagnostics in one coherent interface.

Tale is an implementation-stage terminal application. The hardening work keeps
support claims evidence-based; see `docs/support.md` before treating any
platform or client combination as supported.

## Design documents

- [Research and constraints](docs/research.md)
- [Product definition and feature catalog](docs/product.md)
- [Interaction and user-flow specification](docs/ux.md)
- [Application architecture](docs/architecture.md)
- [Configuration contract](docs/configuration.md)
- [End-to-end feature plan](docs/roadmap.md)
- [Support matrix](docs/support.md)
- [Installation](docs/install.md)
- [Security review](docs/security.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)
- [Release checklist](docs/release-checklist.md)
- [`tale(1)` man page](docs/cli/tale.1)

## Implementation specifications

- [01 — TUI foundation](docs/specs/01-tui-foundation.md)
- [02 — Local observer](docs/specs/02-local-observer.md)
- [03 — Local operator](docs/specs/03-local-operator.md)
- [04 — Local services](docs/specs/04-local-services.md)
- [05 — Admin observer](docs/specs/05-admin-observer.md)
- [06 — Admin operator](docs/specs/06-admin-operator.md)
- [07 — Access, credentials, and audit security](docs/specs/07-access-security.md)
- [08 — Operational depth](docs/specs/08-operational-depth.md)
- [09 — Tale 1.0 hardening](docs/specs/09-one-zero-hardening.md)

Each specification is an implementation contract for one roadmap phase. It
defines feature behavior, code ownership, adapter boundaries, actions, error
states, tests, manual journeys, and the phase exit gate.

## Product boundary

Tale uses supported Tailscale surfaces instead of scraping the admin console:

- the installed `tailscale` command for local-node state and operations;
- the documented Tailscale Control API for tailnet-wide inventory and
  administration.

If Tailscale exposes a feature only in the web console, Tale reports that
limitation plainly. It does not emulate the console through browser automation
or depend on undocumented endpoints.

Local installation and local-node operation do not imply admin access. Admin
mode requires a separately configured profile and least-privilege Control API
credential. Tale never uploads doctor bundles or support data automatically.
