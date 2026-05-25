# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Merge labeled chunks into a single JSONL and report stats.

Usage:
    uv run scripts/merge-chunks.py -i data/labeled/ -o data/labeled.jsonl
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-i", "--input-dir", type=Path, required=True)
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()

    chunks = sorted(args.input_dir.glob("chunk-*.jsonl"))
    if not chunks:
        print(f"No chunk files found in {args.input_dir}", file=sys.stderr)
        sys.exit(1)

    labels = Counter()
    confidences: list[int] = []
    pii_count = 0
    total = 0
    errors = 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w") as fout:
        for chunk in chunks:
            for line_num, line in enumerate(chunk.open(), 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    rec = json.loads(line)
                except json.JSONDecodeError:
                    errors += 1
                    print(f"  bad JSON in {chunk.name}:{line_num}", file=sys.stderr)
                    continue

                label = rec.get("label", "unknown")
                labels[label] += 1
                confidences.append(rec.get("confidence", 0))
                if rec.get("pii"):
                    pii_count += len(rec["pii"])
                total += 1

                fout.write(json.dumps(rec) + "\n")

    avg_conf = sum(confidences) / len(confidences) if confidences else 0

    print(f"\nMerged {total} commands from {len(chunks)} chunks → {args.output}", file=sys.stderr)
    if errors:
        print(f"  {errors} lines skipped (bad JSON)", file=sys.stderr)
    print(f"\nLabel distribution:", file=sys.stderr)
    for label, count in labels.most_common():
        pct = count / total * 100 if total else 0
        print(f"  {label}: {count} ({pct:.1f}%)", file=sys.stderr)
    print(f"\nAvg confidence: {avg_conf:.2f}", file=sys.stderr)
    print(f"Commands with PII flagged: {pii_count}", file=sys.stderr)


if __name__ == "__main__":
    main()
