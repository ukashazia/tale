#!/bin/sh
set -eu

repository='@REPOSITORY@'
version=latest
prefix=${TALE_INSTALL_PREFIX:-"$HOME/.local"}

if [ "$repository" = '@REPOSITORY@' ]; then
  echo 'install this script from a Tale GitHub release, not directly from the repository' >&2
  exit 65
fi

usage() {
  echo 'usage: install.sh [--prefix PATH] [--version TAG]' >&2
  exit 64
}

while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  case "$1" in
    --prefix) prefix=$2 ;;
    --version) version=$2 ;;
    *) usage ;;
  esac
  shift 2
done

case "$(uname -s)" in
  Darwin) operating_system=apple-darwin ;;
  Linux) operating_system=unknown-linux-gnu ;;
  *) echo "unsupported operating system: $(uname -s)" >&2; exit 69 ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) architecture=aarch64 ;;
  x86_64 | amd64) architecture=x86_64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 69 ;;
esac

target="$architecture-$operating_system"
asset="tale-$target.tar.gz"
if [ "$version" = latest ]; then
  base_url="https://github.com/$repository/releases/latest/download"
else
  base_url="https://github.com/$repository/releases/download/$version"
fi

command -v curl >/dev/null 2>&1 || { echo 'curl is required to install Tale' >&2; exit 69; }
umask 077
temporary_directory=$(mktemp -d)
cleanup() { rm -rf "$temporary_directory"; }
trap cleanup EXIT HUP INT TERM

archive="$temporary_directory/$asset"
checksum="$archive.sha256"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --output "$archive" "$base_url/$asset"
curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --output "$checksum" "$base_url/$asset.sha256"

expected_checksum=$(awk 'NR == 1 { print $1 }' "$checksum")
if [ -z "$expected_checksum" ]; then
  echo 'release checksum is empty' >&2
  exit 65
fi
if command -v shasum >/dev/null 2>&1; then
  actual_checksum=$(shasum -a 256 "$archive" | awk '{ print $1 }')
elif command -v sha256sum >/dev/null 2>&1; then
  actual_checksum=$(sha256sum "$archive" | awk '{ print $1 }')
else
  echo 'shasum or sha256sum is required to verify the Tale release' >&2
  exit 69
fi
if [ "$expected_checksum" != "$actual_checksum" ]; then
  echo 'release checksum verification failed' >&2
  exit 65
fi

tar -xzf "$archive" -C "$temporary_directory"
root="$temporary_directory/tale-$target"
if [ ! -f "$root/tale" ]; then
  echo 'release archive has an unexpected layout' >&2
  exit 65
fi
mkdir -p "$prefix/bin" "$prefix/share/man/man1" "$prefix/share/bash-completion/completions" "$prefix/share/zsh/site-functions" "$prefix/share/fish/vendor_completions.d"
install -m 0755 "$root/tale" "$prefix/bin/tale"
install -m 0644 "$root/docs/cli/tale.1" "$prefix/share/man/man1/tale.1"
install -m 0644 "$root/completions/tale.bash" "$prefix/share/bash-completion/completions/tale"
install -m 0644 "$root/completions/_tale" "$prefix/share/zsh/site-functions/_tale"
install -m 0644 "$root/completions/tale.fish" "$prefix/share/fish/vendor_completions.d/tale.fish"

echo "Installed Tale to $prefix/bin/tale"
case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) echo "Add $prefix/bin to PATH to run tale." ;;
esac
