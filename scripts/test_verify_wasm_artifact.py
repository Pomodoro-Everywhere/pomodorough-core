#!/usr/bin/env python3
"""Negative coverage for the WASM ABI signature checks in verify_wasm_artifact."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = Path(__file__).with_name("verify_wasm_artifact.py")


def load_verifier():
    spec = importlib.util.spec_from_file_location("verify_wasm_artifact", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load WASM artifact verifier")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


verifier = load_verifier()
ContractError = verifier.ContractError
validate = verifier.validate


def section(section_id: int, payload: bytes) -> bytes:
    return bytes([section_id, len(payload)]) + payload


def func_type(params: bytes, results: bytes) -> bytes:
    return b"\x60" + bytes([len(params)]) + params + bytes([len(results)]) + results


NAMES = (
    b"pomodorough_alloc",
    b"pomodorough_dispatch",
    b"pomodorough_free",
    b"pomodorough_free_v2",
)


def artifact_with_sigs(sigs: list[bytes]) -> bytes:
    type_section = bytes([len(sigs)]) + b"".join(sigs)
    func_section = bytes([len(sigs)]) + bytes(range(len(sigs)))
    exports = b"\x05\x06memory\x02\x00" + b"".join(
        bytes([len(name)]) + name + b"\x00" + bytes([index])
        for index, name in enumerate(NAMES)
    )
    return (
        b"\0asm\x01\0\0\0"
        + section(1, type_section)
        + section(3, func_section)
        + section(5, b"\x01\x01\x01\x80\x20")
        + section(7, exports)
        + section(10, b"\x01\x02\x00\x0b")
    )


class StubPath:
    def __init__(self, data: bytes) -> None:
        self._data = data

    def read_bytes(self) -> bytes:
        return self._data


I32 = b"\x7f"
I64 = b"\x7e"
CORRECT_SIGS = [
    func_type(I32, I32),
    func_type(I32 * 4, I64),
    func_type(I32 * 2, b""),
    func_type(I32 * 2, I32),
]


class VerifyWasmArtifactTests(unittest.TestCase):
    def test_rejects_uniform_empty_signatures(self) -> None:
        wasm = artifact_with_sigs([func_type(b"", b"")] * 4)
        with self.assertRaisesRegex(ContractError, "incompatible signature"):
            validate(StubPath(wasm))  # type: ignore[arg-type]

    def test_rejects_single_swapped_result(self) -> None:
        sigs = list(CORRECT_SIGS)
        sigs[3] = func_type(I32 * 2, b"")
        with self.assertRaisesRegex(ContractError, "pomodorough_free_v2"):
            validate(StubPath(artifact_with_sigs(sigs)))  # type: ignore[arg-type]

    def test_accepts_exact_release_artifact(self) -> None:
        wasm = ROOT / "target" / "wasm32-unknown-unknown" / "release" / "pomodorough_core.wasm"
        self.assertTrue(wasm.is_file(), f"missing release artifact at {wasm}")
        digest = validate(wasm)
        self.assertRegex(digest, r"^[0-9a-f]{64}$")


if __name__ == "__main__":
    raise SystemExit(unittest.main())
