#!/usr/bin/env python3
"""Decode native bootstrap output with the unchanged Desktop production methods.

Ownership outputs are outside this decoder's caller contract and remain rejected.
The probe retains both method bodies and their staticmethod/classmethod decorators.
"""

from __future__ import annotations

import argparse
import ast
import json
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]


def desktop_decoder(path: Path):
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef)
               and node.name == "SyncStorage"]
    assert len(classes) == 1, "Desktop must define one SyncStorage class"
    definition = classes[0]
    names = {"_completed_history_count", "_validated_bootstrap_plan"}
    methods = [node for node in definition.body if isinstance(node, ast.FunctionDef)
               and node.name in names]
    assert len(methods) == 2 and {node.name for node in methods} == names
    definition.body = methods
    future = ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0)
    module = ast.Module(body=[future, definition], type_ignores=[])
    namespace = {}
    exec(compile(ast.fix_missing_locations(module), str(path), "exec"), namespace)
    return namespace["SyncStorage"]._validated_bootstrap_plan


def decode_case(decoder, case):
    value = json.loads(case["native"])
    local = case["input"].get("localHistory", [])
    remote = case["input"].get("remoteHistory", [])
    try:
        result = {"returned": decoder(value, local, remote)}
    except ValueError as error:
        result = {"error": str(error)}
    return {"name": case["name"], "value": value, "desktop": result}


class BootstrapDesktopSourceParity(unittest.TestCase):
    bridge = ROOT / "target/debug/examples/bootstrap_plan_probe"
    source = ROOT.parent / "desktop/src/pomodorough/storage_sync.py"

    @classmethod
    def setUpClass(cls):
        cls.cases = json.loads((ROOT / "fixtures/bootstrap-plan-v1-desktop.json").read_text())["cases"]
        run = subprocess.run([str(cls.bridge)], check=True, text=True, capture_output=True,
                             input="".join(json.dumps(case["input"]) + "\n" for case in cls.cases))
        cls.native = [json.loads(line) for line in run.stdout.splitlines()]
        assert len(cls.native) == len(cls.cases), "Native bridge must return every fixture row"
        cls.decoder = staticmethod(desktop_decoder(cls.source))

    def assert_case(self, case, native):
        self.assertEqual(json.loads(native["raw"]), native["decoded"], "Native decode equality")
        actual = decode_case(self.decoder, {**case, "native": native["raw"]})
        self.assertEqual(actual["desktop"], case["desktop"],
                         f'{case["name"]} actual Desktop decoder result for native {native["raw"]}')
        self.assertEqual(actual["value"], case["expected"], "Complete native output")

    def assert_named_case(self, name):
        rows = [(case, native) for case, native in zip(self.cases, self.native) if case["name"] == name]
        self.assertEqual(len(rows), 1, f"Missing or duplicate fixture case {name}")
        self.assert_case(*rows[0])

    def test_all_branches_match_actual_desktop_decoder(self):
        self.assertEqual(len(self.cases), 22)
        for case, native in zip(self.cases, self.native):
            with self.subTest(case=case["name"]):
                self.assert_case(case, native)

    def test_both_state_without_completed_history_decodes_as_merge(self):
        self.assert_named_case("both_state_only")

    def test_remote_state_without_completed_history_decodes_as_keep_remote(self):
        self.assert_named_case("remote_state_only")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", type=Path, default=BootstrapDesktopSourceParity.bridge)
    parser.add_argument("--desktop-source", type=Path, default=BootstrapDesktopSourceParity.source)
    args = parser.parse_args()
    BootstrapDesktopSourceParity.bridge = args.native.resolve()
    BootstrapDesktopSourceParity.source = args.desktop_source.resolve()
    unittest.main(argv=[sys.argv[0]], verbosity=2)


if __name__ == "__main__":
    main()
