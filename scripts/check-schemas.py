#!/usr/bin/env python3
"""Validate zthermal JSON output against schemas/*.json.

Usage: scripts/check-schemas.py <zthermal binary>
Runs the binary on the synthetic fixtures in tests/fixtures and validates snapshot,
inspection and profile documents. Needs the `jsonschema` package (>= 4.18, MIT licence).
"""

import json
import pathlib
import subprocess
import sys

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = pathlib.Path(__file__).resolve().parent.parent


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    binary = sys.argv[1]
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
