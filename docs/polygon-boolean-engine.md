# Polygon Boolean Engine

## Active implementation

`gerber-parse` uses `i_overlay` 7.0.2 for polygon union, difference, and
simplification. The implementation is pure Rust and lives in
`src/geometry.rs`.

The overlay configuration intentionally follows the previous KiCad/Clipper2
behavior:

- integer `i32` coordinates
- `NonZero` fill rule
- counter-clockwise outer contours
- preserved input and output collinear vertices
- zero minimum output area

Each `i_overlay` shape is converted to one `Polygon`: the first contour is the
outer outline and the remaining contours are holes.

## Historical Clipper2 reference

The former C++ bridge remains in the repository for reference:

- `cpp/clipper_bridge.cpp`
- `cpp/clipper_bridge.h`
- `../thirdparty/clipper2/Clipper2Lib`

These files are no longer compiled by Cargo. The old Rust FFI module and Cargo
build script were removed when the production implementation moved to
`i_overlay`.

## Compatibility

Compatibility tests in `tests/i_overlay_compat.rs` use fixtures captured from
the previous Clipper2 implementation. They compare polygon geometry after
normalizing contour start positions, winding, polygon order, and redundant
collinear split points.

One known representation difference is retained as an ignored strict test:
when several overlapping contours are simplified, `i_overlay` may retain a
redundant collinear split point that Clipper2 removes. This does not change the
boundary, area, holes, or rendered pixels.

For the recorded case:

- both `earcut` triangulations cover area `200`
- the triangulations contain 12 and 11 triangles respectively
- a 512 by 512 Canvas raster comparison produced zero differing bytes

Code that validates polygon results should compare geometry or topology rather
than raw vertex arrays. Exact Clipper2 vertex sequencing is not an API
guarantee of the Rust implementation.
