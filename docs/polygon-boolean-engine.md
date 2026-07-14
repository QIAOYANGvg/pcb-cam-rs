# Polygon Boolean Engine

## Active implementation

`gerber-parse` uses the same Clipper2 engine as KiCad for polygon union,
difference, and simplification. Cargo builds the small C ABI bridge in
`cpp/` together with the project-local sources in
`vendor/clipper2/Clipper2Lib`.

Using the same integer boolean engine is required because validation compares
KiCad and Rust polygon vertices exactly after only normalizing contour start
positions, winding, polygon order, and hole order.

## Build requirements

The crate is self-contained. A C++17 compiler is required in addition to the
Rust toolchain.

## Compatibility

`tests/clipper2_compat.rs` covers union, difference, simplification, nested
holes, preserved collinear vertices, and strict vertex output. The tracespace
cross-validation corpus provides the larger end-to-end regression suite.
