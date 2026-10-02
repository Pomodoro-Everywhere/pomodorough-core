#!/usr/bin/env python3
"""Capture reproducible source/native red-green receipts without building WASM."""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
TOOLCHAIN = Path.home() / ".rustup/toolchains/1.97.1-aarch64-apple-darwin/bin"


def run(name, command, environment, directory, succeeds=True):
    result = subprocess.run(command, cwd=ROOT, env=environment, text=True, capture_output=True)
    (directory / (name + ".log")).write_text(result.stdout + result.stderr)
    assert (result.returncode == 0) == succeeds, (name, result.returncode, result.stderr)
    print(f"{name}: {'passed' if succeeds else 'expected baseline failure'}")


def dispatch(binary, operation, value):
    result = subprocess.run([str(binary)], input=json.dumps({"operation": operation, "input": value}) + "\n",
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)


def baseline_receipts(directory, baseline, fixed):
    comparisons = []
    for profile in ("desktop", "pwa", "android", "apple"):
        receipts = json.loads((directory / (profile + "-green.json")).read_text())
        for receipt in receipts:
            value = receipt.get("coreRequest", receipt.get("rawRequest", receipt.get("input")))
            if profile == "pwa": value = receipt["input"]
            operation = "workspace.completionMutation.v1" if "stage" in value else "workspace.intent.v1"
            red = dispatch(baseline, operation, value)
            green = dispatch(fixed, operation, value)
            assert green == receipt["core"], (profile, "complete captured Core result differs")
            assert green["outcome"] == "planned"
            comparisons.append({"profile": profile, "operation": operation, "request": value,
                "baseline": red, "fixed": green, "baselineRejected": "error" in red})
    assert sum(row["baselineRejected"] for row in comparisons) >= 30
    (directory / "complete-red-green.json").write_text(json.dumps(comparisons, indent=2) + "\n")
    print(f'{len(comparisons)} complete native output receipts; {sum(row["baselineRejected"] for row in comparisons)} baseline refusals')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, default=ROOT / "target/durable-admission-baseline/debug/examples/completion_policy_probe")
    parser.add_argument("--output", type=Path, default=ROOT / "target/durable-admission-evidence")
    args = parser.parse_args()
    args.output.mkdir(exist_ok=True)
    env = dict(os.environ, PATH=str(TOOLCHAIN) + os.pathsep + os.environ["PATH"],
               RUSTC=str(TOOLCHAIN / "rustc"), RUSTDOC=str(TOOLCHAIN / "rustdoc"), RUST_BACKTRACE="0")
    run("native-green", [str(TOOLCHAIN / "cargo"), "test", "--all-targets", "--all-features", "--locked", "--",
        "--skip", "wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host", "--skip", "c4_release_wasm_rejects_oversized_allocations_without_trapping"], env, args.output)
    run("format-green", [str(TOOLCHAIN / "cargo"), "fmt", "--all", "--", "--check"], env, args.output)
    run("clippy-green", [str(TOOLCHAIN / "cargo"), "clippy", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"], env, args.output)
    run("bridge-build", [str(TOOLCHAIN / "cargo"), "build", "--locked", "--example", "completion_policy_probe"], env, args.output)
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; (\d+) filtered out", (args.output / "native-green.log").read_text())
    assert sum(int(filtered) for _, filtered in counts) == 2
    print(f'{sum(int(passed) for passed, _ in counts)} native tests passed; exactly two tests filtered')
    fixed = ROOT / "target/debug/examples/completion_policy_probe"
    probes = {"desktop": [str(ROOT.parent / "desktop/.venv/bin/python"), "scripts/durable_admission_desktop_probe.py", "--evidence", str(args.output / "desktop-green.json")],
        "pwa": ["node", "scripts/durable_admission_pwa_probe.cjs"], "android": [sys.executable, "scripts/durable_admission_android_probe.py"],
        "apple": [sys.executable, "scripts/durable_admission_apple_probe.py"]}
    for profile, command in probes.items():
        run(profile + "-green", command, env | {"CORE_PROBE": str(fixed), "PROBE_EVIDENCE": str(args.output / (profile + "-green.json"))}, args.output)
    run("desktop-source-red", [*probes["desktop"][:2], "--case", "selectTask"], env | {"CORE_PROBE": str(args.baseline)}, args.output, False)
    run("pwa-source-red", probes["pwa"], env | {"CORE_PROBE": str(args.baseline), "PROBE_CASE": "upsertTask"}, args.output, False)
    baseline_receipts(args.output, args.baseline, fixed)


if __name__ == "__main__": main()
