#!/usr/bin/env python3
"""Download and flatten the pinned tracespace Gerber fixture corpus."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import tempfile
import urllib.request
import zipfile
from pathlib import Path

TRACESPACE_REPOSITORY = "https://github.com/tracespace/tracespace"
TRACESPACE_COMMIT = "64bd2ab74e6dde92aec54052893317d1b80b3ecc"
TRACESPACE_ARCHIVE = (
    f"https://github.com/tracespace/tracespace/archive/{TRACESPACE_COMMIT}.zip"
)
EXPECTED_GERBER_COUNT = 105
EXPECTED_CORPUS_DIGEST = (
    "bbfd165f6cfeb618ae1fdd1aaf3bb08d83404be6c048795c33a1fd171e6d8be7"
)
GERBER_EXTENSIONS = {
    ".art",
    ".g1",
    ".g2",
    ".g3",
    ".g4",
    ".g5",
    ".g6",
    ".g7",
    ".g8",
    ".gbl",
    ".gbo",
    ".gbp",
    ".gbr",
    ".gbs",
    ".gcl",
    ".gdd",
    ".gdl",
    ".ger",
    ".gko",
    ".gml",
    ".gto",
    ".gtl",
    ".gtp",
    ".gts",
    ".pho",
}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def corpus_digest(files: list[dict[str, object]]) -> str:
    digest = hashlib.sha256()
    for entry in sorted(files, key=lambda item: str(item["source"])):
        digest.update(str(entry["source"]).encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(entry["staged"]).encode("utf-8"))
        digest.update(b"\0")
        digest.update(str(entry["size"]).encode("ascii"))
        digest.update(b"\0")
        digest.update(str(entry["sha256"]).encode("ascii"))
        digest.update(b"\n")
    return digest.hexdigest()


def staged_name(relative_path: Path) -> str:
    stem = "__".join(relative_path.with_suffix("").parts)
    stem = re.sub(r"[^A-Za-z0-9._-]+", "_", stem).strip("._")
    extension = relative_path.suffix.lower()
    extension_tag = extension.lstrip(".")
    return f"{stem}__{extension_tag}{extension}"


def download_archive(destination: Path) -> None:
    request = urllib.request.Request(
        TRACESPACE_ARCHIVE,
        headers={"User-Agent": "gerber-parse-corpus-fetcher"},
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        destination.write_bytes(response.read())


def verify_manifest(manifest: dict[str, object]) -> None:
    if manifest.get("repository") != TRACESPACE_REPOSITORY:
        raise RuntimeError("Unexpected tracespace repository in corpus manifest")
    if manifest.get("commit") != TRACESPACE_COMMIT:
        raise RuntimeError("Unexpected tracespace commit in corpus manifest")

    files = manifest.get("files")
    if not isinstance(files, list):
        raise RuntimeError("Corpus manifest does not contain a file list")
    if manifest.get("count") != EXPECTED_GERBER_COUNT:
        raise RuntimeError(
            f"Expected manifest count {EXPECTED_GERBER_COUNT}, "
            f"found {manifest.get('count')}"
        )
    if len(files) != EXPECTED_GERBER_COUNT:
        raise RuntimeError(
            f"Expected {EXPECTED_GERBER_COUNT} manifest entries, "
            f"found {len(files)}"
        )

    actual_digest = corpus_digest(files)
    if actual_digest != EXPECTED_CORPUS_DIGEST:
        raise RuntimeError(
            f"Corpus digest mismatch: actual={actual_digest} "
            f"expected={EXPECTED_CORPUS_DIGEST}"
        )
    if manifest.get("digest") != EXPECTED_CORPUS_DIGEST:
        raise RuntimeError("Corpus manifest digest field is missing or incorrect")


def verify_corpus(
    output_dir: Path,
    manifest: dict[str, object],
) -> list[Path]:
    verify_manifest(manifest)

    input_dir = output_dir.resolve() / "input"
    files = manifest["files"]
    entries_by_name = {
        str(entry["staged"]): entry
        for entry in files
        if isinstance(entry, dict)
    }
    if len(entries_by_name) != EXPECTED_GERBER_COUNT:
        raise RuntimeError("Corpus manifest contains duplicate staged names")
    if len({Path(name).stem for name in entries_by_name}) != EXPECTED_GERBER_COUNT:
        raise RuntimeError("Corpus manifest contains duplicate staged stems")

    input_files = sorted(
        path
        for path in input_dir.iterdir()
        if path.is_file() and path.suffix.lower() in GERBER_EXTENSIONS
    )
    if {path.name for path in input_files} != set(entries_by_name):
        missing = sorted(set(entries_by_name) - {path.name for path in input_files})
        extra = sorted({path.name for path in input_files} - set(entries_by_name))
        raise RuntimeError(
            f"Corpus file set mismatch: missing={missing} extra={extra}"
        )

    for path in input_files:
        entry = entries_by_name[path.name]
        if path.stat().st_size != int(entry["size"]):
            raise RuntimeError(f"Corpus size mismatch: {path.name}")
        if sha256(path) != entry["sha256"]:
            raise RuntimeError(f"Corpus SHA-256 mismatch: {path.name}")

    return input_files


def prepare_corpus(output_dir: Path) -> dict[str, object]:
    output_dir = output_dir.resolve()
    input_dir = output_dir / "input"
    manifest_path = output_dir / "manifest.json"

    with tempfile.TemporaryDirectory(prefix="tracespace-gerber-") as temp_name:
        temp_dir = Path(temp_name)
        archive_path = temp_dir / "tracespace.zip"
        download_archive(archive_path)

        with zipfile.ZipFile(archive_path) as archive:
            archive.extractall(temp_dir / "source")

        roots = [
            path
            for path in (temp_dir / "source").iterdir()
            if path.is_dir() and path.name.startswith("tracespace-")
        ]
        if len(roots) != 1:
            raise RuntimeError("Could not locate the tracespace archive root")

        repository_root = roots[0]
        fixtures_root = repository_root / "packages" / "fixtures"
        source_files = sorted(
            path
            for path in fixtures_root.rglob("*")
            if path.is_file() and path.suffix.lower() in GERBER_EXTENSIONS
        )

        if len(source_files) != EXPECTED_GERBER_COUNT:
            raise RuntimeError(
                f"Expected {EXPECTED_GERBER_COUNT} Gerber files at "
                f"{TRACESPACE_COMMIT}, found {len(source_files)}"
            )

        if input_dir.parent != output_dir:
            raise RuntimeError(f"Unsafe corpus input path: {input_dir}")
        if input_dir.exists():
            shutil.rmtree(input_dir)
        input_dir.mkdir(parents=True)

        files = []
        used_names: set[str] = set()

        for source_path in source_files:
            relative_path = source_path.relative_to(fixtures_root)
            name = staged_name(relative_path)
            if name in used_names:
                raise RuntimeError(f"Flattened corpus name collision: {name}")
            used_names.add(name)

            destination = input_dir / name
            shutil.copyfile(source_path, destination)
            files.append(
                {
                    "source": relative_path.as_posix(),
                    "staged": name,
                    "size": destination.stat().st_size,
                    "sha256": sha256(destination),
                }
            )

        output_dir.mkdir(parents=True, exist_ok=True)
        manifest = {
            "repository": TRACESPACE_REPOSITORY,
            "commit": TRACESPACE_COMMIT,
            "count": len(files),
            "digest": corpus_digest(files),
            "files": files,
        }
        verify_manifest(manifest)
        manifest_path.write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

        license_candidates = sorted(repository_root.glob("LICENSE*"))
        if license_candidates:
            shutil.copyfile(license_candidates[0], output_dir / "SOURCE_LICENSE")

    verify_corpus(output_dir, manifest)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[1]
        / "test-corpus"
        / "tracespace-v5",
        help="Generated corpus directory",
    )
    args = parser.parse_args()

    manifest = prepare_corpus(args.output)
    print(
        f"Prepared {manifest['count']} Gerber files from "
        f"{manifest['commit']} in {args.output.resolve()}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
