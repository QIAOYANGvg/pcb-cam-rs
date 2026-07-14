## Packaged KiCad Golden Exporter

This directory contains the pinned Windows exporter package used as the
independent oracle for `gerber-parse` cross-validation.

- Entry point: `export_gerber_golden.exe`
- Validation wrapper: `python tools/validate_packaged_kicad.py`
- Output policy: generated KiCad JSON stays under ignored `validation-results/`

The packaged exporter SHA-256 is:

`68e57d6fa45358948519abe9949ef05de75f81d075ae1c971a5f0de482045130`

Do not edit generated JSON into the repository. Replace this package only for
an intentional KiCad exporter change, then rerun the full 105-file strict gate.
