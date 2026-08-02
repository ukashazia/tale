# Tale

Tale is a keyboard-first Tailscale terminal application built with Rust and
Ratatui. Its goal is to combine local-client operation, tailnet administration,
and network diagnostics in one coherent interface.

Tale is currently in research and design. The executable remains a scaffold;
the documents below define the product and implementation contract before code
is added.

## Design documents

- [Research and constraints](docs/research.md)
- [Product definition and feature catalog](docs/product.md)
- [Interaction and user-flow specification](docs/ux.md)
- [Application architecture](docs/architecture.md)
- [Configuration contract](docs/configuration.md)
- [End-to-end feature plan](docs/roadmap.md)

## Implementation specifications

- [01 — TUI foundation](docs/specs/01-tui-foundation.md)
- [02 — Local observer](docs/specs/02-local-observer.md)
- [03 — Local operator](docs/specs/03-local-operator.md)
- [04 — Local services](docs/specs/04-local-services.md)

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
