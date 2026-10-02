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
    "pomodorough_free_v2": 0,
}
I32 = 0x7F
I64 = 0x7E
EXPECTED_FUNC_SIGS = {
    "pomodorough_alloc": ((I32,), (I32,)),
    "pomodorough_dispatch": ((I32, I32, I32, I32), (I64,)),
    "pomodorough_free": ((I32, I32), ()),
    "pomodorough_free_v2": ((I32, I32), (I32,)),
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


def read_limits(payload: bytes, offset: int) -> int:
    flags, offset = read_u32(payload, offset)
    _, offset = read_u32(payload, offset)
    if flags & 1:
        _, offset = read_u32(payload, offset)
    return offset


def parse_func_types(payload: bytes) -> list[tuple[tuple[int, ...], tuple[int, ...]]]:
    count, offset = read_u32(payload, 0)
    types: list[tuple[tuple[int, ...], tuple[int, ...]]] = []
    for _ in range(count):
        if offset >= len(payload):
            raise ContractError("truncated function type")
        if payload[offset] != 0x60:
            raise ContractError("expected function type marker 0x60")
        offset += 1
        params: list[int] = []
        results: list[int] = []
        param_count, offset = read_u32(payload, offset)
        for _ in range(param_count):
            if offset >= len(payload):
                raise ContractError("truncated function type parameters")
            params.append(payload[offset])
            offset += 1
        result_count, offset = read_u32(payload, offset)
        for _ in range(result_count):
            if offset >= len(payload):
                raise ContractError("truncated function type results")
            results.append(payload[offset])
            offset += 1
        types.append((tuple(params), tuple(results)))
    if offset != len(payload):
        raise ContractError("trailing type-section data")
    return types


def count_imported_funcs(payload: bytes) -> int:
    count, offset = read_u32(payload, 0)
    imported = 0
    for _ in range(count):
        _, offset = read_name(payload, offset)
        _, offset = read_name(payload, offset)
        if offset >= len(payload):
            raise ContractError("truncated import description")
        kind = payload[offset]
        offset += 1
        if kind == 0:
            _, offset = read_u32(payload, offset)
            imported += 1
        elif kind == 1:
            if offset >= len(payload):
                raise ContractError("truncated table import")
            offset += 1
            offset = read_limits(payload, offset)
        elif kind == 2:
            offset = read_limits(payload, offset)
        elif kind == 3:
            if offset + 2 > len(payload):
                raise ContractError("truncated global import")
            offset += 2
        else:
            raise ContractError(f"unknown import kind {kind}")
    if offset != len(payload):
        raise ContractError("trailing import-section data")
    return imported


def parse_func_type_indices(payload: bytes) -> list[int]:
    count, offset = read_u32(payload, 0)
    indices: list[int] = []
    for _ in range(count):
        index, offset = read_u32(payload, offset)
        indices.append(index)
    if offset != len(payload):
        raise ContractError("trailing function-section data")
    return indices


def parse_func_export_indices(payload: bytes) -> dict[str, int]:
    count, offset = read_u32(payload, 0)
    exports: dict[str, int] = {}
    for _ in range(count):
        name, offset = read_name(payload, offset)
        if offset >= len(payload):
            raise ContractError("truncated export kind")
        kind = payload[offset]
        offset += 1
        index, offset = read_u32(payload, offset)
        if name in exports:
            raise ContractError(f"duplicate export {name!r}")
        if kind == 0:
            exports[name] = index
    return exports


def validate_func_signatures(parsed: dict[int, bytes]) -> None:
    if 1 not in parsed or 3 not in parsed:
        raise ContractError("artifact is missing type or function section")
    func_types = parse_func_types(parsed[1])
    imported_funcs = count_imported_funcs(parsed[2]) if 2 in parsed else 0
    type_indices = parse_func_type_indices(parsed[3])
    func_exports = parse_func_export_indices(parsed[7])
    for name, (params, results) in EXPECTED_FUNC_SIGS.items():
        if name not in func_exports:
            raise ContractError(f"missing function export {name!r}")
        func_index = func_exports[name]
        if func_index < imported_funcs:
            raise ContractError(f"export {name!r} resolves to an import, not a defined function")
        defined = func_index - imported_funcs
        if defined >= len(type_indices):
            raise ContractError(f"export {name!r} function index out of range")
        type_index = type_indices[defined]
        if type_index >= len(func_types):
            raise ContractError(f"export {name!r} type index out of range")
        actual = func_types[type_index]
        if actual != (params, results):
            raise ContractError(
                f"export {name!r} has incompatible signature "
                f"params={list(actual[0])} results={list(actual[1])}"
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
    validate_func_signatures(parsed)
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
