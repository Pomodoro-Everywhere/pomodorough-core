"""Protect official aggregate parity wiring without building or executing WASM."""
from pathlib import Path
import unittest

from scripts.test_c5_release_contract import load_contract


ROOT = Path(__file__).resolve().parents[1]


class AggregateArtifactWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_contract()
        self.release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")

    def test_release_discovers_aggregate_and_all_existing_hosts_before_sealing(self) -> None:
        self.contract.validate_release_workflow(self.release)
        hosts = {path.name for path in (ROOT / "tests").glob("*.mjs")}
        self.assertTrue({"aggregate_wasm_parity.mjs", "c2_round5_wasm_parity.mjs",
                         "c3_wasm_parity.mjs", "c4_wasm_allocation_contract.mjs",
                         "wasm_abi_host.mjs"}.issubset(hosts))
        self.assertLess(self.release.index("for test in tests/*.mjs; do"),
                        self.release.index("name: Seal exact tested release candidate"))

    def test_release_cannot_substitute_native_only_or_ignore_dispatch_failure(self) -> None:
        for replacement in ('node "$test" --native-only', 'node "$test" "$wasm" || true'):
            with self.subTest(replacement=replacement), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(
                    self.release.replace('node "$test" "$wasm"', replacement, 1))

    def test_release_cannot_substitute_artifact_path(self) -> None:
        original = 'wasm=target/wasm32-unknown-unknown/release/pomodorough_core.wasm'
        before, gate = self.release.split("      - name: Exercise exact canonical WASM", 1)
        mutated = before + "      - name: Exercise exact canonical WASM" + gate.replace(
            original, "wasm=other.wasm", 1)
        with self.assertRaises(self.contract.ReleaseContractError):
            self.contract.validate_release_workflow(mutated)

    def test_release_requires_static_gate_checks(self) -> None:
        for command in ("node --experimental-vm-modules --test scripts/test_aggregate_artifact_gate.mjs",
                        "python3 -m unittest scripts/test_aggregate_artifact_workflow.py -v"):
            with self.subTest(command=command), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(self.release.replace(command, "true", 1))

    def test_host_shell_requires_complete_sequence_without_early_bypass(self) -> None:
        marker = "      - name: Exercise exact canonical WASM"
        before, suffix = self.release.split(marker, 1)
        gate, after = suffix.split("      - name: Seal exact tested release candidate", 1)
        mutations = (
            gate.replace('            node "$test" "$wasm"', '            continue\n            node "$test" "$wasm"'),
            gate.replace('            node "$test" "$wasm"', '            break\n            node "$test" "$wasm"'),
            gate.replace("          for test", "          exit 0\n          for test"),
            gate.replace("          for test", "          return 0\n          for test"),
            gate.replace("          done", "          done\n          exit 0"),
            gate.replace("          for test", '          wasm="other.wasm"\n          for test'),
        )
        for index, mutation in enumerate(mutations):
            with self.subTest(mutant=index), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(before + marker + mutation
                    + "      - name: Seal exact tested release candidate" + after)


if __name__ == "__main__":
    unittest.main()
