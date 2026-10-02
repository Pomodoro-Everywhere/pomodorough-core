"""Regression for raw JSON transport through the shared native probe bridge."""
import json
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


class TrustedClockProbeTransportTests(unittest.TestCase):
    def test_exact_fractional_state_survives_bridge(self):
        fixture = json.loads((ROOT / "fixtures/clock-observe-checker-v1.json").read_text())
        case = next(case for case in fixture["cases"] if case["group"] == "transport")
        payload = '{"operation":"clock.observe.v1","input":' + case["request"] + '}\n'
        process = subprocess.run([str(ROOT / "target/debug/examples/read_model_probe")],
            input=payload, text=True, capture_output=True, check=True)
        actual = json.loads(process.stdout)
        self.assertEqual(actual["state"]["anchorUptime"], case["expected"]["state"]["anchorUptime"])


if __name__ == "__main__":
    unittest.main()
