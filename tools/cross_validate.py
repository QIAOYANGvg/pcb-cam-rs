#!/usr/bin/env python3
"""Cross-validate the Rust and Web Gerber parsers against KiCad golden JSON.

This script keeps the comparison layer in Python. It builds a temporary Rust
runner, invokes the Web TypeScript golden exporter, and compares all three
implementations field-by-field.

Example:
    python gerber-parse/tools/cross_validate.py \
        --input-dir C:/tmp/gerbers \
        --golden-dir C:/tmp/golden
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

GERBER_EXTS = {
    ".gbr", ".gtl", ".gbl", ".gts", ".gbs", ".gto", ".gbo", ".gko", ".gtp", ".gbp",
    ".gdl", ".gdd", ".gml", ".gcl", ".g1", ".g2", ".g3", ".g4", ".g5", ".g6", ".g7",
    ".g8", ".ger", ".pho", ".art",
}

GOLDEN_ROOT_FIELDS = {"metadata", "dcodes", "items"}
GOLDEN_METADATA_FIELDS = {
    "fileName",
    "isMetric",
    "isX2",
    "imageNegative",
    "fileFunction",
    "itemCount",
    "dcodeCount",
    "imageOffset",
    "imageRotation",
    "localRotation",
    "offset",
    "scale",
    "swapAxis",
    "mirrorA",
    "mirrorB",
    "imageJustifyOffset",
    "imageJustifyXCenter",
    "imageJustifyYCenter",
    "fmtScale",
    "fmtLen",
    "noTrailingZeros",
    "relative",
}
GOLDEN_DCODE_REQUIRED_FIELDS = {
    "num",
    "type",
    "size",
    "drill",
    "drillShape",
    "rotation",
    "edgesCount",
    "inUse",
    "defined",
    "aperFunction",
}
GOLDEN_DCODE_OPTIONAL_FIELDS = {
    "macroName",
    "macroParams",
    "polygon",
}
GOLDEN_ITEM_REQUIRED_FIELDS = {
    "shapeType",
    "start",
    "end",
    "size",
    "dcode",
    "flashed",
    "unitsMetric",
    "layerNegative",
    "aperFunction",
    "netAttributes",
    "boundingBox",
}
GOLDEN_ITEM_OPTIONAL_FIELDS = {
    "arcCentre",
    "aperture",
    "shapeAsPolygon",
    "macroShapePolygon",
}
GOLDEN_NET_ATTRIBUTE_FIELDS = {
    "netAttribType",
    "netname",
    "cmpref",
    "padname",
    "pinFunction",
}
GOLDEN_APERTURE_FIELDS = {"type", "size"}
GOLDEN_BOUNDING_BOX_FIELDS = {"origin", "size"}
GOLDEN_VECTOR_FIELDS = {"x", "y"}
GOLDEN_POLYGON_FIELDS = {"outline", "holes"}

RUST_RUNNER = r'''
use std::fs;
use std::path::{Path, PathBuf};

use gerber_parse::geometry::{PolySet, Vec2I};
use gerber_parse::readgerb::load_gerber_file;
use gerber_parse::types::{ApertureHoleType, ApertureType, ShapeType};

const GERBER_EXTS: &[&str] = &[
    "gbr", "gtl", "gbl", "gts", "gbs", "gto", "gbo", "gko", "gtp", "gbp", "gdl", "gdd",
    "gml", "gcl", "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "ger", "pho", "art",
];

fn main() {
    let input_dir = PathBuf::from(std::env::args().nth(1).expect("input dir arg"));
    let mut files = fs::read_dir(&input_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| is_gerber(path))
        .collect::<Vec<_>>();
    files.sort();

    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        match load_gerber_file(path.to_str().unwrap()) {
            Ok(image) => {
                println!(
                    "META\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    esc(&stem),
                    image.gerb_metric,
                    image.is_x2_file,
                    image.image_negative,
                    image.drawings.len(),
                    image.aperture_list.len(),
                    image.image_offset.x,
                    image.image_offset.y,
                    image.image_rotation,
                    image.local_rotation,
                    image.offset.x,
                    image.offset.y,
                    image.scale.0,
                    image.scale.1,
                    image.swap_axis,
                    image.mirror_a,
                    image.mirror_b,
                    image.image_justify_offset.x,
                    image.image_justify_offset.y,
                    image.image_justify_x_center,
                    image.image_justify_y_center,
                    image.fmt_scale.x,
                    image.fmt_scale.y,
                    image.fmt_len.x,
                    image.fmt_len.y,
                    image.no_trailing_zeros,
                    image.relative,
                    image.messages.len(),
                    esc(
                        &image
                            .file_function
                            .as_ref()
                            .map(|function| function.get_file_type().to_string())
                            .unwrap_or_default(),
                    ),
                );

                for dcode in image.aperture_list.values() {
                    println!(
                        "DCODE\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        esc(&stem),
                        dcode.num,
                        aperture_type(dcode.apert_type),
                        dcode.size.x,
                        dcode.size.y,
                        dcode.drill.x,
                        dcode.drill.y,
                        drill_shape(dcode.drill_shape),
                        dcode.rotation,
                        dcode.edges_count,
                        dcode.in_use,
                        dcode.defined,
                        esc(&dcode.aper_function),
                        dcode.am_params.len(),
                        dcode.polygon.len(),
                        esc(&dcode.macro_name),
                        esc(&json_polyset(&dcode.polyset)),
                        esc(&json_floats(&dcode.am_params)),
                    );
                }

                for (idx, item) in image.drawings.iter().enumerate() {
                    let dcode = image.aperture_list.get(&item.dcode);
                    let bbox = item.get_bounding_box(dcode);
                    println!(
                        "ITEM\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        esc(&stem),
                        idx,
                        shape_type(item.shape_type),
                        item.start.x,
                        item.start.y,
                        item.end.x,
                        item.end.y,
                        item.size.x,
                        item.size.y,
                        item.dcode,
                        item.flashed,
                        item.units_metric,
                        item.arc_centre.x,
                        item.arc_centre.y,
                        item.layer_negative,
                        esc(&item.aper_function),
                        item.net_attributes.net_attrib_type,
                        esc(&item.net_attributes.netname),
                        esc(&item.net_attributes.cmpref),
                        esc(&item.net_attributes.padname),
                        esc(&item.net_attributes.pad_pin_function),
                        item.shape_as_polygon.len(),
                        item.absolute_polygon.len(),
                        item.swap_axis,
                        item.lyr_rotation,
                        bbox.origin.x,
                        bbox.origin.y,
                        bbox.size.x,
                        bbox.size.y,
                        esc(&json_outlines(&item.shape_as_polygon)),
                        esc(&json_polyset(&item.macro_shape_polygon)),
                    );
                }
            }
            Err(errors) => {
                println!("ERROR\t{}\t{}", esc(&stem), esc(&errors.join(" | ")));
            }
        }
    }
}

fn is_gerber(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| GERBER_EXTS.iter().any(|candidate| candidate.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn aperture_type(value: ApertureType) -> &'static str {
    match value {
        ApertureType::Circle => "C",
        ApertureType::Rect => "R",
        ApertureType::Oval => "O",
        ApertureType::Polygon => "P",
        ApertureType::Macro => "M",
    }
}

fn drill_shape(value: ApertureHoleType) -> &'static str {
    match value {
        ApertureHoleType::NoHole => "NO_HOLE",
        ApertureHoleType::RoundHole => "ROUND_HOLE",
        ApertureHoleType::RectHole => "RECT_HOLE",
    }
}

fn shape_type(value: ShapeType) -> &'static str {
    match value {
        ShapeType::Segment => "SEGMENT",
        ShapeType::Arc => "ARC",
        ShapeType::Circle => "CIRCLE",
        ShapeType::Polygon => "POLYGON",
        ShapeType::SpotCircle => "SPOT_CIRCLE",
        ShapeType::SpotRect => "SPOT_RECT",
        ShapeType::SpotOval => "SPOT_OVAL",
        ShapeType::SpotPoly => "SPOT_POLY",
        ShapeType::SpotMacro => "SPOT_MACRO",
    }
}

fn json_vec(value: Vec2I) -> String {
    format!("{{\"x\":{},\"y\":{}}}", value.x, value.y)
}

fn json_points(points: &[Vec2I]) -> String {
    let body = points.iter().map(|point| json_vec(*point)).collect::<Vec<_>>().join(",");
    format!("[{}]", body)
}

fn json_floats(values: &[f64]) -> String {
    let body = values.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(",");
    format!("[{}]", body)
}

fn json_outlines(outlines: &[Vec<Vec2I>]) -> String {
    let body = outlines
        .iter()
        .map(|outline| format!("{{\"outline\":{},\"holes\":[]}}", json_points(outline)))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", body)
}

fn json_polyset(polyset: &PolySet) -> String {
    let body = polyset
        .polygons
        .iter()
        .map(|poly| {
            let holes = poly
                .holes
                .iter()
                .map(|hole| json_points(hole))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{\"outline\":{},\"holes\":[{}]}}", json_points(&poly.outline), holes)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", body)
}
'''


@dataclass
class ParserFile:
    metadata: dict[str, Any] | None = None
    dcodes: dict[int, dict[str, Any]] = field(default_factory=dict)
    items: list[dict[str, Any]] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)


def parse_bool(value: str) -> bool:
    return value == "true"


def parse_int(value: str) -> int:
    return int(value)


def parse_float(value: str) -> float:
    return float(value)


def unesc(value: str) -> str:
    return value.replace("\\r", "\r").replace("\\n", "\n").replace("\\t", "\t").replace("\\\\", "\\")


def run_rust_exporter(crate_dir: Path, input_dir: Path, keep_runner: bool = False) -> dict[str, ParserFile]:
    work_dir = Path(tempfile.mkdtemp(prefix="gerber-parse-cross-"))
    try:
        (work_dir / "src").mkdir()
        (work_dir / "Cargo.toml").write_text(
            "[package]\n"
            "name = \"gerber_parse_cross_runner\"\n"
            "version = \"0.1.0\"\n"
            "edition = \"2024\"\n\n"
            "[dependencies]\n"
            f"gerber-parse = {{ path = {json.dumps(str(crate_dir))} }}\n",
            encoding="utf-8",
        )
        (work_dir / "src" / "main.rs").write_text(RUST_RUNNER, encoding="utf-8")

        proc = subprocess.run(
            ["cargo", "run", "--quiet", "--manifest-path", str(work_dir / "Cargo.toml"), "--", str(input_dir)],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        if proc.stderr.strip():
            print(proc.stderr, file=sys.stderr)

        if proc.returncode != 0:
            raise RuntimeError(f"Rust exporter failed with exit code {proc.returncode}")

        return parse_rust_records(proc.stdout)
    finally:
        if keep_runner:
            print(f"kept temporary Rust runner: {work_dir}", file=sys.stderr)
        else:
            shutil.rmtree(work_dir, ignore_errors=True)


def parse_rust_records(output: str) -> dict[str, ParserFile]:
    files: dict[str, ParserFile] = {}

    for line in output.splitlines():
        parts = line.split("\t")
        if not parts:
            continue

        kind = parts[0]
        stem = unesc(parts[1])
        record = files.setdefault(stem, ParserFile())

        if kind == "ERROR":
            record.errors.append(unesc(parts[2]))
        elif kind == "META":
            record.metadata = {
                "fileName": "",
                "isMetric": parse_bool(parts[2]),
                "isX2": parse_bool(parts[3]),
                "imageNegative": parse_bool(parts[4]),
                "itemCount": parse_int(parts[5]),
                "dcodeCount": parse_int(parts[6]),
                "imageOffset": {"x": parse_int(parts[7]), "y": parse_int(parts[8])},
                "imageRotation": parse_int(parts[9]),
                "localRotation": parse_float(parts[10]),
                "offset": {"x": parse_int(parts[11]), "y": parse_int(parts[12])},
                "scale": {"x": parse_float(parts[13]), "y": parse_float(parts[14])},
                "swapAxis": parse_bool(parts[15]),
                "mirrorA": parse_bool(parts[16]),
                "mirrorB": parse_bool(parts[17]),
                "imageJustifyOffset": {"x": parse_int(parts[18]), "y": parse_int(parts[19])},
                "imageJustifyXCenter": parse_bool(parts[20]),
                "imageJustifyYCenter": parse_bool(parts[21]),
                "fmtScale": {"x": parse_int(parts[22]), "y": parse_int(parts[23])},
                "fmtLen": {"x": parse_int(parts[24]), "y": parse_int(parts[25])},
                "noTrailingZeros": parse_bool(parts[26]),
                "relative": parse_bool(parts[27]),
                "messageCount": parse_int(parts[28]),
                "fileFunction": unesc(parts[29]) or None,
            }
        elif kind == "DCODE":
            dcode = {
                "num": parse_int(parts[2]),
                "type": parts[3],
                "size": {"x": parse_int(parts[4]), "y": parse_int(parts[5])},
                "drill": {"x": parse_int(parts[6]), "y": parse_int(parts[7])},
                "drillShape": parts[8],
                "rotation": parse_float(parts[9]),
                "edgesCount": parse_int(parts[10]),
                "inUse": parse_bool(parts[11]),
                "defined": parse_bool(parts[12]),
                "aperFunction": unesc(parts[13]),
                "macroParamCount": parse_int(parts[14]),
                "polygonPointCount": parse_int(parts[15]),
                "macroName": unesc(parts[16]),
                "polygon": json.loads(unesc(parts[17])),
                "macroParams": json.loads(unesc(parts[18])),
            }
            record.dcodes[dcode["num"]] = dcode
        elif kind == "ITEM":
            record.items.append({
                "index": parse_int(parts[2]),
                "shapeType": parts[3],
                "start": {"x": parse_int(parts[4]), "y": parse_int(parts[5])},
                "end": {"x": parse_int(parts[6]), "y": parse_int(parts[7])},
                "size": {"x": parse_int(parts[8]), "y": parse_int(parts[9])},
                "dcode": parse_int(parts[10]),
                "flashed": parse_bool(parts[11]),
                "unitsMetric": parse_bool(parts[12]),
                "arcCentre": {"x": parse_int(parts[13]), "y": parse_int(parts[14])},
                "layerNegative": parse_bool(parts[15]),
                "aperFunction": unesc(parts[16]),
                "netAttributes": {
                    "netAttribType": parse_int(parts[17]),
                    "netname": unesc(parts[18]),
                    "cmpref": unesc(parts[19]),
                    "padname": unesc(parts[20]),
                    "pinFunction": unesc(parts[21]),
                },
                "shapeAsPolygonCount": parse_int(parts[22]),
                "absolutePolygonCount": parse_int(parts[23]),
                "swapAxis": parse_bool(parts[24]),
                "lyrRotation": parse_float(parts[25]),
                "boundingBox": {
                    "origin": {"x": parse_int(parts[26]), "y": parse_int(parts[27])},
                    "size": {"x": parse_int(parts[28]), "y": parse_int(parts[29])},
                },
                "shapeAsPolygon": json.loads(unesc(parts[30])),
                "macroShapePolygon": json.loads(unesc(parts[31])),
            })

    return files


def load_golden(golden_dir: Path) -> dict[str, dict[str, Any]]:
    result = {}
    for path in sorted(golden_dir.glob("*.json")):
        result[path.stem] = json.loads(path.read_text(encoding="utf-8"))
    return result


def audit_keys(
    path: str,
    value: Any,
    required: set[str],
    optional: set[str] | None = None,
) -> list[str]:
    if not isinstance(value, dict):
        return [f"{path}: expected object, found {type(value).__name__}"]

    optional = optional or set()
    actual = set(value)
    errors = [
        f"{path}: missing field {field_name}"
        for field_name in sorted(required - actual)
    ]
    errors.extend(
        f"{path}: unknown field {field_name}"
        for field_name in sorted(actual - required - optional)
    )
    return errors


def audit_vector(path: str, value: Any) -> list[str]:
    return audit_keys(path, value, GOLDEN_VECTOR_FIELDS)


def audit_polyset(path: str, value: Any) -> list[str]:
    if not isinstance(value, list):
        return [f"{path}: expected array, found {type(value).__name__}"]

    errors = []
    for polygon_index, polygon in enumerate(value):
        polygon_path = f"{path}[{polygon_index}]"
        errors.extend(
            audit_keys(polygon_path, polygon, GOLDEN_POLYGON_FIELDS)
        )
        if not isinstance(polygon, dict):
            continue

        outline = polygon.get("outline")
        holes = polygon.get("holes")
        if not isinstance(outline, list):
            errors.append(f"{polygon_path}.outline: expected array")
        else:
            for point_index, point in enumerate(outline):
                errors.extend(
                    audit_vector(
                        f"{polygon_path}.outline[{point_index}]",
                        point,
                    )
                )

        if not isinstance(holes, list):
            errors.append(f"{polygon_path}.holes: expected array")
        else:
            for hole_index, hole in enumerate(holes):
                hole_path = f"{polygon_path}.holes[{hole_index}]"
                if not isinstance(hole, list):
                    errors.append(f"{hole_path}: expected array")
                    continue
                for point_index, point in enumerate(hole):
                    errors.extend(
                        audit_vector(
                            f"{hole_path}[{point_index}]",
                            point,
                        )
                    )
    return errors


def audit_golden_schema(stem: str, data: Any) -> list[str]:
    root_path = f"golden[{stem}]"
    errors = audit_keys(root_path, data, GOLDEN_ROOT_FIELDS)
    if not isinstance(data, dict):
        return errors

    metadata = data.get("metadata")
    errors.extend(
        audit_keys(
            f"{root_path}.metadata",
            metadata,
            GOLDEN_METADATA_FIELDS,
        )
    )
    if isinstance(metadata, dict):
        for field_name in (
            "imageOffset",
            "offset",
            "scale",
            "imageJustifyOffset",
            "fmtScale",
            "fmtLen",
        ):
            errors.extend(
                audit_vector(
                    f"{root_path}.metadata.{field_name}",
                    metadata.get(field_name),
                )
            )

    dcodes = data.get("dcodes")
    if not isinstance(dcodes, list):
        errors.append(f"{root_path}.dcodes: expected array")
    else:
        for index, dcode in enumerate(dcodes):
            dcode_path = f"{root_path}.dcodes[{index}]"
            errors.extend(
                audit_keys(
                    dcode_path,
                    dcode,
                    GOLDEN_DCODE_REQUIRED_FIELDS,
                    GOLDEN_DCODE_OPTIONAL_FIELDS,
                )
            )
            if not isinstance(dcode, dict):
                continue
            errors.extend(audit_vector(f"{dcode_path}.size", dcode.get("size")))
            errors.extend(audit_vector(f"{dcode_path}.drill", dcode.get("drill")))
            if "polygon" in dcode:
                errors.extend(
                    audit_polyset(f"{dcode_path}.polygon", dcode["polygon"])
                )

    items = data.get("items")
    if not isinstance(items, list):
        errors.append(f"{root_path}.items: expected array")
    else:
        for index, item in enumerate(items):
            item_path = f"{root_path}.items[{index}]"
            errors.extend(
                audit_keys(
                    item_path,
                    item,
                    GOLDEN_ITEM_REQUIRED_FIELDS,
                    GOLDEN_ITEM_OPTIONAL_FIELDS,
                )
            )
            if not isinstance(item, dict):
                continue
            for field_name in ("start", "end", "size"):
                errors.extend(
                    audit_vector(
                        f"{item_path}.{field_name}",
                        item.get(field_name),
                    )
                )
            if "arcCentre" in item:
                errors.extend(
                    audit_vector(
                        f"{item_path}.arcCentre",
                        item["arcCentre"],
                    )
                )
            errors.extend(
                audit_keys(
                    f"{item_path}.netAttributes",
                    item.get("netAttributes"),
                    GOLDEN_NET_ATTRIBUTE_FIELDS,
                )
            )
            bounding_box = item.get("boundingBox")
            errors.extend(
                audit_keys(
                    f"{item_path}.boundingBox",
                    bounding_box,
                    GOLDEN_BOUNDING_BOX_FIELDS,
                )
            )
            if isinstance(bounding_box, dict):
                errors.extend(
                    audit_vector(
                        f"{item_path}.boundingBox.origin",
                        bounding_box.get("origin"),
                    )
                )
                errors.extend(
                    audit_vector(
                        f"{item_path}.boundingBox.size",
                        bounding_box.get("size"),
                    )
                )
            if "aperture" in item:
                aperture = item["aperture"]
                errors.extend(
                    audit_keys(
                        f"{item_path}.aperture",
                        aperture,
                        GOLDEN_APERTURE_FIELDS,
                    )
                )
                if isinstance(aperture, dict):
                    errors.extend(
                        audit_vector(
                            f"{item_path}.aperture.size",
                            aperture.get("size"),
                        )
                    )
            for field_name in ("shapeAsPolygon", "macroShapePolygon"):
                if field_name in item:
                    errors.extend(
                        audit_polyset(
                            f"{item_path}.{field_name}",
                            item[field_name],
                        )
                    )
    return errors


def parser_files_from_json(files: dict[str, dict[str, Any]]) -> dict[str, ParserFile]:
    result = {}

    for stem, data in files.items():
        result[stem] = ParserFile(
            metadata=data.get("metadata"),
            dcodes={int(dcode["num"]): dcode for dcode in data.get("dcodes", [])},
            items=data.get("items", []),
        )

    return result


def run_web_exporter(
    web_dir: Path,
    input_dir: Path,
    output_dir: Path | None = None,
    keep_output: bool = False,
) -> tuple[dict[str, ParserFile], dict[str, dict[str, Any]]]:
    generated_dir = output_dir or Path(tempfile.mkdtemp(prefix="gerber-web-cross-"))
    generated_dir.mkdir(parents=True, exist_ok=True)

    try:
        pnpm = shutil.which("pnpm") or shutil.which("pnpm.cmd")
        if pnpm is None:
            raise RuntimeError("pnpm was not found in PATH")

        proc = subprocess.run(
            [
                pnpm,
                "exec",
                "tsx",
                "src/cli/export-gerber-golden.ts",
                "--all",
                str(input_dir),
                str(generated_dir),
            ],
            cwd=web_dir,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )

        if proc.stdout.strip():
            print(proc.stdout, file=sys.stderr)
        if proc.stderr.strip():
            print(proc.stderr, file=sys.stderr)
        if proc.returncode != 0:
            print(f"Web exporter completed with exit code {proc.returncode}", file=sys.stderr)

        generated = load_golden(generated_dir)
        return parser_files_from_json(generated), generated
    finally:
        if output_dir is not None or keep_output:
            print(f"Web JSON output: {generated_dir}", file=sys.stderr)
        else:
            shutil.rmtree(generated_dir, ignore_errors=True)


def compare_vec(path: str, actual: dict[str, Any], expected: dict[str, Any], mismatches: list[str], tolerance: int = 0) -> None:
    for axis in ("x", "y"):
        actual_value = actual.get(axis)
        expected_value = expected.get(axis)
        if actual_value is None or expected_value is None:
            if actual_value != expected_value:
                mismatches.append(f"{path}.{axis}: actual={actual_value!r} expected={expected_value!r}")
        elif abs(actual_value - expected_value) > tolerance:
            mismatches.append(f"{path}.{axis}: actual={actual_value} expected={expected_value}")


def compare_scalar(path: str, actual: Any, expected: Any, mismatches: list[str], tolerance: float = 0) -> None:
    if isinstance(actual, float) or isinstance(expected, float):
        if abs(float(actual) - float(expected)) > tolerance:
            mismatches.append(f"{path}: actual={actual!r} expected={expected!r}")
    elif actual != expected:
        mismatches.append(f"{path}: actual={actual!r} expected={expected!r}")


def compare_box(path: str, rust: dict[str, Any], golden: dict[str, Any], mismatches: list[str], tolerance: int = 100) -> None:
    compare_vec(f"{path}.origin", rust.get("origin", {}), golden.get("origin", {}), mismatches, tolerance)
    compare_vec(f"{path}.size", rust.get("size", {}), golden.get("size", {}), mismatches, tolerance)


def is_redundant_collinear(
    previous: tuple[int, int],
    current: tuple[int, int],
    following: tuple[int, int],
) -> bool:
    ax = current[0] - previous[0]
    ay = current[1] - previous[1]
    bx = following[0] - current[0]
    by = following[1] - current[1]
    return ax * by - ay * bx == 0 and ax * bx + ay * by >= 0


def normalize_ring(
    points: list[dict[str, Any]],
    remove_redundant_collinear: bool = True,
) -> tuple[tuple[int, int], ...]:
    ring: list[tuple[int, int]] = []
    for point in points:
        value = (int(point["x"]), int(point["y"]))
        if not ring or ring[-1] != value:
            ring.append(value)

    while len(ring) > 1 and ring[0] == ring[-1]:
        ring.pop()

    if remove_redundant_collinear:
        while len(ring) >= 3:
            filtered = [
                point
                for index, point in enumerate(ring)
                if not is_redundant_collinear(
                    ring[(index - 1) % len(ring)],
                    point,
                    ring[(index + 1) % len(ring)],
                )
            ]
            if len(filtered) == len(ring):
                break
            ring = filtered

    if not ring:
        return ()

    def minimum_rotation(values: list[tuple[int, int]]) -> tuple[tuple[int, int], ...]:
        minimum = min(values)
        candidates = [
            tuple(values[index:] + values[:index])
            for index, value in enumerate(values)
            if value == minimum
        ]
        return min(candidates)

    forward = minimum_rotation(ring)
    reverse = minimum_rotation(list(reversed(ring)))
    return min(forward, reverse)


def normalize_polyset(
    polygons: list[Any],
    remove_redundant_collinear: bool = True,
) -> list[tuple[tuple[tuple[int, int], ...], tuple[tuple[tuple[int, int], ...], ...]]]:
    normalized = []
    for polygon in polygons:
        outline = normalize_ring(
            polygon.get("outline", []),
            remove_redundant_collinear,
        )
        holes = tuple(
            sorted(
                normalize_ring(hole, remove_redundant_collinear)
                for hole in polygon.get("holes", [])
            )
        )
        normalized.append((outline, holes))
    normalized.sort()
    return normalized


def compare_polyset(
    path: str,
    rust: list[Any],
    golden: list[Any],
    mismatches: list[str],
    tolerance: int = 1,
    remove_redundant_collinear: bool = True,
) -> None:
    rust_normalized = normalize_polyset(rust, remove_redundant_collinear)
    golden_normalized = normalize_polyset(golden, remove_redundant_collinear)
    compare_scalar(
        f"{path}.len",
        len(rust_normalized),
        len(golden_normalized),
        mismatches,
    )
    for oi, (actual_polygon, expected_polygon) in enumerate(
        zip(rust_normalized, golden_normalized)
    ):
        actual_outline, actual_holes = actual_polygon
        expected_outline, expected_holes = expected_polygon
        compare_scalar(
            f"{path}[{oi}].outline.len",
            len(actual_outline),
            len(expected_outline),
            mismatches,
        )
        for pi, (actual_point, expected_point) in enumerate(
            zip(actual_outline, expected_outline)
        ):
            compare_vec(
                f"{path}[{oi}].outline[{pi}]",
                {"x": actual_point[0], "y": actual_point[1]},
                {"x": expected_point[0], "y": expected_point[1]},
                mismatches,
                tolerance,
            )
        compare_scalar(
            f"{path}[{oi}].holes.len",
            len(actual_holes),
            len(expected_holes),
            mismatches,
        )
        for hi, (actual_hole, expected_hole) in enumerate(
            zip(actual_holes, expected_holes)
        ):
            compare_scalar(
                f"{path}[{oi}].holes[{hi}].len",
                len(actual_hole),
                len(expected_hole),
                mismatches,
            )
            for pi, (actual_point, expected_point) in enumerate(
                zip(actual_hole, expected_hole)
            ):
                compare_vec(
                    f"{path}[{oi}].holes[{hi}][{pi}]",
                    {"x": actual_point[0], "y": actual_point[1]},
                    {"x": expected_point[0], "y": expected_point[1]},
                    mismatches,
                    tolerance,
                )


def compare_file(
    stem: str,
    actual: ParserFile,
    expected: dict[str, Any],
    max_mismatches: int,
    strict: bool = False,
) -> list[str]:
    mismatches: list[str] = []
    scalar_tolerance = 0.0 if strict else 0.000001
    coordinate_tolerance = 0 if strict else 1
    bounding_box_tolerance = 0 if strict else 100
    rotation_tolerance = 0.0 if strict else 0.01
    macro_param_tolerance = 0.0 if strict else 0.001

    if actual.errors:
        mismatches.extend(f"parse error: {err}" for err in actual.errors)
        return mismatches

    if actual.metadata is None:
        return ["missing parser metadata"]

    gm = expected["metadata"]
    rm = actual.metadata

    for field_name in [
        "fileName", "isMetric", "isX2", "imageNegative", "fileFunction", "itemCount", "dcodeCount", "imageRotation",
        "localRotation", "swapAxis", "mirrorA", "mirrorB", "imageJustifyXCenter", "imageJustifyYCenter",
        "noTrailingZeros", "relative",
    ]:
        compare_scalar(
            f"metadata.{field_name}",
            rm[field_name],
            gm.get(field_name),
            mismatches,
            scalar_tolerance,
        )

    for field_name in ["imageOffset", "offset", "scale", "imageJustifyOffset", "fmtScale", "fmtLen"]:
        compare_vec(
            f"metadata.{field_name}",
            rm[field_name],
            gm.get(field_name, {}),
            mismatches,
            coordinate_tolerance,
        )

    golden_dcodes = {int(d["num"]): d for d in expected.get("dcodes", [])}
    compare_scalar("dcodes.keys", sorted(actual.dcodes.keys()), sorted(golden_dcodes.keys()), mismatches)

    for num, gd in golden_dcodes.items():
        rd = actual.dcodes.get(num)
        if rd is None:
            mismatches.append(f"dcodes[{num}]: missing in actual output")
            continue

        for field_name in ["num", "type", "drillShape", "edgesCount", "inUse", "defined", "aperFunction"]:
            compare_scalar(f"dcodes[{num}].{field_name}", rd[field_name], gd.get(field_name), mismatches)
        compare_scalar(
            f"dcodes[{num}].rotation",
            rd["rotation"],
            gd.get("rotation", 0),
            mismatches,
            rotation_tolerance,
        )
        compare_vec(
            f"dcodes[{num}].size",
            rd["size"],
            gd.get("size", {}),
            mismatches,
            coordinate_tolerance,
        )
        compare_vec(
            f"dcodes[{num}].drill",
            rd["drill"],
            gd.get("drill", {}),
            mismatches,
            coordinate_tolerance,
        )
        if "macroName" in gd:
            compare_scalar(f"dcodes[{num}].macroName", rd.get("macroName"), gd.get("macroName"), mismatches)
        if "macroParams" in gd:
            actual_params = rd.get("macroParams", [])
            expected_params = gd.get("macroParams", [])
            compare_scalar(f"dcodes[{num}].macroParams.len", len(actual_params), len(expected_params), mismatches)
            for index, (actual_param, expected_param) in enumerate(zip(actual_params, expected_params)):
                compare_scalar(
                    f"dcodes[{num}].macroParams[{index}]",
                    actual_param,
                    expected_param,
                    mismatches,
                    macro_param_tolerance,
                )
        if "polygon" in gd:
            compare_polyset(
                f"dcodes[{num}].polygon",
                rd.get("polygon", []),
                gd.get("polygon", []),
                mismatches,
                coordinate_tolerance,
                not strict,
            )

    golden_items = expected.get("items", [])
    compare_scalar("items.len", len(actual.items), len(golden_items), mismatches)

    for idx, gi in enumerate(golden_items[: len(actual.items)]):
        if idx >= len(actual.items):
            mismatches.append(f"items[{idx}]: missing in actual output")
            break
        ri = actual.items[idx]
        for field_name in ["shapeType", "dcode", "flashed", "unitsMetric", "layerNegative", "aperFunction"]:
            compare_scalar(f"items[{idx}].{field_name}", ri[field_name], gi.get(field_name), mismatches)
        compare_vec(
            f"items[{idx}].start",
            ri["start"],
            gi.get("start", {}),
            mismatches,
            coordinate_tolerance,
        )
        compare_vec(
            f"items[{idx}].end",
            ri["end"],
            gi.get("end", {}),
            mismatches,
            coordinate_tolerance,
        )
        compare_vec(
            f"items[{idx}].size",
            ri["size"],
            gi.get("size", {}),
            mismatches,
            coordinate_tolerance,
        )
        if "arcCentre" in gi:
            compare_vec(
                f"items[{idx}].arcCentre",
                ri.get("arcCentre", {}),
                gi.get("arcCentre", {}),
                mismatches,
                coordinate_tolerance,
            )
        compare_net_attrs(f"items[{idx}].netAttributes", ri["netAttributes"], gi.get("netAttributes", {}), mismatches)
        if "boundingBox" in gi:
            compare_box(
                f"items[{idx}].boundingBox",
                ri.get("boundingBox", {}),
                gi.get("boundingBox", {}),
                mismatches,
                bounding_box_tolerance,
            )
        if "shapeAsPolygon" in gi:
            compare_polyset(
                f"items[{idx}].shapeAsPolygon",
                ri.get("shapeAsPolygon", []),
                gi.get("shapeAsPolygon", []),
                mismatches,
                coordinate_tolerance,
                not strict,
            )
        if "macroShapePolygon" in gi:
            compare_polyset(
                f"items[{idx}].macroShapePolygon",
                ri.get("macroShapePolygon", []),
                gi.get("macroShapePolygon", []),
                mismatches,
                coordinate_tolerance,
                not strict,
            )

        if "aperture" in gi and ri["dcode"] in actual.dcodes:
            rd = actual.dcodes[ri["dcode"]]
            compare_scalar(f"items[{idx}].aperture.type", rd["type"], gi["aperture"].get("type"), mismatches)
            compare_vec(
                f"items[{idx}].aperture.size",
                rd["size"],
                gi["aperture"].get("size", {}),
                mismatches,
                coordinate_tolerance,
            )

        if len(mismatches) >= max_mismatches:
            mismatches.append(f"stopped after {max_mismatches} mismatches")
            break

    return mismatches


def compare_net_attrs(path: str, rust: dict[str, Any], golden: dict[str, Any], mismatches: list[str]) -> None:
    mapping = {
        "netAttribType": "netAttribType",
        "netname": "netname",
        "cmpref": "cmpref",
        "padname": "padname",
        "pinFunction": "pinFunction",
    }
    for rust_key, golden_key in mapping.items():
        compare_scalar(f"{path}.{golden_key}", rust.get(rust_key), golden.get(golden_key, "" if rust_key != "netAttribType" else 0), mismatches)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-dir", required=True, type=Path, help="Directory containing Gerber files")
    parser.add_argument("--golden-dir", required=True, type=Path, help="Directory containing KiCad golden JSON files")
    parser.add_argument("--crate-dir", type=Path, default=Path(__file__).resolve().parents[1], help="gerber-parse crate directory")
    parser.add_argument(
        "--web-dir",
        type=Path,
        default=Path(__file__).resolve().parents[2] / "web",
        help="Web parser project directory",
    )
    parser.add_argument("--web-output-dir", type=Path, help="Optional directory to retain generated Web JSON files")
    parser.add_argument("--max-mismatches", type=int, default=20, help="Maximum mismatches to print per file")
    parser.add_argument("--strict", action="store_true", help="Require exact field and polygon vertex equality")
    parser.add_argument("--keep-runner", action="store_true", help="Keep temporary Rust runner for debugging")
    parser.add_argument("--keep-web-output", action="store_true", help="Keep temporary Web JSON output for debugging")
    args = parser.parse_args()

    input_dir = args.input_dir.resolve()
    rust = run_rust_exporter(args.crate_dir.resolve(), input_dir, args.keep_runner)
    web, web_json = run_web_exporter(
        args.web_dir.resolve(),
        input_dir,
        args.web_output_dir.resolve() if args.web_output_dir else None,
        args.keep_web_output,
    )
    golden = load_golden(args.golden_dir.resolve())

    gerber_stems = sorted(path.stem for path in input_dir.iterdir() if path.is_file() and path.suffix.lower() in GERBER_EXTS)
    stats = {
        "rust-vs-kicad": {"matched": 0, "mismatched": 0},
        "web-vs-kicad": {"matched": 0, "mismatched": 0},
        "rust-vs-web": {"matched": 0, "mismatched": 0},
    }
    total = missing_golden = 0

    for stem in gerber_stems:
        total += 1
        if stem not in golden:
            missing_golden += 1
            print(f"MISSING_GOLDEN {stem}")
            continue

        rust_file = rust.get(stem, ParserFile(errors=["missing Rust output"]))
        web_file = web.get(stem, ParserFile(errors=["missing Web output"]))
        comparisons = [
            ("rust-vs-kicad", rust_file, golden[stem]),
            ("web-vs-kicad", web_file, golden[stem]),
        ]

        if stem in web_json:
            comparisons.append(("rust-vs-web", rust_file, web_json[stem]))
        else:
            comparisons.append(("rust-vs-web", ParserFile(errors=["missing Web output"]), golden[stem]))

        for label, actual, expected in comparisons:
            file_mismatches = compare_file(
                stem,
                actual,
                expected,
                args.max_mismatches,
                args.strict,
            )
            if file_mismatches:
                stats[label]["mismatched"] += 1
                print(f"FAIL [{label}] {stem}: {len(file_mismatches)} mismatch(es)")
                for mismatch in file_mismatches[: args.max_mismatches + 1]:
                    print(f"  {mismatch}")
            else:
                stats[label]["matched"] += 1
                item_count = expected["metadata"].get("itemCount")
                dcode_count = expected["metadata"].get("dcodeCount")
                print(f"OK [{label}] {stem}: items={item_count}, dcodes={dcode_count}")

    summary = " ".join(
        f"{label}_matched={values['matched']} {label}_mismatched={values['mismatched']}"
        for label, values in stats.items()
    )
    print(f"SUMMARY total={total} missing_golden={missing_golden} {summary}")
    mismatched = sum(values["mismatched"] for values in stats.values())
    return 0 if mismatched == 0 and missing_golden == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
