#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/.." && pwd)
assets_dir="${repo_root}/docs/assets"
tapes_dir="${script_dir}/screenshots"
capture_nixpkgs='https://api.flakehub.com/f/pinned/DeterminateSystems/nixpkgs-weekly/0.1.1042126%2Brev-624af665418d3c65d544145b4d34ad696439570e/019fcb6c-e772-7cb3-baa0-211e12b79e38/source.tar.gz'

if command -v vhs >/dev/null 2>&1; then
  vhs_command=(vhs)
elif command -v nix >/dev/null 2>&1; then
  vhs_command=(nix run "${capture_nixpkgs}#vhs" --)
else
  printf '%s\n' 'capture requires vhs or Nix to provide it' >&2
  exit 1
fi

if command -v ffmpeg >/dev/null 2>&1; then
  ffmpeg_command=(ffmpeg)
elif command -v nix >/dev/null 2>&1; then
  ffmpeg_command=(nix run "${capture_nixpkgs}#ffmpeg-headless" --)
else
  printf '%s\n' 'capture requires ffmpeg or Nix to provide it' >&2
  exit 1
fi

if command -v timeout >/dev/null 2>&1; then
  timeout_command=(timeout)
elif command -v gtimeout >/dev/null 2>&1; then
  timeout_command=(gtimeout)
elif command -v nix >/dev/null 2>&1; then
  timeout_command=(nix shell "${capture_nixpkgs}#coreutils" -c timeout)
else
  printf '%s\n' 'capture requires timeout, gtimeout, or Nix to provide it' >&2
  exit 1
fi

mkdir -p "${assets_dir}"
cd "${repo_root}"

cargo build --locked --bin tale

intermediates=(
  "${assets_dir}/.tale-devices.mp4"
  "${assets_dir}/.tale-services.mp4"
  "${assets_dir}/.tale-navigation.mp4"
)

cleanup() {
  rm -f -- "${intermediates[@]}"
}
trap cleanup EXIT INT TERM

capture_tape() {
  local tape=$1
  local attempt

  for attempt in 1 2; do
    if "${timeout_command[@]}" --kill-after=5s 45s "${vhs_command[@]}" "${tape}"; then
      return 0
    fi

    if [[ ${attempt} -eq 1 ]]; then
      printf 'capture stalled; retrying %s\n' "${tape}" >&2
    fi
  done

  return 1
}

for name in tale-devices tale-services tale-navigation; do
  capture_tape "${tapes_dir}/${name}.tape"
  "${ffmpeg_command[@]}" \
    -y \
    -hide_banner \
    -loglevel error \
    -sseof -0.05 \
    -i "${assets_dir}/.${name}.mp4" \
    -frames:v 1 \
    -map_metadata -1 \
    "${assets_dir}/${name}.png"
done

printf '%s\n' 'Updated README screenshots in docs/assets.'
