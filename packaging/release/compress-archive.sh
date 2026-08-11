#!/bin/sh
set -eu

usage() {
  echo 'usage: compress-archive.sh --input PATH --output PATH' >&2
  exit 64
}

input=
output=
while [ "$#" -gt 0 ]; do
  [ "$#" -ge 2 ] || usage
  case "$1" in
    --input) input=$2 ;;
    --output) output=$2 ;;
    *) usage ;;
  esac
  shift 2
done

[ -n "$input" ] && [ -n "$output" ] || usage
[ -f "$input" ] || { echo "input is not a regular file: $input" >&2; exit 66; }
[ ! -e "$output" ] || { echo "output already exists: $output" >&2; exit 73; }
zstd --no-progress --force -o "$output" "$input"
