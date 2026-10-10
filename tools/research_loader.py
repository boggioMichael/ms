#!/usr/bin/env python3
"""Load a research export of the MapleSyrup data program (P1): verify it, then read it.

Standard library only. Before reading a single row it checks the export against its
manifest.json: every file listed is present, with the SHA-256, the size and (for .jsonl) the
number of rows the manifest gives; the manifest and schema versions are ones this loader knows.
On any mismatch it refuses: exit code 2 and the reason on stderr. It never "repairs" a file.

Missing values are JSON null and stay None here: do not coerce them to 0.

Usage:
    python3 tools/research_loader.py <export-folder>          # verify, load, print a summary
    python3 tools/research_loader.py <export-folder> --json   # the summary as JSON

As a module:
    from research_loader import load
    data = load("path/to/export")   # {"manifest": {...}, "events": [...], "episodes": [...]}

The format is described in docs/data-program/DATA_CONTRACTS.md.
"""

import argparse
import hashlib
import json
import sys
from pathlib import Path

MANIFEST_VERSIONS = {1}
SCHEMA_VERSIONS = {"0.1.0"}


class Refused(Exception):
    """The export does not match its manifest (or is not one this loader knows)."""


def _sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 16), b""):
            digest.update(block)
    return digest.hexdigest()


def _no_constants(name):
    raise Refused(f"a non-JSON number ({name}) in a row")


def verify(folder):
    """The manifest of the export in `folder`, after checking every file it lists."""
    folder = Path(folder)
    manifest_path = folder / "manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise Refused(f"manifest.json: {error}") from error
    if manifest.get("manifest_version") not in MANIFEST_VERSIONS:
        raise Refused(f"manifest version {manifest.get('manifest_version')!r} is not known")
    if manifest.get("schema_version") not in SCHEMA_VERSIONS:
        raise Refused(f"schema version {manifest.get('schema_version')!r} is not known")
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise Refused("manifest.json lists no files")
    for entry in files:
        name = entry.get("path", "")
        if not name or "/" in name or "\\" in name or name.startswith("."):
            raise Refused(f"{name!r}: not a file of this folder")
        path = folder / name
        if not path.is_file():
            raise Refused(f"{name}: missing")
        actual = _sha256(path)
        if actual != entry.get("sha256"):
            raise Refused(f"{name}: checksum mismatch (manifest {entry.get('sha256')}, file {actual})")
        size = path.stat().st_size
        if size != entry.get("bytes"):
            raise Refused(f"{name}: size mismatch (manifest {entry.get('bytes')}, file {size})")
        if entry.get("rows") is not None:
            with open(path, "rb") as handle:
                rows = sum(1 for line in handle if line.strip())
            if rows != entry["rows"]:
                raise Refused(f"{name}: {rows} rows, manifest says {entry['rows']}")
    listed = {entry["path"] for entry in files} | {"manifest.json"}
    extra = sorted(p.name for p in folder.iterdir() if p.is_file() and p.name not in listed)
    if extra:
        print(f"warning: files not in the manifest (not loaded): {', '.join(extra)}", file=sys.stderr)
    return manifest


def _rows(path, schema_version, check_schema):
    rows = []
    with open(path, encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                row = json.loads(line, parse_constant=_no_constants)
            except ValueError as error:
                raise Refused(f"{path.name} line {number}: {error}") from error
            if check_schema and row.get("schema_version") != schema_version:
                raise Refused(f"{path.name} line {number}: schema {row.get('schema_version')!r}")
            rows.append(row)
    return rows


def load(folder):
    """Verify the export in `folder`, then read its events and episodes."""
    folder = Path(folder)
    manifest = verify(folder)
    schema = manifest["schema_version"]
    return {
        "manifest": manifest,
        "events": _rows(folder / "events.jsonl", schema, True),
        "episodes": _rows(folder / "episodes.jsonl", schema, True),
    }


def summary(data):
    manifest = data["manifest"]
    outcomes = {}
    for episode in data["episodes"]:
        result = (episode.get("outcome") or {}).get("result")
        outcomes[result] = outcomes.get(result, 0) + 1
    return {
        "export_id": manifest["export_id"],
        "revision": manifest["revision"],
        "synthetic": manifest["synthetic"],
        "purpose": manifest["purpose"],
        "recipient_class": manifest["recipient_class"],
        "files_verified": len(manifest["files"]),
        "events": len(data["events"]),
        "episodes": len(data["episodes"]),
        "participants": manifest["counts"]["participants"],
        "outcomes": dict(sorted(outcomes.items(), key=lambda kv: str(kv[0]))),
        "excluded": manifest.get("excluded", {}),
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("folder", help="the export folder (holding manifest.json)")
    parser.add_argument("--json", action="store_true", help="print the summary as JSON")
    args = parser.parse_args(argv)
    try:
        data = load(args.folder)
    except Refused as error:
        print(f"refused: {error}", file=sys.stderr)
        return 2
    result = summary(data)
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        label = " (SYNTHETIC)" if result["synthetic"] else ""
        print(
            f"verified: {result['files_verified']} files of export {result['export_id']} "
            f"revision {result['revision']}{label}"
        )
        print(
            f"loaded: {result['events']} events, {result['episodes']} episodes, "
            f"{result['participants']} participants; outcomes {result['outcomes']}"
        )
        if result["excluded"]:
            print(f"left out by the gate (rows): {result['excluded']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
