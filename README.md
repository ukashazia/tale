# Tale

Tale is a keyboard-first Tailscale terminal application built with Rust and
Ratatui. Its goal is to combine local-client operation, tailnet administration,
and network diagnostics in one coherent interface.

Tale is an implementation-stage terminal application. The hardening work keeps
support claims evidence-based; see `docs/support.md` before treating any
platform or client combination as supported.

<p align="center">
  <a href="docs/assets/tale-devices.png">
    <img src="docs/assets/tale-devices.png" alt="Tale device inventory with the edge-relay inspector open" width="100%">
  </a>
</p>

<table>
  <tr>
    <td width="50%"><strong>Services and exposure</strong></td>
    <td width="50%"><strong>Resource-first navigation</strong></td>
  </tr>
  <tr>
    <td>
      <a href="docs/assets/tale-services.png">
        <img src="docs/assets/tale-services.png" alt="Tale services view showing public and tailnet-only mappings">
      </a>
    </td>
    <td>
      <a href="docs/assets/tale-navigation.png">
        <img src="docs/assets/tale-navigation.png" alt="Tale command palette grouping fleet, local, network, and operations routes">
      </a>
    </td>
  </tr>
</table>

<details>
<summary>Regenerate these screenshots</summary>

```sh
nix develop
./scripts/capture-readme-screenshots.sh docs/assets/readme-wallpaper.jpg
```

The script builds Tale, drives the deterministic mock TUI, and replaces all
three PNGs. The Nix development shell provides the pinned capture toolchain.
The bundled wallpaper is the free Unsplash image
[“Abstract diagonal lines of light blue fabric”](https://unsplash.com/photos/O0hlizfP-Kw)
by Logan Voss. Pass another image to use an arbitrary wallpaper instead:

```sh
./scripts/capture-readme-screenshots.sh /path/to/wallpaper.jpg
```

The wallpaper is center-cropped to each screenshot's dimensions. Omitting it
keeps the solid background defined by the VHS tapes.

</details>

## Project documents

These documents describe the product and its current contracts. Completed
implementation plans and point-in-time build evidence are intentionally not
kept in the main documentation set.

- [Design principles](DESIGN.md)
- [Product definition and feature catalog](docs/product.md)
- [Interaction and user flows](docs/ux.md)
- [Application architecture](docs/architecture.md)
- [Configuration contract](docs/configuration.md)
- [Architectural decisions](docs/decisions)
- [Control API contract ledger](docs/contracts/control-api-2026-08-03.md)
- [LocalAPI contract ledger](docs/contracts/localapi-1.98.9.md)
- [Support matrix](docs/support.md)
- [Installation](docs/install.md)
- [Security review](docs/security.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)
- [Release checklist](docs/release-checklist.md)
- [`tale(1)` man page](docs/cli/tale.1)

## Product boundary

Tale uses supported Tailscale surfaces instead of scraping the admin console:

- the configured LocalAPI socket or named pipe for local status, preferences,
  peer observation, and event-driven invalidation;
- the installed `tailscale` command only for typed local operations whose
  LocalAPI mutation contract is intentionally not adopted;
- the documented Tailscale Control API for tailnet-wide inventory and
  administration.

Local daemon observation, local CLI execution, and admin API access are
independent capabilities. Tale subscribes before its initial LocalAPI reads,
coalesces event invalidations into authoritative reads, retains last-good data
during reconnect, and never falls back to CLI status observation. The bottom
interaction shell uses `:` and `/` inline editors, `a`/`y` transient mnemonic
menus, `?` contextual help, and `[`/`]` bounded view history. Built-in
`tailscale-dark`, `tailscale-light`, and `terminal` themes preserve the same
state, source, and risk meanings through truecolor, reduced-color, and no-color
projections.

If Tailscale exposes a feature only in the web console, Tale reports that
limitation plainly. It does not emulate the console through browser automation
or depend on undocumented endpoints.

Local installation and local-node operation do not imply admin access. Admin
mode requires a separately configured profile and least-privilege Control API
credential. Tale never uploads doctor bundles or support data automatically.
