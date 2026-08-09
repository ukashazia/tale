# Tale package sources

This directory contains distribution glue for the same generated payload: the
`tale` executable, `tale(1)`, Bash/Zsh/Fish completions, and notices. It does
not install a service, modify shell startup files, install Tailscale, or handle
credentials.

## Release assets

For each Supported target, build the binary, generate the checked-in CLI
artifacts, and call the deterministic `package-artifact` tool from
`docs/release-checklist.md`. Then compress the resulting `.tar` with
`packaging/release/compress-archive.sh --input release/tale-TARGET.tar --output release/tale-TARGET.tar.zst`.

Publish the raw `tale-TARGET` executable alongside `.tar.zst` and `.tar.gz`
payloads and their SHA-256 manifests. The raw executable is the universal
fallback; `.tar.zst` is the Arch payload, while `.tar.gz` supports Homebrew and
the portable installer without extra decompression tooling.

## Debian

Build a native Debian package on a runner for the target architecture. For
example: `packaging/debian/package-deb.sh --binary target/TARGET/release/tale
--target TARGET --version VERSION --output release/tale_VERSION_ARCHITECTURE.deb`.

The package lists Tailscale as an optional suggestion and installs the man page
and completions in standard Debian paths.

## Homebrew

Keep the public formula in a dedicated `homebrew-tap` repository. Copy
`homebrew/tale.rb.in` to that repository as `Formula/tale.rb`, replace every
`@…@` value from the signed release manifest, and publish the tap only after
the macOS targets are Supported. The formula selects the ARM64 or Intel archive
and installs the executable, man page, and completions using Homebrew paths.

## Arch

`arch/PKGBUILD.in` is the source for the companion AUR package. Replace its
`@…@` values from the signed release manifest, using `x86_64` for the x86_64
Linux target. `makepkg` produces the native `.pkg.tar.zst` that pacman can
install; the release archive alone is not represented as an Arch package.

## Nix

`nix/flake.nix` is separate from the root development flake. It exposes
`packages.default` for every declared Nix system and installs the man page plus
Bash/Zsh/Fish completions. Package availability is not a support claim;
`docs/support.md` remains authoritative.
