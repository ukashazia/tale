#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "${script_dir}/.." && pwd)
source_image=${1:-"${script_dir}/screenshots/tale-devices-source.png"}
output="${repo_root}/docs/assets/tale-devices.png"
margin=32
title_bar=44
radius=14

if [[ $# -gt 1 ]]; then
  printf 'usage: %s [source-image]\n' "$0" >&2
  exit 2
fi

if ! command -v magick >/dev/null 2>&1; then
  printf 'framing requires magick; run it from `nix develop`\n' >&2
  exit 1
fi

if [[ ! -r ${source_image} ]]; then
  printf 'source image is not readable: %s\n' "${source_image}" >&2
  exit 1
fi

dimensions=$(magick identify -format '%w %h' "${source_image}")
read -r source_width source_height <<<"${dimensions}"

if [[ ! ${source_width} =~ ^[0-9]+$ || ! ${source_height} =~ ^[0-9]+$ ]]; then
  printf 'could not determine source dimensions: %s\n' "${source_image}" >&2
  exit 1
fi

window_width=${source_width}
window_height=$((source_height + title_bar))
canvas_width=$((window_width + margin * 2))
canvas_height=$((window_height + margin * 2))
work_dir=$(mktemp -d)

cleanup() {
  rm -rf -- "${work_dir}"
}
trap cleanup EXIT INT TERM

magick \
  -size "${window_width}x${window_height}" \
  xc:'#20233b' \
  "${source_image}" \
  -geometry "+0+${title_bar}" \
  -compose over \
  -composite \
  -fill '#ff5f57' -draw 'circle 22,22 28,22' \
  -fill '#febc2e' -draw 'circle 42,22 48,22' \
  -fill '#28c840' -draw 'circle 62,22 68,22' \
  "${work_dir}/window.png"

magick \
  -size "${window_width}x${window_height}" \
  xc:black \
  -fill white \
  -draw "roundrectangle 0,0 $((window_width - 1)),$((window_height - 1)) ${radius},${radius}" \
  "${work_dir}/mask.png"

magick \
  "${work_dir}/window.png" \
  "${work_dir}/mask.png" \
  -alpha off \
  -compose copy_opacity \
  -composite \
  "${work_dir}/framed-window.png"

magick \
  -size "${canvas_width}x${canvas_height}" \
  xc:'#e8e2df' \
  "${work_dir}/framed-window.png" \
  -geometry "+${margin}+${margin}" \
  -compose over \
  -composite \
  -strip \
  "${output}"

printf '%s\n' 'Updated the framed README screenshot in docs/assets.'
