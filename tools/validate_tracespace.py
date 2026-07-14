#!/usr/bin/env python3
"""Generate KiCad golden JSON temporarily and validate the Rust parser."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import cross_validate
from fetch_tracespace_corpus import prepare_corpus, sha256, verify_corpus


def field_contract() -> dict[str, list[str]]:
    return {
        "root": sorted(cross_validate.GOLDEN_ROOT_FIELDS),
        "metadata": sorted(cross_validate.GOLDEN_METADATA_FIELDS),
        "dcodeRequired": sorted(
            cross_validate.GOLDEN_DCODE_REQUIRED_FIELDS
        ),
        "dcodeOptional": sorted(
            cross_validate.GOLDEN_DCODE_OPTIONAL_FIELDS
        ),
        "itemRequired": sorted(
            cross_validate.GOLDEN_ITEM_REQUIRED_FIELDS
        ),
        "itemOptional": sorted(
            cross_validate.GOLDEN_ITEM_OPTIONAL_FIELDS
        ),
        "netAttributes": sorted(
            cross_validate.GOLDEN_NET_ATTRIBUTE_FIELDS
        ),
        "aperture": sorted(cross_validate.GOLDEN_APERTURE_FIELDS),
        "boundingBox": sorted(
            cross_validate.GOLDEN_BOUNDING_BOX_FIELDS
        ),
        "vector": sorted(cross_validate.GOLDEN_VECTOR_FIELDS),
        "polygon": sorted(cross_validate.GOLDEN_POLYGON_FIELDS),
    }


def mismatch_class(mismatch: str) -> str:
    if mismatch.startswith("parse error:") or mismatch.startswith("missing "):
        return "parser-output"
    if any(
        marker in mismatch
        for marker in ("shapeAsPolygon", "macroShapePolygon", ".polygon")
    ):
        return "polygon-geometry"
    if mismatch.startswith("metadata."):
        return "metadata"
    if mismatch.startswith("dcodes[") or mismatch.startswith("dcodes."):
        return "aperture"
    if mismatch.startswith("items[") or mismatch.startswith("items."):
        return "drawing-item"
    return "other"


def packaged_exporter(crate_dir: Path) -> Path:
    return crate_dir / "tools" / "kicad-exporter" / "export_gerber_golden.exe"


def exporter_candidates(crate_dir: Path) -> list[Path]:
    candidates = []
    candidates.append(packaged_exporter(crate_dir))
    env_path = os.environ.get("GERBER_GOLDEN_EXPORTER")
    if env_path:
        candidates.append(Path(env_path))
    return candidates


def default_kicad_exporter(crate_dir: Path) -> Path | None:
    candidates = exporter_candidates(crate_dir)
    return next((path for path in candidates if path.exists()), None)


def run_kicad_exporter(
    exporter: Path,
    input_dir: Path,
    golden_dir: Path,
) -> subprocess.CompletedProcess[str]:
    if golden_dir.exists():
        shutil.rmtree(golden_dir)
    golden_dir.mkdir(parents=True)

    runtime_dirs = [exporter.parent]
    if "build" in {part.lower() for part in exporter.parts}:
        try:
            build_dir = exporter.parents[3]
            runtime_dirs.extend(
                [
                    build_dir / "common" / "Release",
                    build_dir / "common",
                    build_dir / "common" / "gal" / "Release",
                    build_dir / "common" / "gal",
                    build_dir / "api" / "Release",
                    build_dir / "api",
                ]
            )
        except IndexError:
            pass

    env = os.environ.copy()
    env["PATH"] = os.pathsep.join(
        [str(path) for path in runtime_dirs if path.exists()]
        + [env.get("PATH", "")]
    )

    return subprocess.run(
        [str(exporter), "--all", str(input_dir), str(golden_dir)],
        cwd=exporter.parent,
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def main() -> int:
    crate_dir = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--corpus-dir",
        type=Path,
        default=crate_dir / "test-corpus" / "tracespace-v5",
    )
    parser.add_argument(
        "--results-dir",
        type=Path,
        default=crate_dir / "validation-results" / "tracespace-v5",
    )
    parser.add_argument(
        "--kicad-exporter",
        type=Path,
        default=default_kicad_exporter(crate_dir),
        help=(
            "KiCad golden exporter; defaults to the packaged repository tool "
            "or GERBER_GOLDEN_EXPORTER"
        ),
    )
    parser.add_argument(
        "--refresh-corpus",
        action="store_true",
        help="Maintenance-only download of the pinned tracespace sources",
    )
    parser.add_argument("--max-mismatches", type=int, default=20)
    args = parser.parse_args()

    corpus_dir = args.corpus_dir.resolve()
    manifest_path = corpus_dir / "manifest.json"
    input_dir = corpus_dir / "input"

    if args.refresh_corpus or not manifest_path.exists():
        manifest = prepare_corpus(corpus_dir)
    else:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))

    input_files = verify_corpus(corpus_dir, manifest)
    manifest_entries = {
        str(entry["staged"]): entry
        for entry in manifest["files"]
    }

    if args.kicad_exporter is None:
        raise FileNotFoundError(
            "KiCad golden exporter not configured. Pass --kicad-exporter "
            "or set GERBER_GOLDEN_EXPORTER."
        )

    exporter = args.kicad_exporter.resolve()
    if not exporter.exists():
        raise FileNotFoundError(
            f"KiCad golden exporter not found: {exporter}. "
            "Pass --kicad-exporter or set GERBER_GOLDEN_EXPORTER."
        )

    results_dir = args.results_dir.resolve()
    golden_dir = results_dir / "kicad-golden"
    results_dir.mkdir(parents=True, exist_ok=True)

    kicad = run_kicad_exporter(exporter, input_dir, golden_dir)
    (results_dir / "kicad.stdout.txt").write_text(
        kicad.stdout,
        encoding="utf-8",
    )
    (results_dir / "kicad.stderr.txt").write_text(
        kicad.stderr,
        encoding="utf-8",
    )
    if kicad.returncode != 0:
        raise RuntimeError(
            f"KiCad exporter failed with exit code {kicad.returncode}"
        )

    golden = cross_validate.load_golden(golden_dir)
    schema_errors = [
        error
        for stem, data in golden.items()
        for error in cross_validate.audit_golden_schema(stem, data)
    ]
    if schema_errors:
        details = "\n".join(schema_errors[:20])
        raise RuntimeError(
            f"KiCad golden schema drifted ({len(schema_errors)} error(s)):\n"
            f"{details}"
        )

    rust = cross_validate.run_rust_exporter(crate_dir, input_dir)
    report_files = []
    matched = 0
    mismatched = 0
    missing_golden = 0
    failure_classes: dict[str, int] = {}

    for input_path in input_files:
        stem = input_path.stem
        manifest_entry = manifest_entries[input_path.name]
        if stem not in golden:
            missing_golden += 1
            failure_classes["missing-kicad-golden"] = (
                failure_classes.get("missing-kicad-golden", 0) + 1
            )
            report_files.append(
                {
                    "file": input_path.name,
                    "source": manifest_entry["source"],
                    "sha256": manifest_entry["sha256"],
                    "status": "missing-kicad-golden",
                    "classes": ["missing-kicad-golden"],
                    "mismatches": [],
                }
            )
            print(f"MISSING_KICAD {input_path.name}")
            continue

        rust_file = rust.get(
            stem,
            cross_validate.ParserFile(errors=["missing Rust output"]),
        )
        mismatches = cross_validate.compare_file(
            stem,
            rust_file,
            golden[stem],
            args.max_mismatches,
            strict=True,
        )
        if mismatches:
            mismatched += 1
            status = "mismatched"
            classes = sorted(
                {mismatch_class(mismatch) for mismatch in mismatches}
            )
            for class_name in classes:
                failure_classes[class_name] = (
                    failure_classes.get(class_name, 0) + 1
                )
            print(f"FAIL {input_path.name}: {len(mismatches)} mismatch(es)")
            for mismatch in mismatches[: args.max_mismatches + 1]:
                print(f"  {mismatch}")
        else:
            matched += 1
            status = "matched"
            classes = []
            print(f"OK {input_path.name}")

        report_files.append(
            {
                "file": input_path.name,
                "source": manifest_entry["source"],
                "sha256": manifest_entry["sha256"],
                "status": status,
                "classes": classes,
                "mismatches": mismatches,
            }
        )

    report = {
        "schemaVersion": 1,
        "corpus": {
            "repository": manifest["repository"],
            "commit": manifest["commit"],
            "count": manifest["count"],
            "digest": manifest["digest"],
        },
        "goldenGenerator": {
            "path": str(exporter),
            "sha256": sha256(exporter),
            "exitCode": kicad.returncode,
            "count": len(golden),
        },
        "comparisonPolicy": {
            "goldenSchemaVersion": 1,
            "schemaAudit": "reject-missing-and-unknown-fields",
            "fieldTolerance": 0,
            "coordinateToleranceIU": 0,
            "boundingBoxToleranceIU": 0,
            "polygonVertexToleranceIU": 0,
            "polygonNormalization": [
                "contour-start",
                "winding",
                "polygon-order",
                "hole-order",
            ],
        },
        "fieldContract": field_contract(),
        "summary": {
            "total": len(input_files),
            "matched": matched,
            "mismatched": mismatched,
            "missingKiCadGolden": missing_golden,
            "failureClasses": dict(sorted(failure_classes.items())),
        },
        "files": report_files,
    }
    report_path = results_dir / "report.json"
    report_path.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    print(
        f"SUMMARY total={len(input_files)} matched={matched} "
        f"mismatched={mismatched} missing_kicad={missing_golden} "
        f"kicad_exit={kicad.returncode}"
    )
    print(f"REPORT {report_path}")

    return 0 if mismatched == 0 and missing_golden == 0 else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f"ERROR {error}", file=sys.stderr)
        raise
