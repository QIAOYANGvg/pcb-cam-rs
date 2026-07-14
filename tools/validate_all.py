#!/usr/bin/env python3
"""Run the complete gerber-parse regression gate."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


def run(command: list[str], cwd: Path) -> int:
    print(f"\n==> {' '.join(command)}", flush=True)
    return subprocess.run(command, cwd=cwd, check=False).returncode


def main() -> int:
    crate_dir = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--kicad-exporter",
        type=Path,
        help="Optional path to export_gerber_golden.exe",
    )
    parser.add_argument("--max-mismatches", type=int, default=20)
    args = parser.parse_args()

    commands = [
        ["cargo", "test"],
        [
            sys.executable,
            "-m",
            "unittest",
            "discover",
            "-s",
            "tools/tests",
            "-v",
        ],
        [
            sys.executable,
            "tools/validate_tracespace.py",
            "--max-mismatches",
            str(args.max_mismatches),
        ],
    ]
    if args.kicad_exporter:
        commands[-1].extend(
            ["--kicad-exporter", str(args.kicad_exporter.resolve())]
        )

    for command in commands:
        return_code = run(command, crate_dir)
        if return_code != 0:
            return return_code

    print("\nALL VALIDATION GATES PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
