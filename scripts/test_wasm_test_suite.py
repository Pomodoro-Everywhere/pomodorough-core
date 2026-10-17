"""Check fail-closed discovery and execute the exact CI and release gate shells."""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from scripts.test_c5_release_contract import load_contract
from scripts.wasm_test_suite import MANIFEST, ROOT, discover


CANDIDATES = {
    "tests/aggregate_wasm_parity.mjs",
    "tests/c2_round5_wasm_parity.mjs",
    "tests/c3_wasm_parity.mjs",
    "tests/c4_wasm_allocation_contract.mjs",
    "tests/wasm_abi_host.mjs",
}
HISTORICAL = {
    "tests/r49_c01_official_baseline.mjs",
    "tests/r49_c01_standalone_parity.mjs",
    "tests/r49_c02_official_baseline.mjs",
}
RUNNER = "scripts/wasm_test_suite.py"
ARTIFACT = "target/wasm32-unknown-unknown/release/pomodorough_core.wasm"
INVOCATION = 'python3 scripts/wasm_test_suite.py "$wasm"'
EXPECTED_GATE_SHELL = (
    "set -euo pipefail\n"
    "wasm=target/wasm32-unknown-unknown/release/pomodorough_core.wasm\n"
    'python3 scripts/wasm_test_suite.py "$wasm"\n'
)
OLD_GLOB_SHELL = 'for test in tests/*.mjs; do node "$test" "$wasm"; done'
STUB = """import assert from 'node:assert/strict';
import { appendFileSync, readFileSync, existsSync } from 'node:fs';
import { relative } from 'node:path';
import { fileURLToPath } from 'node:url';
const script = relative(process.cwd(), fileURLToPath(import.meta.url));
assert.equal(process.argv.length, 3);
assert.equal(readFileSync(0, 'utf8'), '');
assert.equal(readFileSync(process.argv[2], 'utf8'), 'candidate fixture bytes');
appendFileSync('calls.jsonl', JSON.stringify({ script, args: process.argv.slice(2) }) + '\\n');
if (existsSync('fail.txt') && readFileSync('fail.txt', 'utf8') === script) process.exit(7);
"""


def gate_script(workflow: str, job: str) -> str:
    contract = load_contract()
    contents = (ROOT / f".github/workflows/{workflow}.yml").read_text(encoding="utf-8")
    steps = contract._workflow_jobs(contents)[job].steps
    return next(step.run for step in steps if step.name == "Exercise exact canonical WASM")


class WasmTestSuiteTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.suite = discover(ROOT)
        (self.root / "scripts").mkdir()
        shutil.copyfile(ROOT / RUNNER, self.root / RUNNER)
        for category, scripts in self.suite.items():
            for script in scripts:
                path = self.root / script
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(STUB if category == "candidate" else "throw Error('not a candidate gate');\n",
                                encoding="utf-8")
        self.write_manifest(self.suite)
        artifact = self.root / ARTIFACT
        artifact.parent.mkdir(parents=True)
        artifact.write_text("candidate fixture bytes", encoding="utf-8")

    def write_manifest(self, suite: object) -> None:
        (self.root / MANIFEST).write_text(json.dumps(suite), encoding="utf-8")

    def run_shell(self, shell: str) -> subprocess.CompletedProcess:
        env = dict(os.environ)
        env.pop("R49_C01_OFFICIAL_WASM", None)
        return subprocess.run(["bash", "-c", shell], cwd=self.root, env=env,
                              input="", capture_output=True, text=True, timeout=30)

    def calls(self) -> list[dict]:
        path = self.root / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_repository_classifies_every_script_and_preserves_required_gates(self) -> None:
        self.assertTrue(CANDIDATES.issubset(self.suite["candidate"]))
        self.assertEqual(set(self.suite["historical"]), HISTORICAL)
        for wrapper, script in (("tests/wasm_contract.rs", "tests/wasm_abi_host.mjs"),
                                ("tests/c4_wasm_allocation_contract.rs", "tests/c4_wasm_allocation_contract.mjs")):
            self.assertIn(f'.arg("{script}")', (ROOT / wrapper).read_text())
            self.assertIn(script, self.suite["candidate"])

    def test_exact_ci_and_release_shells_execute_all_candidates_only(self) -> None:
        for workflow, job in (("ci", "rust"), ("release", "build-test-attest")):
            with self.subTest(workflow=workflow):
                (self.root / "calls.jsonl").unlink(missing_ok=True)
                result = self.run_shell(gate_script(workflow, job))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.calls(), [
                    {"script": script, "args": [str((self.root / ARTIFACT).resolve())]}
                    for script in self.suite["candidate"]
                ])

    def test_release_fails_before_any_gate_for_missing_or_unclassified_scripts(self) -> None:
        original = self.root / self.suite["candidate"][0]
        contents = original.read_text()
        mutations = (original, self.root / "tests/new_gate.mjs",
                     self.root / "tests/aggregate_artifact/new_gate.mjs")
        for path in mutations:
            with self.subTest(path=path):
                if path == original:
                    path.unlink()
                else:
                    path.write_text(STUB)
                result = self.run_shell(gate_script("release", "build-test-attest"))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("missing scripts:" if path == original else "unclassified scripts:", result.stderr)
                self.assertEqual(self.calls(), [])
                if path == original:
                    path.write_text(contents)
                else:
                    path.unlink()

    def test_missing_historical_and_support_files_also_fail_closed(self) -> None:
        for category in ("historical", "support"):
            path = self.root / self.suite[category][0]
            contents = path.read_text()
            path.unlink()
            result = self.run_shell(gate_script("release", "build-test-attest"))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing scripts:", result.stderr)
            self.assertEqual(self.calls(), [])
            path.write_text(contents)

    def test_invalid_classifications_fail_before_execution(self) -> None:
        invalid = []
        for paths in ([], [None], [""], ["../outside.mjs"], ["/tests/absolute.mjs"],
                      ["tests/../outside.mjs"], ["tests//alias.mjs"], ["tests/not_javascript.py"]):
            suite = copy.deepcopy(self.suite)
            suite["candidate"] = paths
            invalid.append(suite)
        duplicate = copy.deepcopy(self.suite)
        duplicate["historical"].append(duplicate["candidate"][0])
        invalid.extend([duplicate, {}, {**self.suite, "ignored": []}])
        for suite in invalid:
            with self.subTest(suite=suite):
                self.write_manifest(suite)
                result = self.run_shell(gate_script("release", "build-test-attest"))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("WASM test discovery failed:", result.stderr)
                self.assertEqual(self.calls(), [])

    def test_duplicate_json_categories_fail_closed(self) -> None:
        (self.root / MANIFEST).write_text('{"candidate": [], "candidate": []}')
        result = self.run_shell(gate_script("release", "build-test-attest"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate manifest category", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_failure_propagates_and_stops_later_gates(self) -> None:
        (self.root / "fail.txt").write_text(self.suite["candidate"][0])
        result = self.run_shell(gate_script("release", "build-test-attest"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Candidate WASM gate failed:", result.stderr)
        self.assertEqual([call["script"] for call in self.calls()], self.suite["candidate"][:1])

    def test_cli_lists_every_classification_and_rejects_missing_artifact(self) -> None:
        result = self.run_shell(f"python3 {RUNNER} --list")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), self.suite)
        for arguments in ("", "missing.wasm", "--native-only", f"--list {ARTIFACT}"):
            result = self.run_shell(f"python3 {RUNNER} {arguments}")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.calls(), [])

    def test_workflow_contract_rejects_candidate_gate_bypasses(self) -> None:
        contract = load_contract()
        for workflow in ("ci", "release"):
            contents = (ROOT / f".github/workflows/{workflow}.yml").read_text()
            validate = getattr(contract, f"validate_{workflow}_workflow")
            validate(contents)
            for replacement in ("true", INVOCATION + " || true", "exit 0\n          " + INVOCATION,
                                INVOCATION.replace('"$wasm"', "other.wasm"), INVOCATION + " --list"):
                with self.subTest(workflow=workflow, replacement=replacement):
                    with self.assertRaises(contract.ReleaseContractError):
                        validate(contents.replace(INVOCATION, replacement))

    def test_original_historical_scripts_reject_candidate_bytes(self) -> None:
        for script in self.suite["historical"]:
            env = {**os.environ, "R49_C01_OFFICIAL_WASM": str(self.root / ARTIFACT)}
            result = subprocess.run(["node", str(ROOT / script), str(self.root / ARTIFACT)],
                                    env=env, input="", capture_output=True, text=True, timeout=30)
            with self.subTest(script=script):
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("775d3d85cc4f6ced2243fd8b831e0057d6264e2202fdc2bf28d83b62e311e05a", result.stderr)


    def test_exact_gate_shell_matches_release_candidate_command(self) -> None:
        for workflow, job in (("ci", "rust"), ("release", "build-test-attest")):
            with self.subTest(workflow=workflow):
                self.assertEqual(gate_script(workflow, job).splitlines(), EXPECTED_GATE_SHELL.splitlines())
                self.assertIn(INVOCATION, gate_script(workflow, job))

    def test_extra_manifest_entry_fails_before_any_gate(self) -> None:
        suite = copy.deepcopy(self.suite)
        suite["candidate"].append("tests/extra_gate.mjs")
        self.write_manifest(suite)
        result = self.run_shell(gate_script("release", "build-test-attest"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing scripts:", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_old_glob_shape_fails_where_dispatcher_passes(self) -> None:
        gate = gate_script("release", "build-test-attest")
        self.assertEqual(self.run_shell(gate).returncode, 0)
        self.assertEqual(len(self.calls()), len(self.suite["candidate"]))
        (self.root / "calls.jsonl").unlink(missing_ok=True)
        old = f'set -euo pipefail\nwasm={ARTIFACT}\n{OLD_GLOB_SHELL}\n'
        result = self.run_shell(old)
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
