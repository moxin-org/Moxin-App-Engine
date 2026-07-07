#!/usr/bin/env python3
"""README standardization helper for MoFA repositories.

Usage examples:
  python .github/scripts/standardize_readme.py --check README.md README_cn.md
  python .github/scripts/standardize_readme.py --write README_cn.md
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

CANONICAL_DISCORD = "https://discord.com/invite/hKJZzDMMm9"
CANONICAL_DISCUSSIONS = "https://github.com/mofa-org/mofa/discussions"
REQUIRED_SECTIONS = [
    "overview",
    "quick start",
    "contributing",
    "community",
]

SECTION_ALIASES = {
    "overview": "overview",
    "quick start": "quick start",
    "contributing": "contributing",
    "community": "community",
    "概述": "overview",
    "快速开始": "quick start",
    "贡献": "contributing",
    "社区": "community",
}


def parse_sections(text: str) -> set[str]:
    sections: set[str] = set()
    for line in text.splitlines():
        if line.startswith("## "):
            raw = line[3:].strip().lower()
            sections.add(SECTION_ALIASES.get(raw, raw))
    return sections


def normalize_community_labels(text: str) -> str:
    # Normalize labels and URLs for community links.
    text = text.replace(
        "GitHub Issues: [https://github.com/mofa-org/mofa/discussions](https://github.com/mofa-org/mofa/discussions)",
        "GitHub Discussions: [https://github.com/mofa-org/mofa/discussions](https://github.com/mofa-org/mofa/discussions)",
    )

    text = re.sub(
        r"https://discord(?:\.gg|\.com/invite)/hKJZzDMMm9",
        CANONICAL_DISCORD,
        text,
    )
    text = re.sub(
        r"https://github\.com/mofa-org/mofa/discussions",
        CANONICAL_DISCUSSIONS,
        text,
    )
    return text


def check_required_sections(path: Path, text: str) -> list[str]:
    present = parse_sections(text)
    missing = [section for section in REQUIRED_SECTIONS if section not in present]
    if missing:
        print(f"[WARN] {path}: missing sections: {', '.join(missing)}")
    else:
        print(f"[OK]   {path}: required sections present")
    return missing


def process_file(path: Path, write: bool) -> int:
    original = path.read_text(encoding="utf-8")
    updated = normalize_community_labels(original)

    missing = check_required_sections(path, updated)

    if write and updated != original:
        path.write_text(updated, encoding="utf-8")
        print(f"[FIX]  {path}: normalized community labels/links")

    return 1 if missing else 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Standardize README structure and community links")
    parser.add_argument("files", nargs="+", help="README files to process")
    parser.add_argument("--write", action="store_true", help="Write normalized content in-place")
    parser.add_argument("--check", action="store_true", help="Only check and report")
    args = parser.parse_args()

    missing_total = 0
    for file_name in args.files:
        path = Path(file_name)
        if not path.exists():
            print(f"[ERROR] {path}: file not found", file=sys.stderr)
            return 2
        missing_total += process_file(path, write=args.write and not args.check)

    if missing_total > 0:
        print("\nSome README files are missing required sections.")
        return 1

    print("\nREADME standardization check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
