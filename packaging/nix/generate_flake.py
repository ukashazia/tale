#!/usr/bin/env python3

import argparse
import re
import shutil
from pathlib import Path
from string import Template


VERSION_PATTERN = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?")
HASH_PATTERN = re.compile(r"sha256-[0-9A-Za-z+/]{43}=")


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--aarch64-darwin-hash", required=True)
    parser.add_argument("--aarch64-linux-hash", required=True)
    parser.add_argument("--x86-64-darwin-hash", required=True)
    parser.add_argument("--x86-64-linux-hash", required=True)
    return parser.parse_args()


def main() -> None:
    arguments = parse_arguments()
    if VERSION_PATTERN.fullmatch(arguments.version) is None:
        raise ValueError("version must be a safe semantic version")

    values = {
        "version": arguments.version,
        "aarch64_darwin_hash": arguments.aarch64_darwin_hash,
        "aarch64_linux_hash": arguments.aarch64_linux_hash,
        "x86_64_darwin_hash": arguments.x86_64_darwin_hash,
        "x86_64_linux_hash": arguments.x86_64_linux_hash,
    }
    for name, value in values.items():
        if name != "version" and HASH_PATTERN.fullmatch(value) is None:
            raise ValueError(f"{name} must be a sha256 SRI hash")

    source_directory = Path(__file__).parent
    template = Template(
        (source_directory / "flake.nix.in").read_text(encoding="utf-8")
    )
    rendered = template.substitute(values)

    arguments.output_dir.mkdir(parents=True, exist_ok=True)
    (arguments.output_dir / "flake.nix").write_text(rendered, encoding="utf-8")
    shutil.copyfile(source_directory / "flake.lock", arguments.output_dir / "flake.lock")


if __name__ == "__main__":
    main()
