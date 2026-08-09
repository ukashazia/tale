#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/.." && pwd)
assets_dir="${repo_root}/docs/assets"
tapes_dir="${script_dir}/screenshots"
wallpaper=${1:-}
intermediates=(
  "${assets_dir}/.tale-devices.mp4"
  "${assets_dir}/.tale-services.mp4"
  "${assets_dir}/.tale-navigation.mp4"
)

if [[ $# -gt 1 ]]; then
  printf 'usage: %s [wallpaper]\n' "$0" >&2
  exit 2
fi

if [[ -n ${wallpaper} && ! -r ${wallpaper} ]]; then
  printf 'wallpaper is not readable: %s\n' "${wallpaper}" >&2
  exit 1
fi

if [[ -n ${wallpaper} ]]; then
  wallpaper=$(cd "$(dirname "${wallpaper}")" && pwd)/$(basename "${wallpaper}")
fi

cleanup() {
  rm -f -- "${intermediates[@]}"
}
trap cleanup EXIT INT TERM

for command in ffmpeg ffprobe timeout vhs; do
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

extract_frame() {
  local name=$1
  local video="${assets_dir}/.${name}.mp4"
  local output="${assets_dir}/${name}.png"

  if [[ -z ${wallpaper} ]]; then
    ffmpeg \
      -y \
      -hide_banner \
      -loglevel error \
      -sseof -0.05 \
      -i "${video}" \
      -frames:v 1 \
      -map_metadata -1 \
      "${output}"
    return
  fi

  local dimensions
  local width
  local height
  dimensions=$(ffprobe \
    -v error \
    -select_streams v:0 \
    -show_entries stream=width,height \
    -of csv=s=x:p=0 \
    "${video}")

  if [[ ! ${dimensions} =~ ^[0-9]+x[0-9]+$ ]]; then
    printf 'could not determine video dimensions: %s\n' "${video}" >&2
    return 1
  fi

  width=${dimensions%x*}
  height=${dimensions#*x}

  ffmpeg \
    -y \
    -hide_banner \
    -loglevel error \
    -i "${wallpaper}" \
    -sseof -0.05 \
    -i "${video}" \
    -filter_complex \
    "[0:v]setpts=PTS-STARTPTS,scale=${width}:${height}:force_original_aspect_ratio=increase,crop=${width}:${height},format=rgba,split=2[wall][blur_source]; \
     [blur_source]gblur=sigma=18[blurred]; \
     [1:v]setpts=PTS-STARTPTS,format=rgba,colorkey=0xE8E1DF:0.035:0.08,split=2[mask_source][terminal_source]; \
     [mask_source]alphaextract[mask]; \
     [blurred][mask]alphamerge[blurred_window]; \
     [wall][blurred_window]overlay=format=auto[wall_with_blur]; \
     [terminal_source]colorchannelmixer=aa=0.82[terminal]; \
     [wall_with_blur][terminal]overlay=format=auto,format=rgb24[out]" \
    -map '[out]' \
    -frames:v 1 \
    -map_metadata -1 \
    "${output}"
}

for name in tale-devices tale-services tale-navigation; do
  capture_tape "${tapes_dir}/${name}.tape"
  extract_frame "${name}"
done

printf '%s\n' 'Updated README screenshots in docs/assets.'
