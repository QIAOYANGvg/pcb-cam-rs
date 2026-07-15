# Rendering Architecture

The Cargo workspace enforces four ownership boundaries:

1. `gerber-parse`: reads Gerber commands and produces `GerberFileImage` and
   `DrawItem`.
2. `gerber-render-plan`: converts parser-owned data into immutable,
   backend-neutral drawing operations with owned IR value types.
3. `gerber-render-wgpu`: lowers drawing operations into GPU geometry, renders
   into an offscreen coverage target, and returns RGBA pixels.
4. `gerber-cli`: composes the crates and owns file formats such as PNG and JSON.

The parser crate does not depend on `wgpu`, tessellation, windowing, image
encoders, or a frontend runtime. The WGPU crate does not depend directly on the
parser crate. Renderer code must not mutate parser-owned items, D-codes, or
polygon caches.

## Render Plan Contract

A render plan preserves source draw order. Each operation contains:

- effective polarity (`Dark` or `Clear`);
- source item index for diagnostics;
- conservative operation bounds;
- one semantic primitive such as a line stroke, arc stroke, or filled path.

Arc operations retain their center, endpoints, and width. They are not flattened
while building the plan. Filled paths retain outline and hole topology.

Colors, viewport dimensions, layer visibility, and user display modes belong to
the renderer configuration, not the plan.

## Coordinate Precision

Parser and plan coordinates remain integer Gerber internal units. The GPU
lowering stage rebases coordinates around the plan bounds before converting them
to `f32`. This prevents large absolute board coordinates from losing low bits in
GPU vertex buffers.

Arc tessellation is based on a geometric chord-error tolerance. Fixed angular
subdivision is not acceptable because its error grows with radius.

## Polarity

Operations are applied in source order:

- `Dark` adds coverage.
- `Clear` erases existing coverage.

Clear geometry is not represented as transparent source color under ordinary
alpha blending. The renderer uses explicit coverage replacement or an equivalent
erase pass. Aperture holes are path topology and remain part of one filled
primitive; they are not emitted as later clear operations.

The effective item polarity is the XOR of item layer polarity and image
polarity. Negative-image Gerber behavior remains an explicit compatibility area
and must not be silently approximated.

## GPU Backend

The first WGPU backend is offscreen and deterministic:

- CPU geometry lowering and tessellation;
- typed vertex/index buffers;
- multisampled color/coverage rendering;
- texture readback with row-padding handling;
- PNG encoding in the CLI boundary.

An interactive surface backend can reuse the same render plan and geometry
lowering later. The offscreen path must not require a window or display server.

The current CLI entry point is:

```text
cargo run -p gerber-cli -- --render input.gbr output.png [width height]
```

## Web Portability

The plan is intentionally independent of native handles. A future wasm build can
use the same parser and plan builder, then feed a WebGPU surface renderer. The
web frontend owns interaction, layer controls, and presentation colors; it does
not reinterpret Gerber geometry.

## Validation

Required coverage includes:

- every Gerber draw-item shape;
- clockwise, counter-clockwise, single-quadrant, and full-circle arcs;
- zero-length round strokes;
- dark/clear/dark ordering;
- drilled and macro apertures with holes;
- transforms, mirrors, rotations, and large coordinates;
- real-board offscreen rendering with non-empty pixel output.
