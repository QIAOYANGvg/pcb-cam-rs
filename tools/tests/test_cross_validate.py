import math
import copy
import sys
import unittest
import os
from pathlib import Path
from unittest import mock

TOOLS_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS_DIR))

import cross_validate
import fetch_tracespace_corpus
import validate_tracespace


def points(values: list[tuple[int, int]]) -> list[dict[str, int]]:
    return [{"x": x, "y": y} for x, y in values]


def polygon(values: list[tuple[int, int]]) -> list[dict[str, object]]:
    return [{"outline": points(values), "holes": []}]


def regular_polygon(radius: int, vertex_count: int) -> list[dict[str, object]]:
    return polygon(
        [
            (
                round(radius * math.cos(2 * math.pi * index / vertex_count)),
                round(radius * math.sin(2 * math.pi * index / vertex_count)),
            )
            for index in range(vertex_count)
        ]
    )


def minimal_golden() -> dict[str, object]:
    metadata = {
        "fileName": "",
        "isMetric": True,
        "isX2": False,
        "imageNegative": False,
        "fileFunction": None,
        "itemCount": 0,
        "dcodeCount": 0,
        "imageOffset": {"x": 0, "y": 0},
        "imageRotation": 0,
        "localRotation": 0.0,
        "offset": {"x": 0, "y": 0},
        "scale": {"x": 1, "y": 1},
        "swapAxis": False,
        "mirrorA": False,
        "mirrorB": False,
        "imageJustifyOffset": {"x": 0, "y": 0},
        "imageJustifyXCenter": False,
        "imageJustifyYCenter": False,
        "fmtScale": {"x": 4, "y": 4},
        "fmtLen": {"x": 6, "y": 6},
        "noTrailingZeros": False,
        "relative": False,
    }
    return {"metadata": metadata, "dcodes": [], "items": []}


class PolygonComparisonTests(unittest.TestCase):
    def test_ring_start_direction_and_polygon_order_are_ignored(self) -> None:
        first = [(0, 0), (100, 0), (100, 100), (0, 100)]
        second = [(200, 200), (250, 200), (250, 250), (200, 250)]
        actual = polygon(first) + polygon(second)
        expected = polygon(list(reversed(second[1:] + second[:1]))) + polygon(
            first[2:] + first[:2]
        )
        mismatches: list[str] = []

        cross_validate.compare_polyset(
            "shape",
            actual,
            expected,
            mismatches,
            tolerance=0,
            remove_redundant_collinear=False,
        )

        self.assertEqual(mismatches, [])

    def test_different_circle_tessellations_are_rejected_in_strict_mode(self) -> None:
        mismatches: list[str] = []

        cross_validate.compare_polyset(
            "shape",
            regular_polygon(300_000, 64),
            regular_polygon(300_000, 72),
            mismatches,
            tolerance=0,
            remove_redundant_collinear=False,
        )

        self.assertTrue(any(".outline.len:" in mismatch for mismatch in mismatches))

    def test_translation_outside_coordinate_tolerance_is_rejected(self) -> None:
        actual = polygon([(2, 0), (102, 0), (102, 100), (2, 100)])
        expected = polygon([(0, 0), (100, 0), (100, 100), (0, 100)])
        mismatches: list[str] = []

        cross_validate.compare_polyset(
            "shape",
            actual,
            expected,
            mismatches,
            tolerance=0,
            remove_redundant_collinear=False,
        )

        self.assertTrue(any(".outline[0].x:" in mismatch for mismatch in mismatches))

    def test_material_shape_change_is_rejected_vertex_by_vertex(self) -> None:
        actual = polygon(
            [(0, 0), (100_000, 0), (100_000, 100_000), (0, 100_000)]
        )
        expected = polygon(
            [
                (0, 0),
                (100_000, 0),
                (100_000, 100_000),
                (60_000, 100_000),
                (50_000, 50_000),
                (40_000, 100_000),
                (0, 100_000),
            ]
        )
        mismatches: list[str] = []

        cross_validate.compare_polyset(
            "shape",
            actual,
            expected,
            mismatches,
            tolerance=0,
            remove_redundant_collinear=False,
        )

        self.assertTrue(any(".outline.len:" in mismatch for mismatch in mismatches))
        self.assertTrue(any(".outline[" in mismatch for mismatch in mismatches))


class TracespaceCorpusTests(unittest.TestCase):
    def test_staged_names_have_unique_stems_for_layer_extensions(self) -> None:
        top = fetch_tracespace_corpus.staged_name(
            Path("boards/core/core.GTL")
        )
        bottom = fetch_tracespace_corpus.staged_name(
            Path("boards/core/core.GBL")
        )

        self.assertEqual(top, "boards__core__core__gtl.gtl")
        self.assertEqual(bottom, "boards__core__core__gbl.gbl")
        self.assertNotEqual(Path(top).stem, Path(bottom).stem)


class GoldenSchemaTests(unittest.TestCase):
    def test_current_field_contract_is_accepted(self) -> None:
        self.assertEqual(
            cross_validate.audit_golden_schema("fixture", minimal_golden()),
            [],
        )

    def test_unknown_golden_field_is_rejected(self) -> None:
        golden = minimal_golden()
        golden["metadata"]["newKiCadField"] = 1

        errors = cross_validate.audit_golden_schema("fixture", golden)

        self.assertTrue(
            any("unknown field newKiCadField" in error for error in errors)
        )

    def test_file_name_is_part_of_strict_field_comparison(self) -> None:
        golden = minimal_golden()
        actual = cross_validate.ParserFile(
            metadata=copy.deepcopy(golden["metadata"]),
        )

        self.assertEqual(
            cross_validate.compare_file(
                "fixture",
                actual,
                golden,
                max_mismatches=20,
                strict=True,
            ),
            [],
        )

        actual.metadata["fileName"] = "unexpected.gbr"
        mismatches = cross_validate.compare_file(
            "fixture",
            actual,
            golden,
            max_mismatches=20,
            strict=True,
        )
        self.assertTrue(
            any("metadata.fileName" in mismatch for mismatch in mismatches)
        )


class ExporterConfigurationTests(unittest.TestCase):
    def test_packaged_exporter_path_is_preferred(self) -> None:
        crate_dir = Path(__file__).resolve().parents[2]
        candidates = validate_tracespace.exporter_candidates(crate_dir)
        self.assertEqual(
            candidates[0],
            crate_dir / "tools" / "kicad-exporter" / "export_gerber_golden.exe",
        )

    def test_exporter_candidates_include_environment_override(self) -> None:
        crate_dir = Path(__file__).resolve().parents[2]
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("GERBER_GOLDEN_EXPORTER", None)
            self.assertEqual(
                validate_tracespace.exporter_candidates(crate_dir),
                [
                    crate_dir
                    / "tools"
                    / "kicad-exporter"
                    / "export_gerber_golden.exe"
                ],
            )

        with mock.patch.dict(
            os.environ,
            {"GERBER_GOLDEN_EXPORTER": r"C:\tools\export_gerber_golden.exe"},
            clear=False,
        ):
            candidates = validate_tracespace.exporter_candidates(crate_dir)
            self.assertEqual(
                candidates,
                [
                    crate_dir
                    / "tools"
                    / "kicad-exporter"
                    / "export_gerber_golden.exe",
                    Path(r"C:\tools\export_gerber_golden.exe"),
                ],
            )


if __name__ == "__main__":
    unittest.main()
