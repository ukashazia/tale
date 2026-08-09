# Installing Tale

Tale is a local terminal application. This repository currently declares no
Supported release artifact; the commands below are for local development and
for a maintainer’s evidence run.

## Prerequisites

- a documented stable Rust toolchain;
- an installed Tailscale client when local observation or operation is wanted;
- a terminal with keyboard input and an alternate-screen implementation for the
  interactive UI.

The current fixture evidence covers Tailscale 1.98.9 on Linux only. Admin mode
also requires a Control API credential with the least-privilege scopes needed
for the resources being inspected or changed. Read-only profiles should use
read-only credentials whenever the Control API permits it.

## Install a release

When a platform row is marked Supported in the support matrix, release assets
are available for both ARM64 and x86_64 where that operating system supports
them. Choose the native channel for your system:

- macOS: the project's Homebrew tap installs Tale, `tale(1)`, and Bash/Zsh/Fish
  completions;
- Nix: install the package exposed by the separate release flake in
  `packaging/nix`;
- Debian-family Linux: install the architecture-matched `.deb` release asset;
- Arch Linux: install the companion AUR package, which produces a native
  `.pkg.tar.zst` package;
- other supported Unix systems: use the matching raw `tale-TARGET` executable.

No distribution channel installs Tailscale, starts a service, modifies shell
startup files, or handles credentials. Tale requires an installed Tailscale
client only for local observation or operation.

The current matrix contains no Supported platform rows, so these channels must
not yet be advertised as production installations.

## Portable installer

Once releases are published, the fallback installer is served directly from the
repository and downloads a verified payload from the latest GitHub release:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://github.com/ukashazia/tale/releases/latest/download/install.sh | sh
```

The release workflow renders the installer with the repository that published
the release. It supports macOS and Linux on ARM64 and x86_64. Before installing it downloads
the matching `.tar.gz` release payload and SHA-256 file, verifies them, then
installs the executable, man page, and completions beneath `~/.local`. It does
not use `sudo`, edit shell profiles, install Tailscale, or handle credentials.
Use `curl --proto '=https' --tlsv1.2 -fsSL URL | TALE_INSTALL_PREFIX=/path sh`
to choose a prefix, or download and inspect the script before running it.

## Generate shell completions

Tale can generate current completion material from its embedded CLI definition:

```sh
tale gen-completions --shell "$SHELL"
```

Supported shells are `bash`, `zsh`, and `fish`; their executable paths are also
accepted. Redirect the output to a
shell-specific completion directory when using a custom installation layout.

## Build from this checkout

Use the locked dependency graph:

```text
cargo install --locked --path .
cargo run --locked --bin generate-artifacts -- --output-dir .
```

The second command regenerates the checked-in Bash, Zsh, Fish, and `tale(1)`
artifacts from the typed CLI definition. It does not download runtime assets.

For a local release build, use `cargo build --release --locked` and follow the
artifact procedure in `docs/release-checklist.md`. Only target rows with dated
support evidence may be packaged as Supported artifacts.

## Configuration and credentials

`tale config path` prints the resolved config, state, and cache locations.
`tale config check` validates the selected configuration. Profiles name a
tailnet and a credential reference; credential values belong in the OS keyring
or the explicitly documented environment override, never in a support bundle
or command-line argument.

`--mock` is an internal test route and is hidden from user help and the man
page. It is not a production transport and must not be used as platform
evidence.

## Updates and uninstall

Install a new checkout with the same locked process, then rerun the complete
verification sequence. Do not copy credentials into a new config file.

Removing the binary does not remove user data. To uninstall, remove the
installed executable and separately remove Tale’s profile entries and keyring
records using the explicit `tale auth remove PROFILE` command when they are no
longer needed. Inspect `tale config path` first; never delete the entire state
directory merely to repair a damaged config.
