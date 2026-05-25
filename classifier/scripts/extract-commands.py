# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Extract bash commands from Claude Code session logs.

Walks the session log directory, pulls every Bash tool_use, and emits
a deduplicated JSONL file ready for labeling.

Usage:
    uv run scripts/extract-commands.py /path/to/logs -o data/commands.jsonl
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Iterator
from dataclasses import dataclass, asdict
from pathlib import Path


@dataclass(frozen=True)
class CommandRecord:
    command: str
    session_id: str
    timestamp: str
    cwd: str


def extract_from_session(path: Path) -> Iterator[CommandRecord]:
    try:
        entries = json.loads(path.read_text())
    except (json.JSONDecodeError, OSError):
        return

    if not isinstance(entries, list):
        return

    session_id = path.stem
    session_cwd = ""
    session_ts = ""

    for entry in entries:
        if not isinstance(entry, dict):
            continue

        if entry.get("type") == "user" and not session_cwd:
            session_cwd = entry.get("cwd", "")
            session_ts = entry.get("timestamp", "")

        if entry.get("type") != "assistant":
            continue

        message = entry.get("message", {})
        content = message.get("content", [])
        if not isinstance(content, list):
            continue

        for block in content:
            if not isinstance(block, dict):
                continue
            if block.get("type") != "tool_use" or block.get("name") != "Bash":
                continue

            cmd = (block.get("input") or {}).get("command", "").strip()
            if not cmd:
                continue

            yield CommandRecord(
                command=cmd,
                session_id=session_id,
                timestamp=entry.get("timestamp", session_ts),
                cwd=entry.get("cwd", session_cwd),
            )


def collect_commands(logs_dir: Path) -> list[CommandRecord]:
    records: list[CommandRecord] = []
    seen: set[str] = set()

    log_files = sorted(logs_dir.rglob("*.json"))
    print(f"Scanning {len(log_files)} session files...", file=sys.stderr)

    for path in log_files:
        for rec in extract_from_session(path):
            if rec.command not in seen:
                seen.add(rec.command)
                records.append(rec)

    return records


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs_dir", type=Path, help="Root directory of session logs")
    parser.add_argument(
        "-o", "--output", type=Path, default=Path("data/commands.jsonl"),
        help="Output JSONL path (default: data/commands.jsonl)",
    )
    parser.add_argument(
        "--keep-dupes", action="store_true",
        help="Keep duplicate commands (default: deduplicate by exact match)",
    )
    args = parser.parse_args()

    if not args.logs_dir.is_dir():
        print(f"Error: {args.logs_dir} is not a directory", file=sys.stderr)
        sys.exit(1)

    records = collect_commands(args.logs_dir)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w") as f:
        for rec in records:
            f.write(json.dumps(asdict(rec)) + "\n")

    print(f"Extracted {len(records)} unique commands → {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
