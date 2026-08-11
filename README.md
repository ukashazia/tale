# Tale

Tale is a keyboard-first Tailscale workspace for your terminal. Use it to see
what is happening on this machine, explore a tailnet, and manage common
Tailscale resources without jumping between commands and the admin console.

> Tale is experimental while its platform support matrix is being finalized.

![Tale showing a device inventory and inspector](docs/assets/tale-devices.png)

## What can Tale do?

- Inspect the local Tailscale client, peers, preferences, routes, and services.
- Browse tailnet devices, users, DNS, access policy, credentials, and audit data.
- Run diagnostics, review task history, and export redacted data.
- Make changes through preview and confirmation flows. `--read-only` disables
  mutations for an entire session.

Tale works in local-only, admin-only, or combined mode. The Tailscale client is
optional: it is needed only for views and actions involving the current
machine. Tailnet administration uses the Control API and does not require the
`tailscale` command or a local Tailscale daemon.

## Install

### Homebrew

```sh
brew install ukashazia/tale/tale
```

### Nix

```sh
nix profile install 'github:ukashazia/tale?dir=packaging/nix'
```

### Debian or Ubuntu

Download the `.deb` for your architecture from the
[latest release](https://github.com/ukashazia/tale/releases/latest), then run:

```sh
# x86_64 / amd64
sudo apt install ./tale_VERSION_amd64.deb

# ARM64
sudo apt install ./tale_VERSION_arm64.deb
```

Replace `VERSION` with the release version in the downloaded filename.

### Arch Linux (AUR)

Using an AUR helper:

```sh
yay -S tale-bin
# or
paru -S tale-bin
```

### Build from source

From a checkout, with a stable Rust toolchain:

```sh
cargo install --locked --path .
```

Once Tale is installed, launch it with:

```sh
tale
```

Install Tailscale separately only if you want Tale to inspect or operate the
current machine.

On your first run:

1. Press `:` and choose `local` to inspect this machine.
2. Press `:` and choose `devices` to see its peers.
3. Press `/` to filter the current list and `Enter` to inspect a row.
4. Press `?` at any time for help, or `q` to quit.

## Connect a tailnet

Create a profile when you want tailnet-wide inventory or administrative
actions. On a machine that also runs Tailscale:

```sh
tale auth add ops
tale --profile ops --read-only
```

For admin-only use without the `tailscale` command or daemon:

```sh
tale auth add ops
tale --no-local --profile ops --read-only
```

`--no-local` skips both CLI discovery and the local daemon connection. It does
not disable profile-backed Control API views or actions.

`auth add` prompts for the tailnet, credential type, and secret. Start with a
least-privilege credential and keep `--read-only` enabled until you intentionally
need write actions. Tale stores credentials separately from its shareable
configuration.

To add an access token without an interactive secret prompt:

```sh
printf '%s' "$TOKEN" |
  tale auth add ops --tailnet TAILNET_ID --kind access-token --secret-stdin
```

## Essential keys

| Key | Action |
| --- | --- |
| `:` | Go to a view |
| `/` | Filter the current view |
| `a` | Show available actions |
| `Enter` | Open or focus the selected item |
| `[` / `]` | Go backward / forward |
| `r` / `R` | Refresh this view / all sources |
| `?` | Show contextual help |
| `Esc` | Cancel the current prompt |
| `q` | Quit |

Actions that can destroy data require typed confirmation; they are never bound
to a single direct key.

## Useful commands

```sh
tale config path   # show config, state, and cache locations
tale config check  # validate configuration without opening the UI
tale config show   # show resolved settings and where they came from
tale doctor        # print a redacted, non-mutating diagnostic report
tale --no-local    # run on a workstation without a local Tailscale client
```

Generate shell completions with:

```sh
tale gen-completions --shell "$SHELL"
```

The complete command reference is available through `tale --help` and the
generated [`tale(1)` man page](docs/cli/tale.1).

## Troubleshooting

- Run `tale config check` when Tale rejects its configuration.
- Run `tale doctor` for a redacted report that is safe to inspect locally.
- Use `--tailscale-path PATH` if the Tailscale executable is not on `PATH`.
- Use `NO_COLOR=1 tale` if terminal colors render incorrectly.
- If the terminal is left in an unusual state after an external interruption,
  use your shell's normal `reset` command.

Tale never uploads diagnostic data automatically. To write a report to a new
file for review, run `tale doctor --output support.json`.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --locked
```

Regenerate the man page and completions after changing the CLI:

```sh
cargo run --locked --bin generate-artifacts -- --output-dir .
```
