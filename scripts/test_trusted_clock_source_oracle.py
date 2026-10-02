"""Regressions for faithful raw observations in production-source clock probes."""
import copy
import json
from pathlib import Path
import unittest

import trusted_clock_source_probe as probe

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = json.loads((ROOT / "fixtures/clock-observe-oracle-v1.json").read_text())


def native_request(name):
    case = next(case for case in FIXTURE["cases"] if case["name"] == name)
    return probe.NativeClockRequest(json.loads(case["request"]), case["nativeObservations"])


class TrustedClockSourceOracleTests(unittest.TestCase):
    def test_native_anchor_maps_1001_to_901(self):
        request = native_request("desktop-native-anchor-decodes-1001-not-1000")
        actual = probe.desktop([request], None)[0]
        self.assertEqual(actual["physicalAnchorMs"], 901)
        expected = probe.core(request)
        expected.pop("schemaVersion")
        expected.pop("compatibility")
        self.assertEqual(actual, expected)

    def test_every_raw_native_fixture_matches_complete_core_result(self):
        for case in FIXTURE["cases"]:
            with self.subTest(case=case["name"]):
                request = native_request(case["name"])
                actual = probe.desktop([request], None)[0]
                expected = probe.core(case["request"])
                expected.pop("schemaVersion")
                expected.pop("compatibility")
                self.assertEqual(actual, expected)

    def test_native_timestamp_decoders_have_independent_boundary_expectations(self):
        namespace = probe.desktop_source()
        for case in FIXTURE["timestampDecodes"]:
            with self.subTest(raw=case["raw"]):
                self.assertEqual(namespace["parse_timestamp_ms"](case["raw"]), case["milliseconds"])

    def test_native_wall_float_decoders_have_independent_boundary_expectations(self):
        namespace = probe.desktop_source()
        for case in FIXTURE["wallDecodes"]:
            with self.subTest(raw=case["raw"]):
                native = probe.NativeClockRequest(
                    {"reading":{"wallMs":case["milliseconds"],"monotonicMs":20000}},
                    {"wallSeconds":case["raw"],"monotonicNs":20000000000})
                observed = probe.desktop_native_observations(native, namespace)
                self.assertEqual(int(observed["wallSeconds"] * 1000), case["milliseconds"])

    def test_bad_anchor_pair_is_oracle_failure_not_native_policy_rejection(self):
        request = native_request("desktop-native-anchor-decodes-1001-not-1000")
        request.native_observations["trustedAnchorAt"] = "1970-01-01T00:00:01.001000Z"
        with self.assertRaisesRegex(AssertionError, "native anchor decode mismatch"):
            probe.desktop([request], None)

    def test_bad_wall_pair_is_oracle_failure_not_restore_clear(self):
        request = native_request("desktop-native-restore-drift-is-exactly-1000")
        request.native_observations["wallSeconds"] = "1.001"
        with self.assertRaisesRegex(AssertionError, "native wall decode mismatch"):
            probe.desktop([request], None)

    def test_bad_monotonic_pair_is_oracle_failure(self):
        request = native_request("desktop-native-restore-drift-is-exactly-1000")
        request.native_observations["monotonicNs"] = 19999999999
        with self.assertRaisesRegex(AssertionError, "native monotonic decode mismatch"):
            probe.desktop([request], None)

    def test_missing_raw_observations_never_use_inverse_reconstruction(self):
        request = native_request("desktop-native-restore-drift-is-exactly-1000")
        with self.assertRaisesRegex(AssertionError, "requires raw native observations"):
            probe.desktop([dict(request)], None)
        request = native_request("desktop-native-anchor-decodes-1001-not-1000")
        request.native_observations.pop("trustedAnchorAt")
        with self.assertRaisesRegex(AssertionError, "Missing raw native trusted anchor"):
            probe.desktop([request], None)

    def test_raw_sidecars_never_enter_core_request_or_replace_given_values(self):
        for case in FIXTURE["cases"]:
            request = native_request(case["name"])
            before = copy.deepcopy(dict(request))
            self.assertEqual(json.loads(json.dumps(request)), before)
            self.assertNotIn("nativeObservations", request)
            probe.desktop([request], None)
            self.assertEqual(dict(request), before)

    def test_native_wall_keeps_restore_at_exact_drift_bound(self):
        request = native_request("desktop-native-restore-drift-is-exactly-1000")
        actual = probe.desktop([request], None)[0]
        self.assertEqual(actual["state"]["sample"], request["state"]["sample"])
        self.assertEqual(actual["state"]["anchor"], request["state"]["sample"])
        expected = probe.core(request)
        expected.pop("schemaVersion")
        expected.pop("compatibility")
        self.assertEqual(actual, expected)


if __name__ == "__main__":
    unittest.main()
