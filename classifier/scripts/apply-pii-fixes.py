# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Apply PII corrections from the labeling pass to produce a clean dataset.

Reads labeled.jsonl, applies the "original → replacement" substitutions
from each record's pii field, and writes the cleaned output.

Usage:
    uv run scripts/apply-pii-fixes.py -i data/labeled.jsonl -o data/labeled-clean.jsonl
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


def parse_pii_entry(entry: str) -> tuple[str, str] | None:
    parts = entry.split(" → ", 1)
    if len(parts) != 2:
        return None
    original, replacement = parts[0].strip(), parts[1].strip()
    if not original or not replacement:
        return None
    return original, replacement


def apply_fixes(command: str, pii: list[str]) -> str:
    for entry in pii:
        pair = parse_pii_entry(entry)
        if not pair:
            continue
        original, replacement = pair
        command = command.replace(original, replacement)
    return command


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-i", "--input", type=Path, required=True)
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()

    fixed = 0
    total = 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.input.open() as fin, args.output.open("w") as fout:
        for line in fin:
            rec = json.loads(line)
            total += 1

            pii = rec.get("pii", [])
            if pii:
                rec["command"] = apply_fixes(rec["command"], pii)
                fixed += 1

            rec.pop("pii", None)
            fout.write(json.dumps(rec) + "\n")

    print(f"Applied PII fixes to {fixed}/{total} commands → {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
