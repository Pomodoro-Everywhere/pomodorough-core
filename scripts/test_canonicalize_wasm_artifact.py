from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("canonicalize_wasm_artifact.py")


def load_module():
    spec = importlib.util.spec_from_file_location("canonicalize_wasm_artifact", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load canonicalizer")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def uleb(value: int) -> bytes:
    encoded = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            encoded.append(byte | 0x80)
        else:
            encoded.append(byte)
            return bytes(encoded)


def section(section_id: int, payload: bytes) -> bytes:
    return bytes([section_id]) + uleb(len(payload)) + payload


HEADER = b"\x00asm\x01\x00\x00\x00"


class CanonicalizeWASMTests(unittest.TestCase):
    def test_removes_custom_sections_and_preserves_standard_sections(self) -> None:
        module = (
            HEADER
            + section(0, uleb(4) + b"name" + b"host-dependent")
            + section(1, b"\x00")
            + section(0, uleb(9) + b"producers" + b"toolchain")
            + section(10, b"\x00")
        )

        canonicalize = load_module().canonicalize_wasm

        self.assertEqual(
            canonicalize(module),
            HEADER + section(1, b"\x00") + section(10, b"\x00"),
        )

    def test_is_idempotent(self) -> None:
        canonicalize = load_module().canonicalize_wasm
        module = HEADER + section(1, b"\x00") + section(10, b"\x00")
        self.assertEqual(canonicalize(canonicalize(module)), module)

    def test_rejects_truncated_section(self) -> None:
        canonicalize = load_module().canonicalize_wasm
        with self.assertRaisesRegex(ValueError, "truncated WASM section"):
            canonicalize(HEADER + b"\x01\x02\x00")

    def test_rejects_malformed_custom_section_names_before_removal(self) -> None:
        canonicalize = load_module().canonicalize_wasm
        malformed_payloads = {
            "missing name length": b"",
            "truncated name": uleb(2) + b"x",
            "invalid UTF-8 name": uleb(1) + b"\xff",
        }
        for label, payload in malformed_payloads.items():
            with self.subTest(label=label), self.assertRaisesRegex(
                ValueError, "invalid WASM custom section name"
            ):
                canonicalize(HEADER + section(0, payload))

        self.assertEqual(canonicalize(HEADER + section(0, uleb(0))), HEADER)

    def test_canonicalizes_file_in_place(self) -> None:
        module = HEADER + section(0, uleb(4) + b"name" + b"variable") + section(1, b"\x00")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "core.wasm"
            path.write_bytes(module)

            load_module().canonicalize_file(path)

            self.assertEqual(path.read_bytes(), HEADER + section(1, b"\x00"))


if __name__ == "__main__":
    unittest.main()
