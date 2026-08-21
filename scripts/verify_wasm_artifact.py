#!/usr/bin/env python3
"""Validate the portable structural contract of a Pomodorough WebAssembly artifact."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path


EXPECTED_EXPORTS = {
    "memory": 2,
    "pomodorough_alloc": 0,
    "pomodorough_dispatch": 0,
    "pomodorough_free": 0,
}
MAX_ARTIFACT_BYTES = 16 * 1024 * 1024
MAX_MEMORY_PAGES = 4096


class ContractError(ValueError):
    pass


def read_u32(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    shift = 0
    for _ in range(5):
        if offset >= len(data):
            raise ContractError("truncated unsigned LEB128")
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte & 0x80 == 0:
            return value, offset
        shift += 7
    raise ContractError("oversized unsigned LEB128")


def read_name(data: bytes, offset: int) -> tuple[str, int]:
    length, offset = read_u32(data, offset)
    end = offset + length
    if end > len(data):
        raise ContractError("truncated WebAssembly name")
    try:
        return data[offset:end].decode("utf-8"), end
    except UnicodeDecodeError as error:
        raise ContractError("invalid UTF-8 WebAssembly name") from error


def sections(data: bytes) -> dict[int, bytes]:
    if data[:8] != b"\0asm\x01\0\0\0":
        raise ContractError("invalid WebAssembly magic or version")
    result: dict[int, bytes] = {}
    offset = 8
    last_standard = 0
    while offset < len(data):
        section_id = data[offset]
        offset += 1
        length, offset = read_u32(data, offset)
        end = offset + length
        if end > len(data):
            raise ContractError("truncated WebAssembly section")
        if section_id != 0:
            if section_id in result:
                raise ContractError(f"duplicate WebAssembly section {section_id}")
            if section_id < last_standard:
                raise ContractError("out-of-order WebAssembly sections")
            last_standard = section_id
            result[section_id] = data[offset:end]
        offset = end
    return result


def validate_memory(payload: bytes) -> None:
    count, offset = read_u32(payload, 0)
    if count != 1:
        raise ContractError(f"expected one linear memory, found {count}")
    flags, offset = read_u32(payload, offset)
    if flags != 1:
        raise ContractError("linear memory must be 32-bit, unshared, and declare a maximum")
    minimum, offset = read_u32(payload, offset)
    maximum, offset = read_u32(payload, offset)
    if offset != len(payload):
        raise ContractError("trailing memory-section data")
    if minimum > maximum or maximum != MAX_MEMORY_PAGES:
        raise ContractError(
            f"expected memory maximum {MAX_MEMORY_PAGES} pages, got {minimum}..{maximum}"
        )


def validate_exports(payload: bytes) -> None:
    count, offset = read_u32(payload, 0)
    exports: dict[str, int] = {}
    for _ in range(count):
        name, offset = read_name(payload, offset)
        if offset >= len(payload):
            raise ContractError("truncated export kind")
        kind = payload[offset]
        offset += 1
        _, offset = read_u32(payload, offset)
        if name in exports:
            raise ContractError(f"duplicate export {name!r}")
        exports[name] = kind
    if offset != len(payload):
        raise ContractError("trailing export-section data")
    for name, kind in EXPECTED_EXPORTS.items():
        if exports.get(name) != kind:
            raise ContractError(f"missing or incorrectly typed export {name!r}")


def validate(path: Path, expected_sha256: str | None = None) -> str:
    data = path.read_bytes()
    if not data or len(data) > MAX_ARTIFACT_BYTES:
        raise ContractError(f"artifact size {len(data)} is outside 1..{MAX_ARTIFACT_BYTES}")
    digest = hashlib.sha256(data).hexdigest()
    if expected_sha256 is not None and digest != expected_sha256:
        raise ContractError(f"SHA-256 mismatch: expected {expected_sha256}, got {digest}")
    parsed = sections(data)
    if 5 not in parsed or 7 not in parsed:
        raise ContractError("artifact is missing memory or export section")
    validate_memory(parsed[5])
    validate_exports(parsed[7])
    return digest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifact", type=Path)
    parser.add_argument("--sha256")
    args = parser.parse_args()
    try:
        digest = validate(args.artifact, args.sha256)
    except (OSError, ContractError) as error:
        parser.error(str(error))
    print(f"{digest}  {args.artifact}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
