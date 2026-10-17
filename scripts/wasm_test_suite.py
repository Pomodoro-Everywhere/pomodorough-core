"""Discover classified WASM tests and run every candidate gate on one artifact."""
from __future__ import annotations

import argparse
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = "tests/wasm_test_suite.json"
CATEGORIES = {"candidate", "historical", "support"}


def unique_fields(pairs: list[tuple[str, object]]) -> dict[str, object]:
    fields: dict[str, object] = {}
    for name, value in pairs:
        if name in fields:
            raise ValueError(f"duplicate manifest category: {name}")
        fields[name] = value
    return fields


def classify(manifest: object, actual: set[str]) -> dict[str, list[str]]:
    if not isinstance(manifest, dict) or set(manifest) != CATEGORIES:
        raise ValueError("manifest must classify candidate, historical, and support scripts")
    classified: set[str] = set()
    for category, paths in manifest.items():
        if not isinstance(paths, list) or not paths:
            raise ValueError(f"{category} must be a nonempty list")
        for path in paths:
            if not isinstance(path, str):
                raise ValueError("script paths must be strings")
            relative = PurePosixPath(path)
            if (not relative.parts or relative.is_absolute() or ".." in relative.parts
                    or relative.parts[0] != "tests" or relative.suffix != ".mjs"
                    or relative.as_posix() != path):
                raise ValueError(f"invalid test script path: {path}")
            if path in classified:
                raise ValueError(f"script classified more than once: {path}")
            classified.add(path)
    missing = classified - actual
    unclassified = actual - classified
    if missing or unclassified:
        raise ValueError(f"missing scripts: {sorted(missing)}; unclassified scripts: {sorted(unclassified)}")
    return manifest


def discover(root: Path) -> dict[str, list[str]]:
    manifest = json.loads((root / MANIFEST).read_text(encoding="utf-8"), object_pairs_hook=unique_fields)
    actual = {path.relative_to(root).as_posix() for path in (root / "tests").rglob("*.mjs")}
    return classify(manifest, actual)


def run_candidate(root: Path, suite: dict[str, list[str]], artifact: Path) -> None:
    artifact = artifact.resolve(strict=True)
    if not artifact.is_file():
        raise ValueError(f"candidate artifact is not a file: {artifact}")
    for script in suite["candidate"]:
        print(f"Candidate WASM gate: {script}", flush=True)
        subprocess.run(["node", script, str(artifact)], cwd=root, stdin=subprocess.DEVNULL, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", nargs="?", type=Path, help="exact canonical candidate WASM")
    parser.add_argument("--list", action="store_true", help="validate and print every script classification")
    arguments = parser.parse_args()
    if arguments.list == (arguments.artifact is not None):
        parser.error("provide either an artifact or --list")
    try:
        suite = discover(ROOT)
        if arguments.list:
            print(json.dumps(suite, indent=2))
        else:
            run_candidate(ROOT, suite, arguments.artifact)
    except (OSError, ValueError) as error:
        print(f"WASM test discovery failed: {error}", file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f"Candidate WASM gate failed: {error.cmd[1]}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
