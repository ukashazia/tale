#!/bin/sh
set -eu

usage() {
  echo 'usage: package-deb.sh --binary PATH --target TARGET --version VERSION --output PATH' >&2
  exit 64
}

binary=
target=
version=
output=
while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  case "$1" in
    --binary) binary=$2 ;;
    --target) target=$2 ;;
    --version) version=$2 ;;
    --output) output=$2 ;;
    *) usage ;;
  esac
  shift 2
done

[ -n "$binary" ] && [ -n "$target" ] && [ -n "$version" ] && [ -n "$output" ] || usage
[ -f "$binary" ] || { echo "binary is not a regular file: $binary" >&2; exit 66; }
[ ! -e "$output" ] || { echo "output already exists: $output" >&2; exit 73; }

case "$target" in
  x86_64-unknown-linux-gnu) architecture=amd64 ;;
  aarch64-unknown-linux-gnu) architecture=arm64 ;;
  *) echo "unsupported Debian target: $target" >&2; exit 64 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
stage=$(mktemp -d)
cleanup() { rm -rf "$stage"; }
trap cleanup EXIT HUP INT TERM

mkdir -p "$stage/DEBIAN" "$stage/usr/bin" "$stage/usr/share/man/man1" "$stage/usr/share/bash-completion/completions" "$stage/usr/share/zsh/vendor-completions" "$stage/usr/share/fish/vendor_completions.d" "$stage/usr/share/doc/tale"
sed -e "s/@VERSION@/$version/g" -e "s/@ARCHITECTURE@/$architecture/g" "$root/packaging/debian/control.in" > "$stage/DEBIAN/control"
install -m 0755 "$binary" "$stage/usr/bin/tale"
install -m 0644 "$root/docs/cli/tale.1" "$stage/usr/share/man/man1/tale.1"
install -m 0644 "$root/completions/tale.bash" "$stage/usr/share/bash-completion/completions/tale"
install -m 0644 "$root/completions/_tale" "$stage/usr/share/zsh/vendor-completions/_tale"
install -m 0644 "$root/completions/tale.fish" "$stage/usr/share/fish/vendor_completions.d/tale.fish"
install -m 0644 "$root/LICENSE" "$root/NOTICE" "$root/README.md" "$stage/usr/share/doc/tale"
dpkg-deb --root-owner-group --build "$stage" "$output"
