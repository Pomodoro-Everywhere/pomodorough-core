#!/usr/bin/env python3
"""Canonicalize a WebAssembly module by removing non-semantic custom sections."""

from __future__ import annotations

import argparse
from pathlib import Path

WASM_HEADER = b"\x00asm\x01\x00\x00\x00"


def _read_uleb32(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    for shift in range(0, 35, 7):
        if offset >= len(data):
            raise ValueError("truncated WASM section length")
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            if value > 0xFFFFFFFF:
                raise ValueError("WASM section length exceeds u32")
            return value, offset
    raise ValueError("invalid WASM section length")


def _validate_custom_section(payload: bytes) -> None:
    try:
        name_size, name_start = _read_uleb32(payload, 0)
    except ValueError as error:
        raise ValueError("invalid WASM custom section name length") from error
    name_end = name_start + name_size
    if name_end > len(payload):
        raise ValueError("invalid WASM custom section name: truncated UTF-8 bytes")
    try:
        payload[name_start:name_end].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("invalid WASM custom section name: invalid UTF-8") from error


def canonicalize_wasm(data: bytes) -> bytes:
    if not data.startswith(WASM_HEADER):
        raise ValueError("invalid WebAssembly header or version")

    output = bytearray(WASM_HEADER)
    offset = len(WASM_HEADER)
    while offset < len(data):
        section_start = offset
        section_id = data[offset]
        offset += 1
        section_size, payload_start = _read_uleb32(data, offset)
        section_end = payload_start + section_size
        if section_end > len(data):
            raise ValueError("truncated WASM section")
        if section_id == 0:
            _validate_custom_section(data[payload_start:section_end])
        else:
            output.extend(data[section_start:section_end])
        offset = section_end
    return bytes(output)


def canonicalize_file(path: Path) -> None:
    canonical = canonicalize_wasm(path.read_bytes())
    path.write_bytes(canonical)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", type=Path)
    args = parser.parse_args()
    canonicalize_file(args.artifact)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
