#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 ZairenkaiPrjctX contributors
"""Validate zthermal JSON output against schemas/*.json.

Usage: scripts/check-schemas.py <zthermal binary> [--snapshot FILE ...]
Runs the binary on the synthetic fixtures in tests/fixtures and validates snapshot,
inspection and profile documents. `--snapshot FILE` also validates snapshots taken elsewhere
(for example `adb pull`ed from a device). Needs the `jsonschema` package (>= 4.18, MIT).
"""

import json
import pathlib
import subprocess
import sys

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = pathlib.Path(__file__).resolve().parent.parent


def main() -> int:
    args = sys.argv[1:]
    extra = []
    while "--snapshot" in args:
        i = args.index("--snapshot")
        if i + 1 >= len(args):
            print(__doc__, file=sys.stderr)
            return 2
        extra.append(args[i + 1])
        del args[i : i + 2]
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    binary = args[0]
    schemas = {p.name: json.loads(p.read_text()) for p in (ROOT / "schemas").glob("*.json")}
    for s in schemas.values():
        Draft202012Validator.check_schema(s)
    registry = Registry().with_resources([(s["$id"], Resource.from_contents(s)) for s in schemas.values()])

    def run(*args: str) -> object:
        out = subprocess.run([binary, *args], cwd=ROOT, capture_output=True, text=True, check=False)
        if out.returncode not in (0, 3, 5):
            raise SystemExit(f"{args}: exit {out.returncode}: {out.stderr}")
        return json.loads(out.stdout)

    docs = []
    for d in sorted((ROOT / "tests/fixtures/devices").iterdir()):
        docs.append(("snapshot.schema.json", f"snapshot {d.name}", run("snapshot", "--framework", "--root", str(d))))
    for f in sorted((ROOT / "tests/fixtures/configs").iterdir()):
        docs.append(("inspection.schema.json", f"inspect {f.name}", run("inspect-config", "--json", str(f))))
    for f in extra:
        docs.append(("snapshot.schema.json", f"snapshot file {f}", json.loads(pathlib.Path(f).read_text())))
    for f in sorted((ROOT / "profiles").glob("*.json")):
        docs.append(("profile.schema.json", f"profile {f.name}", json.loads(f.read_text())))

    failed = 0
    for schema, name, doc in docs:
        errors = list(Draft202012Validator(schemas[schema], registry=registry).iter_errors(doc))
        print(f"{'ok  ' if not errors else 'FAIL'} {schema:24} {name}")
        for e in errors[:5]:
            print(f"     {list(e.absolute_path)}: {e.message}")
        failed += bool(errors)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
