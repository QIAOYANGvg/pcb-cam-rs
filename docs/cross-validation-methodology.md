# Gerber Cross-Validation Methodology

## Architecture

`gerber-parse` stores the pinned 105-file tracespace Gerber corpus, its source
license, its SHA-256 manifest, and the packaged KiCad golden exporter under
`tools/kicad-exporter/`. It does not store KiCad-generated JSON.

Each validation run generates temporary golden JSON under
`validation-results/tracespace-v5/kicad-golden`, compares Rust against it, and
keeps only local reports. `validation-results/` is ignored by Git.

## Complete Gate

From the project root:

```powershell
python tools/validate_packaged_kicad.py
```

An explicit exporter can still be supplied, or `GERBER_GOLDEN_EXPORTER` can
override the packaged one:

```powershell
python tools/validate_all.py `
  --kicad-exporter C:\path\to\export_gerber_golden.exe
```

The gate runs Rust tests, Python schema tests, fresh KiCad export, and the full
105-file strict comparison.

`gerber-parse` does not auto-discover `.kiro` or a neighboring KiCad build.
The standalone repository either uses its packaged exporter or an explicit
override.

## Exactness Contract

- All covered scalar, string, boolean, coordinate, and bounding-box fields use
  zero tolerance.
- KiCad JSON with missing or unknown fields fails schema audit.
- Item order and D-code identity must match.
- Polygon vertex count and integer coordinates must match exactly.
- Polygon normalization only ignores contour start, winding, polygon order,
  and hole order.

Do not weaken this contract to pass a parser change.

## Persistent Corpus

`test-corpus/tracespace-v5` contains:

- `input/`: 105 uniquely named Gerber files;
- `manifest.json`: pinned tracespace commit, paths, sizes, and SHA-256 values;
- `SOURCE_LICENSE`: tracespace fixture license.

To refresh the pinned corpus deliberately:

```powershell
python tools/validate_tracespace.py `
  --refresh-corpus `
  --kicad-exporter C:\path\to\export_gerber_golden.exe
```

Review the complete file/hash diff before changing the expected corpus digest.

## Failure Handling

Start with the first structural mismatch. A missing item or polygon vertex can
shift every later comparison.

Reports classify failures as parser output, metadata, aperture, drawing item,
polygon geometry, or missing KiCad golden. Detailed output is written to
`validation-results/tracespace-v5/report.json`.

## CI

CI must provide Python, Rust, Cargo, a C++17 compiler, and the packaged exporter
artifact or `GERBER_GOLDEN_EXPORTER` environment variable:

```powershell
python tools/validate_packaged_kicad.py
```
