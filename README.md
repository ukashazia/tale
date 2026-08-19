# Tale

See what is happening across your Tailscale network—and manage it—without
leaving the terminal.

![Tale showing a device inventory and inspector](docs/assets/tale-devices.png)

Tale puts your devices, routes, services, DNS, access rules, and audit activity
in one keyboard-driven interface. It can inspect the Tailscale client on this
machine, connect to the Tailscale API to work with the whole network, or do
both at once.

Changes are never one accidental keypress away: Tale shows you what it is
about to do and asks for confirmation. Run it with `--read-only` when you do
not want the session to make any changes at all.

## Install Tale

### Homebrew

```sh
brew install ukashazia/tale/tale
```

### macOS or Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ukashazia/tale/releases/latest/download/tale-installer.sh | sh
```

### Nix

```sh
nix profile install 'tarball+https://github.com/ukashazia/tale/releases/latest/download/tale-nix-flake.tar.gz'
```

The Nix download contains the release's prebuilt binaries. To pin a version,
replace `releases/latest` with `releases/download/vX.Y.Z`.

Homebrew and Nix installations include completions for Bash, Zsh, and Fish.

<details>
<summary>Debian, Ubuntu, Arch Linux, and source builds</summary>

For Debian or Ubuntu, download the `.deb` for your architecture from the
[latest release](https://github.com/ukashazia/tale/releases/latest), then run:

```sh
# x86_64 / amd64
sudo apt install ./tale_VERSION_amd64.deb

# ARM64
sudo apt install ./tale_VERSION_arm64.deb
```

On Arch Linux, install the AUR package:

```sh
yay -S tale-bin
# or
paru -S tale-bin
```

To build from a checkout with a stable Rust toolchain:

```sh
cargo install --locked --path .
```

</details>

## Take a first look

Start Tale:

```sh
tale
```

Then:

1. Press `:` to choose where to go.
2. Open **Local** to inspect this machine, or **Devices** to see its peers.
3. Press `/` to filter a list and `Enter` to inspect the selected item.
4. Press `a` to see the actions available for the current item.

Press `?` whenever you need help, and `q` to quit.

## Connect the rest of your network

Tale can use the Tailscale client already running on this machine without any
extra setup. To see and manage devices and settings across the whole network,
give Tale a Tailscale API credential. `ops` is simply a name for this login:

Create one in the Tailscale admin console: use a scoped
[OAuth client](https://console.tailscale.com/admin/settings/trust-credentials)
for ongoing access, or generate an
[API access token](https://console.tailscale.com/admin/settings/keys) for a
simpler setup.

```sh
tale auth add ops
tale --profile ops --read-only
```

`auth add` asks for the network and credential details, then stores the secret
separately from your shareable configuration. Start with the smallest
permissions you need and keep `--read-only` enabled until you intend to make
changes.

On a machine without a local Tailscale client, skip local discovery:

```sh
tale --no-local --profile ops --read-only
```

For scripts, pass an access token through standard input instead of putting it
on the command line:

```sh
printf '%s' "$TOKEN" |
  tale auth add ops --tailnet TAILNET_ID --kind access-token --secret-stdin
```

## Keyboard shortcuts

| Key | What it does |
| --- | --- |
| `:` | Choose a view |
| `/` | Filter the current list |
| `Enter` | Inspect the selected item |
| `a` | Show available actions |
| `Tab` / `Shift-Tab` | Move between tabs |
| `[` / `]` | Go back or forward |
| `r` / `R` | Refresh this view or everything |
| `?` | Show help for the current view |
| `Esc` | Close or cancel the current prompt |
| `q` | Quit |

Actions that can destroy data always require typed confirmation.

## Configure Tale

Run `tale config path` to find your `config.toml`. You only need to add values
you want to change; Tale supplies the rest.

Here is a small starting point:

```toml
#:schema https://raw.githubusercontent.com/ukashazia/tale/main/docs/tale-config.schema.json

# Prevent this session from making changes.
read_only = true

[ui]
theme = "terminal" # tailscale-dark, tailscale-light, or terminal
mouse = false
time_zone = "local" # local or utc

[experimental_features]
saved_views = false
```

The checked-in [configuration schema](docs/tale-config.schema.json) documents
every setting and gives compatible editors completion and validation. Run
`tale config check` after editing the file.

Credentials do not belong in `config.toml`. Use `tale auth add`, which writes
them to a separate file with owner-only permissions. Never commit or share
that credential file.

Command-line flags override environment variables, which override
`config.toml`.

## Useful commands

```sh
tale config path   # show where Tale stores its files
tale config check  # check the configuration for errors
tale config show   # show the settings Tale will use
tale doctor        # show a redacted report without changing anything
tale --no-local    # run without a local Tailscale client
```

Generate completions for your shell with:

```sh
tale gen-completions --shell "$SHELL"
```

For every command and option, use `tale --help` or read the generated
[`tale(1)` man page](docs/cli/tale.1).

## Troubleshooting

- If Tale cannot find Tailscale, start it with `--tailscale-path PATH`.
- If colors look wrong, start it with `NO_COLOR=1 tale`.
- If an interruption leaves the terminal looking unusual, run `reset` in your
  shell.

Tale never uploads diagnostics automatically. To save a report that you can
inspect before sharing, run:

```sh
tale doctor --output support.json
```

## Development

Enter the pinned development environment and run the complete project checks:

```sh
nix develop
just check
```

After changing the command-line interface, regenerate the man page and shell
completions:

```sh
cargo run --locked --bin generate-artifacts -- --output-dir .
```
