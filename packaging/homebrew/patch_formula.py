#!/usr/bin/env python3
"""Add Tale's packaged shell completions to a cargo-dist Homebrew formula."""

from __future__ import annotations

import argparse
from pathlib import Path


INSTALL_ANCHOR = "    install_binary_aliases!\n"
COMPLETION_INSTALL = """

    bash_completion.install "completions/tale.bash" => "tale"
    zsh_completion.install "completions/_tale"
    fish_completion.install "completions/tale.fish"
"""
LEFTOVER_SOURCE = '    leftover_contents = Dir["*"] - doc_files\n'
LEFTOVER_REPLACEMENT = (
    '    package_manager_files = ["completions"]\n'
    '    leftover_contents = Dir["*"] - doc_files - package_manager_files\n'
)


def patch_formula(contents: str) -> str:
    """Return a patched formula, rejecting unexpected cargo-dist output."""
    if COMPLETION_INSTALL.strip() in contents:
        raise ValueError("formula already installs Tale shell completions")
    if contents.count(INSTALL_ANCHOR) != 1:
        raise ValueError("expected one cargo-dist binary-alias install anchor")
    if contents.count(LEFTOVER_SOURCE) != 1:
        raise ValueError("expected one cargo-dist leftover-content assignment")

    contents = contents.replace(
        INSTALL_ANCHOR,
        INSTALL_ANCHOR + COMPLETION_INSTALL,
        1,
    )
    return contents.replace(LEFTOVER_SOURCE, LEFTOVER_REPLACEMENT, 1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("formula", type=Path)
    arguments = parser.parse_args()
    contents = arguments.formula.read_text(encoding="utf-8")
    arguments.formula.write_text(patch_formula(contents), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
