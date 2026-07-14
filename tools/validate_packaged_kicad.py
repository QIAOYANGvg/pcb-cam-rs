#!/usr/bin/env python3
"""Run the full gerber-parse validation gate with the packaged KiCad exporter."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from validate_tracespace import packaged_exporter


def main() -> int:
    crate_dir = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--max-mismatches", type=int, default=20)
    args = parser.parse_args()

    exporter = packaged_exporter(crate_dir)
    validation = crate_dir / "tools" / "validate_all.py"

    if not exporter.exists():
        raise FileNotFoundError(f"Packaged KiCad exporter not found: {exporter}")
    if not validation.exists():
        raise FileNotFoundError(
            f"gerber-parse validation entry not found: {validation}"
        )

    return subprocess.run(
        [
            sys.executable,
            str(validation),
            "--kicad-exporter",
            str(exporter),
            "--max-mismatches",
            str(args.max_mismatches),
        ],
        cwd=crate_dir,
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
