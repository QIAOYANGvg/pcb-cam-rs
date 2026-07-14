# Project Structure

The Rust source layout follows KiCad GerbView file responsibilities where that
improves traceability. Project-specific infrastructure is grouped separately.

## KiCad-aligned modules

| Rust module | KiCad source |
| --- | --- |
| `readgerb.rs` | `gerbview/readgerb.cpp` |
| `gerber_file_image.rs` | `gerbview/gerber_file_image.*` |
| `gerber_draw_item.rs` | `gerbview/gerber_draw_item.*` |
| `rs274_read_xy_and_ij_coordinates.rs` | `gerbview/rs274_read_XY_and_IJ_coordinates.cpp` |
| `rs274d.rs` | `gerbview/rs274d.cpp` |
| `rs274x.rs` | `gerbview/rs274x.cpp` |
| `dcode.rs` | `gerbview/dcode.*` |
| `am_param.rs` | `gerbview/am_param.*` |
| `am_primitive.rs` | `gerbview/am_primitive.*` |
| `aperture_macro.rs` | `gerbview/aperture_macro.*` |
| `evaluate.rs` | `gerbview/evaluate.cpp` |
| `x2_gerber_attributes.rs` | `gerbview/X2_gerber_attributes.*` |

## Rust-specific modules

- `geometry/` contains shared vectors, polygon sets, fracture logic, and shape
  generation. These replace KiCad infrastructure spread across `kimath` and
  GerbView.
- `export/` contains project-specific serializers and validation output.
- `cpp/` contains the active C ABI bridge to the exact Clipper2 engine.
- `vendor/clipper2/` contains the project-local Clipper2 source and license.
- `test-corpus/` contains the tracked Gerber corpus, source license, and
  integrity manifest. KiCad golden JSON is generated temporarily during
  validation runs.
- `tools/` contains validation scripts and the packaged KiCad golden exporter.

## Compatibility paths

`lib.rs` keeps the former public module names such as `coord`, `draw_item`,
`gerber_parser`, `golden_export`, and `x2_attribute` as re-exports. New code
should use the KiCad-aligned module names directly.
