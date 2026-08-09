#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/.." && pwd)
assets_dir="${repo_root}/docs/assets"
tapes_dir="${script_dir}/screenshots"
intermediates=(
  "${assets_dir}/.tale-devices.mp4"
  "${assets_dir}/.tale-services.mp4"
  "${assets_dir}/.tale-navigation.mp4"
)

cleanup() {
  rm -f -- "${intermediates[@]}"
}
trap cleanup EXIT INT TERM

for command in ffmpeg timeout vhs; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    printf 'capture requires %s; run it from `nix develop`\n' "${command}" >&2
    exit 1
  fi
done

mkdir -p "${assets_dir}"
cd "${repo_root}"

cargo build --locked --bin tale

capture_tape() {
  local tape=$1
  local attempt

  for attempt in 1 2; do
    if timeout --kill-after=5s 45s vhs "${tape}"; then
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
  ffmpeg \
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
