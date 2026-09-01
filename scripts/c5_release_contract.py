#!/usr/bin/env python3
"""Enforce fail-closed source and artifact rules for Core releases."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib
from typing import NamedTuple, Sequence


ARTIFACT_NAME = "pomodorough_core.wasm"
MANIFEST_NAME = "SHA256SUMS"
RELEASE_WORKFLOW = ".github/workflows/release.yml"
STRICT_TAG = re.compile(
    r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$"
)
STRICT_SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT_SHA = re.compile(r"^[0-9a-f]{40}$")
ACTION_USE = re.compile(r"^\s*uses:\s*([^\s@]+)@([^\s#]+)", re.MULTILINE)
RELEASE_TIME = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
DRAFT_BODY_PREFIX = "pomodorough-core-release-seal-v1:"
PUBLICATION_RESIDUAL = (
    "GitHub REST has no conditional release PATCH; an external writer can move the "
    "tag during the final request. The contract rechecks the tag immediately before "
    "and after publication, then requires both the PATCH response and final ID-bound "
    "release record to report immutable=true. An already-public release may require "
    "incident response."
)
CI_REQUIRED = (
    "push:\n    branches: [main]",
    "pull_request:",
    "workflow_dispatch:",
    "permissions:\n  contents: read",
    "cargo +1.97.1 fmt --all -- --check",
    "cargo +1.97.1 test --all-targets --locked",
    "cargo +1.97.1 clippy --all-targets --locked -- -D warnings",
    "python3 -m unittest scripts/test_canonicalize_wasm_artifact.py -v",
    "python3 -m unittest scripts/test_c5_release_contract.py -v",
    "cargo +1.97.1 build --release --target wasm32-unknown-unknown --locked",
    "python3 scripts/canonicalize_wasm_artifact.py",
    "python3 scripts/verify_wasm_artifact.py",
    "actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",
)
RELEASE_REQUIRED = (
    'tags:\n      - "v*"',
    "cancel-in-progress: false",
    "ref: ${{ github.workflow_sha }}",
    "fetch-depth: 0",
    "fetch-tags: true",
    "validate-source",
    "cargo +1.97.1 test --all-targets --locked",
    "cargo +1.97.1 clippy --all-targets --all-features --locked -- -D warnings",
    'node "$test" "$wasm"',
    "dist/pomodorough_core.wasm\n            dist/SHA256SUMS",
    "actions/attest-build-provenance@e8998f949152b193b063cb0ec769d69d929409be",
    '--signer-workflow "$GITHUB_REPOSITORY/.github/workflows/release.yml"',
    '--signer-digest "$GITHUB_WORKFLOW_SHA"',
    '--source-digest "$GITHUB_WORKFLOW_SHA"',
    '--source-ref "$GITHUB_REF"',
    "require-remote-tag-source",
    "prepare-draft-release",
    "capture-draft-release",
    "publish-verified-draft",
    "gh release create",
    "--verify-tag",
    "            --draft \\",
    '--notes-file "$body"',
    '--seal "$RUNNER_TEMP/release-seal.json"',
    '--download-directory "$download"',
)
RELEASE_FORBIDDEN = (
    "workflow_dispatch:",
    "cancel-in-progress: true",
    "gh release upload",
    "gh release delete",
    "gh release download",
    "gh release edit",
    "--clobber",
    "immutable-releases",
)
RELEASE_COUNTS = {
    "validate-source": 2,
    'gh attestation verify "$asset"': 2,
    "require-remote-tag-source": 2,
    "prepare-draft-release": 1,
    "capture-draft-release": 1,
    "publish-verified-draft": 1,
    "gh release create": 1,
}
RELEASE_ORDER = (
    "cargo +1.97.1 test --all-targets --locked",
    "cargo +1.97.1 build --release --target wasm32-unknown-unknown --locked",
    "python3 scripts/canonicalize_wasm_artifact.py",
    'node "$test" "$wasm"',
    "create-manifest",
    "actions/attest-build-provenance@",
    "actions/upload-artifact@",
    "      - name: Download exact tested release candidate",
    "      - name: Verify transferred candidate and provenance",
    "      - name: Create or adopt exact sealed draft release",
    "      - name: Verify ID-bound draft assets",
    "      - name: Publish ID-bound verified draft",
    "publish-verified-draft",
)
CI_HEADER = (
    "name: CI",
    "on:",
    "  push:",
    "    branches: [main]",
    "  pull_request:",
    "  workflow_dispatch:",
    "permissions:",
    "  contents: read",
)
RELEASE_HEADER = (
    "name: Release",
    "on:",
    "  push:",
    "    tags:",
    '      - "v*"',
    "permissions:",
    "  contents: read",
    "concurrency:",
    "  group: core-release-${{ github.ref }}",
    "  cancel-in-progress: false",
)
JOB_FIELDS = {
    "rust": {
        "runs-on": "ubuntu-24.04",
        "timeout-minutes": "20",
        "steps": "",
    },
    "build-test-attest": {
        "name": "Build, test, and attest canonical WASM",
        "runs-on": "ubuntu-24.04",
        "timeout-minutes": "30",
        "permissions": "",
        "steps": "",
    },
    "verify-publish": {
        "name": "Verify draft and publish GitHub release",
        "needs": "build-test-attest",
        "runs-on": "ubuntu-24.04",
        "timeout-minutes": "15",
        "permissions": "",
        "steps": "",
    },
}
JOB_PERMISSIONS = {
    "rust": {},
    "build-test-attest": {
        "contents": "read",
        "id-token": "write",
        "attestations": "write",
    },
    "verify-publish": {
        "contents": "write",
        "attestations": "read",
    },
}


class ReleaseContractError(ValueError):
    pass


class WorkflowStep(NamedTuple):
    name: str
    condition: str | None
    run: str | None
    uses: str | None
    contents: str


class WorkflowJob(NamedTuple):
    name: str
    condition: str | None
    contents: str
    steps: tuple[WorkflowStep, ...]


CI_STEPS = (
    (
        "Check out repository",
        "uses",
        ("actions/checkout@11d5960a326750d5838078e36cf38b85af677262",),
    ),
    (
        "Install pinned Rust toolchain",
        "run",
        ("rustup toolchain install 1.97.1", "rustup target add wasm32-unknown-unknown"),
    ),
    ("Check formatting", "run", ("cargo +1.97.1 fmt --all -- --check",)),
    ("Test all targets", "run", ("cargo +1.97.1 test --all-targets --locked",)),
    (
        "Lint all targets",
        "run",
        ("cargo +1.97.1 clippy --all-targets --locked -- -D warnings",),
    ),
    (
        "Test WASM canonicalizer",
        "run",
        ("python3 -m unittest scripts/test_canonicalize_wasm_artifact.py -v",),
    ),
    (
        "Test C5 release contract",
        "run",
        ("python3 -m unittest scripts/test_c5_release_contract.py -v",),
    ),
    (
        "Build portable WebAssembly core",
        "run",
        (
            "cargo +1.97.1 build --release --target wasm32-unknown-unknown --locked",
            "python3 scripts/canonicalize_wasm_artifact.py",
            "python3 scripts/verify_wasm_artifact.py",
        ),
    ),
    (
        "Upload canonical WebAssembly core",
        "uses",
        ("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",),
    ),
)
RELEASE_STEPS = {
    "build-test-attest": (
        (
            "Check out exact workflow source",
            "uses",
            ("actions/checkout@11d5960a326750d5838078e36cf38b85af677262",),
        ),
        (
            "Validate tag-bound source and workflow contract",
            "run",
            (
                "python3 scripts/c5_release_contract.py validate-source",
                "python3 scripts/c5_release_contract.py check-workflows",
            ),
        ),
        (
            "Install pinned Rust toolchain",
            "run",
            (
                "rustup toolchain install 1.97.1",
                "rustup target add wasm32-unknown-unknown",
            ),
        ),
        (
            "Set up pinned Node.js",
            "uses",
            ("actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020",),
        ),
        ("Check formatting", "run", ("cargo +1.97.1 fmt --all -- --check",)),
        (
            "Test all Rust targets",
            "run",
            ("cargo +1.97.1 test --all-targets --locked",),
        ),
        (
            "Lint all Rust targets",
            "run",
            ("cargo +1.97.1 clippy --all-targets --all-features --locked -- -D warnings",),
        ),
        (
            "Test artifact and release tooling",
            "run",
            (
                "python3 -m unittest scripts/test_canonicalize_wasm_artifact.py -v",
                "python3 -m unittest scripts/test_c5_release_contract.py -v",
            ),
        ),
        (
            "Build and canonicalize release WASM",
            "run",
            (
                "cargo +1.97.1 build --release --target wasm32-unknown-unknown --locked",
                "python3 scripts/canonicalize_wasm_artifact.py",
                "python3 scripts/verify_wasm_artifact.py",
            ),
        ),
        ("Exercise exact canonical WASM", "run", ('node "$test" "$wasm"',)),
        (
            "Seal exact tested release candidate",
            "run",
            (
                "python3 scripts/c5_release_contract.py create-manifest",
                "python3 scripts/c5_release_contract.py verify-bundle",
                "(cd dist && sha256sum --check --strict SHA256SUMS)",
            ),
        ),
        (
            "Attest exact WASM and checksum manifest",
            "uses",
            (
                "actions/attest-build-provenance@"
                "e8998f949152b193b063cb0ec769d69d929409be",
            ),
        ),
        (
            "Reverify candidate after attestation",
            "run",
            (
                "python3 scripts/c5_release_contract.py verify-bundle",
                "(cd dist && sha256sum --check --strict SHA256SUMS)",
            ),
        ),
        (
            "Upload immutable release candidate",
            "uses",
            ("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02",),
        ),
    ),
    "verify-publish": (
        (
            "Check out exact workflow source",
            "uses",
            ("actions/checkout@11d5960a326750d5838078e36cf38b85af677262",),
        ),
        (
            "Revalidate tag-bound publication source",
            "run",
            ("python3 scripts/c5_release_contract.py validate-source",),
        ),
        (
            "Download exact tested release candidate",
            "uses",
            ("actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093",),
        ),
        (
            "Verify transferred candidate and provenance",
            "run",
            (
                "python3 scripts/c5_release_contract.py require-remote-tag-source",
                'digest="$(python3 scripts/c5_release_contract.py verify-bundle',
                "python3 scripts/verify_wasm_artifact.py",
                'gh attestation verify "$asset"',
            ),
        ),
        (
            "Create or adopt exact sealed draft release",
            "run",
            (
                'mode="$(python3 scripts/c5_release_contract.py prepare-draft-release',
                'case "$mode" in',
                "create)",
                "python3 scripts/c5_release_contract.py require-remote-tag-source",
                'gh release create "$GITHUB_REF_NAME"',
                "python3 scripts/c5_release_contract.py capture-draft-release",
                "adopt) ;;",
                '*) echo "Unexpected draft mode: $mode" >&2; exit 1 ;;',
                "esac",
            ),
        ),
        (
            "Verify ID-bound draft assets",
            "run",
            (
                'digest="$(python3 scripts/c5_release_contract.py verify-bundle',
                "python3 scripts/verify_wasm_artifact.py",
                'gh attestation verify "$asset"',
            ),
        ),
        (
            "Publish ID-bound verified draft",
            "run",
            ("python3 scripts/c5_release_contract.py publish-verified-draft",),
        ),
    ),
}
STEP_WITH = {
    ("rust", "Check out repository"): {"persist-credentials": "false"},
    ("rust", "Upload canonical WebAssembly core"): {
        "name": "pomodorough-core-wasm-${{ github.sha }}",
        "path": "target/wasm32-unknown-unknown/release/pomodorough_core.wasm",
        "if-no-files-found": "error",
        "retention-days": "7",
    },
    ("build-test-attest", "Check out exact workflow source"): {
        "ref": "${{ github.workflow_sha }}",
        "fetch-depth": "0",
        "fetch-tags": "true",
        "persist-credentials": "false",
    },
    ("build-test-attest", "Set up pinned Node.js"): {"node-version": "22"},
    ("build-test-attest", "Attest exact WASM and checksum manifest"): {
        "subject-path": "dist/pomodorough_core.wasm\ndist/SHA256SUMS",
    },
    ("build-test-attest", "Upload immutable release candidate"): {
        "name": "core-release-${{ github.run_id }}-${{ github.run_attempt }}",
        "path": "dist/pomodorough_core.wasm\ndist/SHA256SUMS",
        "if-no-files-found": "error",
        "retention-days": "1",
        "overwrite": "false",
    },
    ("verify-publish", "Check out exact workflow source"): {
        "ref": "${{ github.workflow_sha }}",
        "fetch-depth": "0",
        "fetch-tags": "true",
        "persist-credentials": "false",
    },
    ("verify-publish", "Download exact tested release candidate"): {
        "name": "core-release-${{ github.run_id }}-${{ github.run_attempt }}",
        "path": "dist",
    },
}
STEP_ENV = {
    ("verify-publish", name): {"GH_TOKEN": "${{ github.token }}"}
    for name in (
        "Verify transferred candidate and provenance",
        "Create or adopt exact sealed draft release",
        "Verify ID-bound draft assets",
        "Publish ID-bound verified draft",
    )
}


def strict_release_version(tag: str) -> str:
    if STRICT_TAG.fullmatch(tag) is None:
        raise ReleaseContractError("release tag must be strict vMAJOR.MINOR.PATCH SemVer")
    return tag[1:]


def _run_git(repository: Path, arguments: Sequence[str]) -> str:
    result = subprocess.run(
        ["git", "-C", str(repository), *arguments],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise ReleaseContractError(f"git {' '.join(arguments)} failed: {detail}")
    return result.stdout.strip()


def _resolve_commit(repository: Path, revision: str) -> str:
    return _run_git(repository, ["rev-parse", "--verify", f"{revision}^{{commit}}"])


def _cargo_version(cargo_toml: Path) -> str:
    try:
        package = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))["package"]
        version = package["version"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as error:
        raise ReleaseContractError(f"could not read package version: {error}") from error
    if not isinstance(version, str):
        raise ReleaseContractError("Cargo package version must be a string")
    return version


def validate_tagged_source(
    repository: Path,
    tag: str,
    workflow_sha: str,
    event_sha: str,
    event_ref: str,
    workflow_ref: str,
    github_repository: str,
    cargo_toml: Path,
) -> str:
    version = strict_release_version(tag)
    if COMMIT_SHA.fullmatch(workflow_sha) is None or COMMIT_SHA.fullmatch(event_sha) is None:
        raise ReleaseContractError("workflow and event SHAs must be full lowercase commit IDs")
    if event_ref != f"refs/tags/{tag}":
        raise ReleaseContractError("release event ref does not exactly match release tag")
    expected_workflow_ref = f"{github_repository}/{RELEASE_WORKFLOW}@{event_ref}"
    if workflow_ref != expected_workflow_ref:
        raise ReleaseContractError("workflow ref does not exactly match tagged release workflow")
    if _cargo_version(cargo_toml) != version:
        raise ReleaseContractError("release tag does not match Cargo package version")
    commits = {
        "tag": _resolve_commit(repository, f"refs/tags/{tag}"),
        "workflow": _resolve_commit(repository, workflow_sha),
        "event": _resolve_commit(repository, event_sha),
        "checkout": _resolve_commit(repository, "HEAD"),
    }
    if len(set(commits.values())) != 1:
        detail = ", ".join(f"{name}={value}" for name, value in commits.items())
        raise ReleaseContractError(f"tag, event, workflow, and checkout source differ: {detail}")
    return commits["tag"]


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as artifact:
        for chunk in iter(lambda: artifact.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_regular_file(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise ReleaseContractError(f"release entry must be a regular file: {path.name}")
    if path.stat().st_size == 0:
        raise ReleaseContractError(f"release entry must not be empty: {path.name}")


def create_manifest(artifact: Path, manifest: Path) -> str:
    if artifact.name != ARTIFACT_NAME or manifest.name != MANIFEST_NAME:
        raise ReleaseContractError("release artifact or manifest has unexpected name")
    _require_regular_file(artifact)
    digest = _sha256(artifact)
    manifest.write_bytes(f"{digest}  {ARTIFACT_NAME}\n".encode("ascii"))
    return digest


def parse_manifest(manifest: Path) -> str:
    _require_regular_file(manifest)
    try:
        contents = manifest.read_bytes().decode("ascii")
    except UnicodeDecodeError as error:
        raise ReleaseContractError("checksum manifest must be ASCII") from error
    match = re.fullmatch(rf"([0-9a-f]{{64}})  {re.escape(ARTIFACT_NAME)}\n", contents)
    if match is None or STRICT_SHA256.fullmatch(match.group(1)) is None:
        raise ReleaseContractError("checksum manifest is not strict canonical SHA-256 format")
    return match.group(1)


def verify_bundle(directory: Path) -> str:
    expected = {ARTIFACT_NAME, MANIFEST_NAME}
    try:
        entries = {entry.name for entry in directory.iterdir()}
    except OSError as error:
        raise ReleaseContractError(f"could not inspect release directory: {error}") from error
    if entries != expected:
        raise ReleaseContractError(f"release directory entries differ: {sorted(entries)}")
    artifact = directory / ARTIFACT_NAME
    manifest = directory / MANIFEST_NAME
    _require_regular_file(artifact)
    expected_digest = parse_manifest(manifest)
    actual_digest = _sha256(artifact)
    if actual_digest != expected_digest:
        raise ReleaseContractError(
            f"release artifact SHA-256 mismatch: expected {expected_digest}, got {actual_digest}"
        )
    return actual_digest


def validate_action_pins(workflows: Path) -> None:
    failures: list[str] = []
    for workflow in sorted(workflows.glob("*.y*ml")):
        for action, reference in ACTION_USE.findall(workflow.read_text(encoding="utf-8")):
            if not action.startswith("./") and COMMIT_SHA.fullmatch(reference) is None:
                failures.append(f"{workflow.name}: {action}@{reference}")
    if failures:
        raise ReleaseContractError("mutable GitHub Actions:\n" + "\n".join(failures))


def _require_fragments(label: str, contents: str, fragments: Sequence[str]) -> None:
    missing = [fragment for fragment in fragments if fragment not in contents]
    if missing:
        raise ReleaseContractError(f"{label} lacks required contract: {missing[0]}")


def _require_order(label: str, contents: str, fragments: Sequence[str]) -> None:
    positions = [contents.find(fragment) for fragment in fragments]
    if -1 in positions or positions != sorted(positions) or len(set(positions)) != len(positions):
        raise ReleaseContractError(f"{label} safety operations are out of order")


def _direct_condition(lines: Sequence[str], indent: int) -> str | None:
    prefix = " " * indent + "if:"
    values = [line[len(prefix) :].strip() for line in lines if line.startswith(prefix)]
    if len(values) > 1:
        raise ReleaseContractError("workflow block has duplicate direct if conditions")
    return values[0] if values else None


def _direct_scalar(lines: Sequence[str], indent: int, field: str) -> str | None:
    prefix = " " * indent + f"{field}:"
    values = [line[len(prefix) :].strip() for line in lines if line.startswith(prefix)]
    if len(values) > 1:
        raise ReleaseContractError(f"workflow block has duplicate direct {field} fields")
    return values[0] if values else None


def _direct_mapping(lines: Sequence[str], indent: int) -> dict[str, str]:
    marker = re.compile(r"^" + " " * indent + r"([A-Za-z0-9_-]+):(?:\s*(.*))?$")
    values: dict[str, str] = {}
    for line in lines:
        match = marker.match(line)
        if match is None:
            continue
        key = match.group(1)
        if key in values:
            raise ReleaseContractError(f"workflow block has duplicate direct {key} fields")
        values[key] = match.group(2) or ""
    return values


def _nested_mapping(lines: Sequence[str], indent: int, field: str) -> dict[str, str]:
    prefix = " " * indent + f"{field}:"
    starts = [index for index, line in enumerate(lines) if line.startswith(prefix)]
    if len(starts) > 1:
        raise ReleaseContractError(f"workflow block has duplicate direct {field} fields")
    if not starts:
        return {}
    if lines[starts[0]][len(prefix) :].strip():
        raise ReleaseContractError(f"workflow {field} must be a mapping")
    end = next(
        (
            index
            for index in range(starts[0] + 1, len(lines))
            if lines[index].strip() and len(lines[index]) - len(lines[index].lstrip()) <= indent
        ),
        len(lines),
    )
    return _mapping_children(lines[starts[0] + 1 : end], indent + 2, field)


def _mapping_children(lines: Sequence[str], indent: int, field: str) -> dict[str, str]:
    marker = re.compile(r"^" + " " * indent + r"([A-Za-z0-9_-]+):(?:\s*(.*))?$")
    values: dict[str, str] = {}
    starts = [(index, match) for index, line in enumerate(lines) if (match := marker.match(line))]
    for offset, (index, match) in enumerate(starts):
        key = match.group(1)
        if key in values:
            raise ReleaseContractError(f"workflow {field} has duplicate {key} fields")
        value = match.group(2) or ""
        next_index = starts[offset + 1][0] if offset + 1 < len(starts) else len(lines)
        if value == "|":
            body = [line[indent + 2 :] for line in lines[index + 1 : next_index] if line.strip()]
            if not body or any(not line.startswith(" " * (indent + 2)) for line in lines[index + 1 : next_index] if line.strip()):
                raise ReleaseContractError(f"workflow {field}.{key} block is malformed")
            value = "\n".join(body)
        values[key] = value
    return values


def _direct_run(lines: Sequence[str]) -> str | None:
    value = _direct_scalar(lines, 8, "run")
    if value is None:
        return None
    if value != "|":
        return value
    start = next(index for index, line in enumerate(lines) if line.startswith("        run:"))
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line and not line.startswith("          "):
            break
        body.append(line[10:] if line else "")
    if not body:
        raise ReleaseContractError("required workflow run block is empty")
    return "\n".join(body)


def _shell_lines(script: str) -> tuple[str, ...]:
    joined = re.sub(r"\\\n\s*", " ", script)
    return tuple(line.strip() for line in joined.splitlines() if line.strip())


def _matches_command(line: str, command: str) -> bool:
    if line == command:
        return True
    return line.startswith(command) and line[len(command)] in " \t"


def _require_run_commands(step: WorkflowStep, commands: Sequence[str]) -> None:
    if step.run is None or step.uses is not None:
        raise ReleaseContractError(f"required step {step.name} must be a direct run step")
    lines = _shell_lines(step.run)
    positions: list[int] = []
    for command in commands:
        matches = [
            index for index, line in enumerate(lines) if _matches_command(line, command)
        ]
        if len(matches) != 1:
            raise ReleaseContractError(f"required step {step.name} command changed: {command}")
        positions.append(matches[0])
    if positions != sorted(positions) or len(set(positions)) != len(positions):
        raise ReleaseContractError(f"required step {step.name} commands are out of order")
    forbidden = ("set +e", "if ", "function ", "trap ")
    allowed_semicolons = {
        ";;",
        "adopt) ;;",
        '*) echo "Unexpected draft mode: $mode" >&2; exit 1 ;;',
    }
    disabled = any(
        line.startswith(forbidden)
        or re.search(r"\b[A-Za-z_][A-Za-z0-9_]*\s*\(\s*\)\s*\{", line)
        or "||" in line
        or re.search(r"(?<!\|)\|(?!\|)", line)
        or (";" in line and not line.endswith("; do") and line not in allowed_semicolons)
        or re.search(r"(?<![>&])&(?![>&])", line)
        for line in lines
    )
    if disabled:
        raise ReleaseContractError(f"required step {step.name} contains a disabled command gate")


def _require_uses(step: WorkflowStep, action: str) -> None:
    if step.uses is None or step.run is not None:
        raise ReleaseContractError(f"required step {step.name} must be a direct uses step")
    if step.uses.split(" #", 1)[0] != action:
        raise ReleaseContractError(f"required step {step.name} action changed")


def _require_step_configuration(job_name: str, step: WorkflowStep, kind: str) -> None:
    lines = step.contents.splitlines()
    expected_with = STEP_WITH.get((job_name, step.name), {})
    expected_env = STEP_ENV.get((job_name, step.name), {})
    fields = {kind}
    if expected_with:
        fields.add("with")
    if expected_env:
        fields.add("env")
    if set(_direct_mapping(lines, 8)) != fields:
        raise ReleaseContractError(f"required step {step.name} configuration changed")
    if _nested_mapping(lines, 8, "with") != expected_with:
        raise ReleaseContractError(f"required step {step.name} inputs changed")
    if _nested_mapping(lines, 8, "env") != expected_env:
        raise ReleaseContractError(f"required step {step.name} environment changed")


def _workflow_blocks(contents: str, indent: int, start: int, end: int) -> list[tuple[str, int, int]]:
    lines = contents.splitlines()
    marker = re.compile(r"^" + " " * indent + r"([A-Za-z0-9_-]+):\s*$")
    starts = [
        (match.group(1), index)
        for index in range(start, end)
        if (match := marker.match(lines[index]))
    ]
    return [(name, index, starts[offset + 1][1] if offset + 1 < len(starts) else end) for offset, (name, index) in enumerate(starts)]


def _workflow_steps(lines: Sequence[str]) -> tuple[WorkflowStep, ...]:
    marker = re.compile(r"^      - name:\s*(\S(?:.*\S)?)\s*$")
    starts = [(match.group(1), index) for index, line in enumerate(lines) if (match := marker.match(line))]
    steps: list[WorkflowStep] = []
    for offset, (name, start) in enumerate(starts):
        end = starts[offset + 1][1] if offset + 1 < len(starts) else len(lines)
        block = lines[start:end]
        steps.append(
            WorkflowStep(
                name,
                _direct_condition(block, 8),
                _direct_run(block),
                _direct_scalar(block, 8, "uses"),
                "\n".join(block),
            )
        )
    return tuple(steps)


def _workflow_jobs(contents: str) -> dict[str, WorkflowJob]:
    if "\t" in contents:
        raise ReleaseContractError("workflow must not contain tabs")
    lines = contents.splitlines()
    jobs_lines = [index for index, line in enumerate(lines) if line == "jobs:"]
    if len(jobs_lines) != 1:
        raise ReleaseContractError("workflow must contain exactly one top-level jobs mapping")
    start = jobs_lines[0] + 1
    end = next((index for index in range(start, len(lines)) if lines[index] and not lines[index].startswith(" ")), len(lines))
    blocks = _workflow_blocks(contents, 2, start, end)
    if not blocks or len({name for name, _, _ in blocks}) != len(blocks):
        raise ReleaseContractError("workflow job structure is malformed or duplicated")
    return {
        name: WorkflowJob(name, _direct_condition(lines[block_start:block_end], 4), "\n".join(lines[block_start:block_end]), _workflow_steps(lines[block_start:block_end]))
        for name, block_start, block_end in blocks
    }


def _require_header(contents: str, expected: Sequence[str]) -> None:
    lines = contents.splitlines()
    jobs_index = next((index for index, line in enumerate(lines) if line == "jobs:"), -1)
    if jobs_index < 0:
        raise ReleaseContractError("workflow lacks top-level jobs mapping")
    actual = tuple(
        line.rstrip()
        for line in lines[:jobs_index]
        if line.strip() and not line.lstrip().startswith("#")
    )
    if actual != tuple(expected):
        raise ReleaseContractError("workflow trigger, permissions, or concurrency changed")


def _require_job(
    job: WorkflowJob, expected_steps: Sequence[tuple[str, str, Sequence[str]]]
) -> None:
    if job.condition is not None:
        raise ReleaseContractError(f"required job {job.name} must not have an if condition")
    if _direct_scalar(job.contents.splitlines(), 4, "continue-on-error") is not None:
        raise ReleaseContractError(f"required job {job.name} must not continue on error")
    if _direct_mapping(job.contents.splitlines(), 4) != JOB_FIELDS[job.name]:
        raise ReleaseContractError(f"required job {job.name} configuration changed")
    if _nested_mapping(job.contents.splitlines(), 4, "permissions") != JOB_PERMISSIONS[job.name]:
        raise ReleaseContractError(f"required job {job.name} permissions changed")
    names = [step.name for step in job.steps]
    expected_names = [name for name, _, _ in expected_steps]
    if names != expected_names:
        raise ReleaseContractError(f"required job {job.name} step structure changed")
    for step, (_, kind, commands) in zip(job.steps, expected_steps, strict=True):
        if step.condition is not None:
            raise ReleaseContractError(f"required step {step.name} must not have an if condition")
        if _direct_scalar(step.contents.splitlines(), 8, "continue-on-error") is not None:
            raise ReleaseContractError(f"required step {step.name} must not continue on error")
        _require_step_configuration(job.name, step, kind)
        if kind == "uses":
            _require_uses(step, commands[0])
        else:
            _require_run_commands(step, commands)


def validate_ci_workflow(contents: str) -> None:
    _require_fragments("CI workflow", contents, CI_REQUIRED)
    _require_header(contents, CI_HEADER)
    jobs = _workflow_jobs(contents)
    if set(jobs) != {"rust"}:
        raise ReleaseContractError("CI workflow job structure changed")
    _require_job(jobs["rust"], CI_STEPS)


def validate_release_workflow(contents: str) -> None:
    _require_fragments("release workflow", contents, RELEASE_REQUIRED)
    _require_header(contents, RELEASE_HEADER)
    if any(fragment in contents for fragment in RELEASE_FORBIDDEN):
        raise ReleaseContractError("release workflow contains unsafe recovery or overwrite behavior")
    if any(contents.count(fragment) != count for fragment, count in RELEASE_COUNTS.items()):
        raise ReleaseContractError("release workflow operation count differs from fail-closed contract")
    _require_order("release workflow", contents, RELEASE_ORDER)
    jobs = _workflow_jobs(contents)
    if set(jobs) != set(RELEASE_STEPS):
        raise ReleaseContractError("release workflow job structure changed")
    if not re.search(r"^    needs:\s*build-test-attest\s*$", jobs["verify-publish"].contents, re.MULTILINE):
        raise ReleaseContractError("publication job dependency changed")
    for name, expected_steps in RELEASE_STEPS.items():
        _require_job(jobs[name], expected_steps)


def _run_gh(arguments: Sequence[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["gh", *arguments], check=False, capture_output=True, text=True)


def _run_gh_bytes(arguments: Sequence[str]) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(["gh", *arguments], check=False, capture_output=True)


def _gh_json(arguments: Sequence[str], label: str) -> object:
    result = _run_gh(arguments)
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise ReleaseContractError(f"{label} failed closed: {detail}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ReleaseContractError(f"{label} returned malformed JSON") from error


def _runtime_integer(value: object, label: str) -> int:
    if isinstance(value, bool):
        raise ReleaseContractError(f"{label} must be a positive integer")
    try:
        integer = int(value)
    except (TypeError, ValueError) as error:
        raise ReleaseContractError(f"{label} must be a positive integer") from error
    if integer <= 0 or str(integer) != str(value):
        raise ReleaseContractError(f"{label} must be a positive integer")
    return integer


def _workflow_identity(
    repository: str,
    tag: str,
    source_sha: str,
    workflow_sha: str,
    run_id: object,
    run_attempt: object,
) -> dict[str, object]:
    strict_release_version(tag)
    if COMMIT_SHA.fullmatch(source_sha) is None or workflow_sha != source_sha:
        raise ReleaseContractError("release source and workflow SHA identity is malformed")
    return {
        "repository": repository,
        "tag": tag,
        "source_sha": source_sha,
        "workflow_sha": workflow_sha,
        "run_id": _runtime_integer(run_id, "workflow run ID"),
        "creator_run_attempt": _runtime_integer(run_attempt, "workflow run attempt"),
    }


def draft_release_body(identity: dict[str, object]) -> str:
    return DRAFT_BODY_PREFIX + json.dumps(identity, sort_keys=True, separators=(",", ":"))


def _parse_draft_body(body: str) -> dict[str, object]:
    if not body.startswith(DRAFT_BODY_PREFIX):
        raise ReleaseContractError("draft release lacks exact workflow identity marker")
    payload = body[len(DRAFT_BODY_PREFIX) :]
    try:
        identity = json.loads(payload)
    except json.JSONDecodeError as error:
        raise ReleaseContractError("draft release workflow identity marker is malformed") from error
    expected = {
        "repository",
        "tag",
        "source_sha",
        "workflow_sha",
        "run_id",
        "creator_run_attempt",
    }
    if not isinstance(identity, dict) or set(identity) != expected:
        raise ReleaseContractError("draft release workflow identity marker is malformed")
    return identity


def _require_workflow_identity(
    actual: dict[str, object], expected: dict[str, object], current_attempt: int
) -> None:
    fixed = ("repository", "tag", "source_sha", "workflow_sha", "run_id")
    if any(actual.get(field) != expected[field] for field in fixed):
        raise ReleaseContractError("draft release belongs to a different workflow identity")
    creator_attempt = _runtime_integer(actual.get("creator_run_attempt"), "creator run attempt")
    if creator_attempt > current_attempt:
        raise ReleaseContractError("draft release attempt identity is not adoptable")


def _github_object(endpoint: str) -> tuple[str, str]:
    try:
        target = _gh_json(["api", endpoint], "GitHub object lookup")["object"]
        object_type, sha = target["type"], target["sha"]
    except (KeyError, TypeError) as error:
        raise ReleaseContractError("GitHub object response is malformed") from error
    if object_type not in {"commit", "tag"} or COMMIT_SHA.fullmatch(sha) is None:
        raise ReleaseContractError("GitHub tag target is not a full commit or tag object")
    return object_type, sha


def require_remote_tag_source(repository: str, tag: str, expected_sha: str) -> None:
    strict_release_version(tag)
    if COMMIT_SHA.fullmatch(expected_sha) is None:
        raise ReleaseContractError("expected source must be a full lowercase commit ID")
    endpoint = f"repos/{repository}/git/ref/tags/{tag}"
    object_type, sha = _github_object(endpoint)
    visited: set[str] = set()
    while object_type == "tag" and len(visited) < 8:
        if sha in visited:
            raise ReleaseContractError("GitHub tag peel contains a cycle")
        visited.add(sha)
        object_type, sha = _github_object(f"repos/{repository}/git/tags/{sha}")
    if object_type != "commit" or sha != expected_sha:
        raise ReleaseContractError(
            f"remote tag does not peel to exact workflow source: expected {expected_sha}, got {sha}"
        )


def _positive_integer(record: dict[str, object], field: str) -> int:
    value = record.get(field)
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ReleaseContractError(f"GitHub release field {field} is malformed")
    return value


def _text(record: dict[str, object], field: str) -> str:
    value = record.get(field)
    if not isinstance(value, str) or not value:
        raise ReleaseContractError(f"GitHub release field {field} is malformed")
    return value


def _timestamp(record: dict[str, object], field: str) -> str:
    value = _text(record, field)
    if RELEASE_TIME.fullmatch(value) is None:
        raise ReleaseContractError(f"GitHub release field {field} is not an exact UTC timestamp")
    return value


def _boolean(record: dict[str, object], field: str) -> bool:
    value = record.get(field)
    if not isinstance(value, bool):
        raise ReleaseContractError(f"GitHub release field {field} is malformed")
    return value


def _normalize_asset(value: object) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ReleaseContractError("GitHub release asset is malformed")
    digest = _text(value, "digest")
    if not digest.startswith("sha256:") or STRICT_SHA256.fullmatch(digest[7:]) is None:
        raise ReleaseContractError("GitHub release asset digest is not exact SHA-256")
    state = _text(value, "state")
    if state != "uploaded":
        raise ReleaseContractError("GitHub release asset is not fully uploaded")
    size = _positive_integer(value, "size")
    return {
        "id": _positive_integer(value, "id"),
        "name": _text(value, "name"),
        "state": state,
        "size": size,
        "digest": digest,
        "created_at": _timestamp(value, "created_at"),
        "updated_at": _timestamp(value, "updated_at"),
    }


def _normalize_release(value: object, tag: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise ReleaseContractError("GitHub release record is malformed")
    assets = value.get("assets")
    if not isinstance(assets, list):
        raise ReleaseContractError("GitHub release assets are malformed")
    normalized_assets = sorted((_normalize_asset(asset) for asset in assets), key=lambda a: a["name"])
    published_at = value.get("published_at")
    if published_at is not None:
        published_at = _timestamp(value, "published_at")
    normalized = {
        "id": _positive_integer(value, "id"),
        "node_id": _text(value, "node_id"),
        "tag_name": _text(value, "tag_name"),
        "name": _text(value, "name"),
        "body": _text(value, "body"),
        "draft": _boolean(value, "draft"),
        "prerelease": _boolean(value, "prerelease"),
        "immutable": _boolean(value, "immutable"),
        "created_at": _timestamp(value, "created_at"),
        "updated_at": _timestamp(value, "updated_at"),
        "published_at": published_at,
        "assets": normalized_assets,
    }
    if normalized["tag_name"] != tag:
        raise ReleaseContractError("GitHub release tag identity changed")
    return normalized


def _release_records(repository: str) -> list[dict[str, object]]:
    endpoint = f"repos/{repository}/releases?per_page=100"
    pages = _gh_json(["api", "--paginate", "--slurp", endpoint], "release list lookup")
    if not isinstance(pages, list) or any(not isinstance(page, list) for page in pages):
        raise ReleaseContractError("paginated GitHub release list is malformed")
    records: list[dict[str, object]] = []
    for page in pages:
        for value in page:
            if not isinstance(value, dict) or not isinstance(value.get("tag_name"), str):
                raise ReleaseContractError("GitHub release list entry is malformed")
            records.append(value)
    return records


def _matching_releases(repository: str, tag: str) -> list[dict[str, object]]:
    strict_release_version(tag)
    return [record for record in _release_records(repository) if record["tag_name"] == tag]


def _unique_release(repository: str, tag: str) -> dict[str, object]:
    matches = _matching_releases(repository, tag)
    if len(matches) != 1:
        raise ReleaseContractError(
            f"release tag must identify exactly one release, found {len(matches)}"
        )
    return _normalize_release(matches[0], tag)


def _release_by_id(repository: str, release_id: int, tag: str) -> dict[str, object]:
    endpoint = f"repos/{repository}/releases/{release_id}"
    return _normalize_release(_gh_json(["api", endpoint], "release ID lookup"), tag)


def _current_release(repository: str, tag: str) -> dict[str, object]:
    listed = _unique_release(repository, tag)
    current = _release_by_id(repository, int(listed["id"]), tag)
    if current != listed:
        raise ReleaseContractError("release changed between tag list and ID lookup")
    return current


def _require_draft_state(release: dict[str, object], tag: str) -> None:
    if (
        release["name"] != tag
        or release["draft"] is not True
        or release["prerelease"] is not False
        or release["immutable"] is not False
        or release["published_at"] is not None
    ):
        raise ReleaseContractError("release is not exact unpublished draft")


def require_missing_release(repository: str, tag: str) -> None:
    matches = _matching_releases(repository, tag)
    if matches:
        raise ReleaseContractError(
            f"release {tag} already exists ({len(matches)} matches); refusing overwrite"
        )


def require_draft_release(
    repository: str, tag: str, expected_id: int | None = None
) -> dict[str, object]:
    release = _current_release(repository, tag)
    _require_draft_state(release, tag)
    if expected_id is not None and release["id"] != expected_id:
        raise ReleaseContractError("draft release ID changed")
    return release


def _local_assets(directory: Path) -> dict[str, dict[str, object]]:
    verify_bundle(directory)
    assets: dict[str, dict[str, object]] = {}
    for name in (ARTIFACT_NAME, MANIFEST_NAME):
        path = directory / name
        assets[name] = {
            "name": name,
            "size": path.stat().st_size,
            "digest": f"sha256:{_sha256(path)}",
        }
    return assets


def _release_seal(
    release: dict[str, object], directory: Path, workflow: dict[str, object]
) -> dict[str, object]:
    local = _local_assets(directory)
    remote_assets = release["assets"]
    if not isinstance(remote_assets, list):
        raise ReleaseContractError("normalized release assets are malformed")
    remote_names = [asset["name"] for asset in remote_assets]
    if len(remote_assets) != len(local) or set(remote_names) != set(local):
        raise ReleaseContractError("draft release asset names differ from exact bundle")
    if len(set(remote_names)) != len(remote_names):
        raise ReleaseContractError("draft release asset names are duplicated")
    if len({asset["id"] for asset in remote_assets}) != len(remote_assets):
        raise ReleaseContractError("draft release asset IDs are duplicated")
    for asset in remote_assets:
        expected = local[str(asset["name"])]
        if asset["size"] != expected["size"] or asset["digest"] != expected["digest"]:
            raise ReleaseContractError("draft release asset bytes differ from exact bundle")
    release_identity = {key: value for key, value in release.items() if key != "assets"}
    return {
        "schema": 2,
        "release": release_identity,
        "assets": remote_assets,
        "workflow": workflow,
    }


def _asset_bytes(repository: str, asset: dict[str, object]) -> bytes:
    endpoint = f"repos/{repository}/releases/assets/{asset['id']}"
    result = _run_gh_bytes(["api", "-H", "Accept: application/octet-stream", endpoint])
    if result.returncode != 0:
        detail = result.stderr.decode(errors="replace").strip()
        raise ReleaseContractError(f"release asset ID download failed closed: {detail}")
    digest = f"sha256:{hashlib.sha256(result.stdout).hexdigest()}"
    if len(result.stdout) != asset["size"] or digest != asset["digest"]:
        raise ReleaseContractError("release asset ID bytes differ from bound identity")
    return result.stdout


def _verify_asset_bytes(
    repository: str, assets: list[dict[str, object]], directory: Path | None = None
) -> None:
    if directory is not None:
        if directory.exists() or directory.is_symlink():
            raise ReleaseContractError("draft asset download directory already exists")
        directory.mkdir(parents=True)
    for asset in assets:
        contents = _asset_bytes(repository, asset)
        if directory is not None:
            (directory / str(asset["name"])).write_bytes(contents)
    if directory is not None:
        verify_bundle(directory)


def _write_seal(path: Path, seal: dict[str, object]) -> None:
    if path.exists() or path.is_symlink():
        raise ReleaseContractError("release seal already exists")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    if temporary.exists() or temporary.is_symlink():
        raise ReleaseContractError("release seal temporary path already exists")
    temporary.write_text(json.dumps(seal, sort_keys=True, separators=(",", ":")) + "\n")
    temporary.replace(path)


def _write_new_text(path: Path, contents: str) -> None:
    if path.exists() or path.is_symlink():
        raise ReleaseContractError(f"output path already exists: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")


def _read_seal(path: Path) -> dict[str, object]:
    if path.is_symlink() or not path.is_file():
        raise ReleaseContractError("release seal must be a regular file")
    try:
        seal = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ReleaseContractError("release seal is malformed") from error
    expected = {"schema", "release", "assets", "workflow"}
    if not isinstance(seal, dict) or set(seal) != expected:
        raise ReleaseContractError("release seal structure is malformed")
    release = seal.get("release")
    workflow = seal.get("workflow")
    if seal.get("schema") != 2 or not isinstance(release, dict):
        raise ReleaseContractError("release seal identity is malformed")
    _positive_integer(release, "id")
    if not isinstance(seal.get("assets"), list) or not isinstance(workflow, dict):
        raise ReleaseContractError("release seal assets are malformed")
    return seal


def _release_workflow_identity(
    release: dict[str, object], expected: dict[str, object], current_attempt: int
) -> dict[str, object]:
    body = release.get("body")
    if not isinstance(body, str):
        raise ReleaseContractError("draft release body is malformed")
    actual = _parse_draft_body(body)
    _require_workflow_identity(actual, expected, current_attempt)
    return actual


def prepare_draft_release(
    repository: str,
    tag: str,
    expected_sha: str,
    workflow_sha: str,
    run_id: object,
    run_attempt: object,
    directory: Path,
    download_directory: Path,
    body_path: Path,
    seal_path: Path,
) -> str:
    identity = _workflow_identity(
        repository, tag, expected_sha, workflow_sha, run_id, run_attempt
    )
    body = draft_release_body(identity)
    matches = _matching_releases(repository, tag)
    if not matches:
        _write_new_text(body_path, body + "\n")
        return "create"
    if len(matches) != 1:
        raise ReleaseContractError("release tag has duplicate records; refusing adoption")
    release = _current_release(repository, tag)
    _require_draft_state(release, tag)
    actual_identity = _release_workflow_identity(
        release, identity, int(identity["creator_run_attempt"])
    )
    seal = _release_seal(release, directory, actual_identity)
    assets = seal["assets"]
    if not isinstance(assets, list):
        raise ReleaseContractError("release seal assets are malformed")
    _verify_asset_bytes(repository, assets, download_directory)
    _write_seal(seal_path, seal)
    return "adopt"


def capture_draft_release(
    repository: str,
    tag: str,
    expected_sha: str,
    workflow_sha: str,
    run_id: object,
    run_attempt: object,
    directory: Path,
    download_directory: Path,
    seal_path: Path,
) -> dict[str, object]:
    identity = _workflow_identity(
        repository, tag, expected_sha, workflow_sha, run_id, run_attempt
    )
    release = require_draft_release(repository, tag)
    actual_identity = _release_workflow_identity(
        release, identity, int(identity["creator_run_attempt"])
    )
    seal = _release_seal(release, directory, actual_identity)
    assets = seal["assets"]
    if not isinstance(assets, list):
        raise ReleaseContractError("release seal assets are malformed")
    _verify_asset_bytes(repository, assets, download_directory)
    _write_seal(seal_path, seal)
    return seal


def _require_published_identity(
    published: dict[str, object], seal: dict[str, object]
) -> None:
    draft = seal["release"]
    if not isinstance(draft, dict):
        raise ReleaseContractError("release seal identity is malformed")
    stable_fields = ("id", "node_id", "tag_name", "name", "body", "created_at")
    if any(published[field] != draft[field] for field in stable_fields):
        raise ReleaseContractError("published release identity differs from bound draft")
    if published["draft"] is not False or published["prerelease"] is not False:
        raise ReleaseContractError("release publication response is not exact public release")
    if published["immutable"] is not True:
        raise ReleaseContractError("published release is not immutable")
    if published["published_at"] is None or published["assets"] != seal["assets"]:
        raise ReleaseContractError("published release assets or timestamp changed unexpectedly")


def _publish_by_id(repository: str, release_id: int, tag: str) -> dict[str, object]:
    endpoint = f"repos/{repository}/releases/{release_id}"
    payload = _gh_json(
        ["api", "--method", "PATCH", endpoint, "-F", "draft=false"],
        "ID-bound release publication",
    )
    return _normalize_release(payload, tag)


def _verified_draft_for_publication(
    repository: str,
    tag: str,
    expected_sha: str,
    workflow_sha: str,
    run_id: object,
    run_attempt: object,
    directory: Path,
    seal_path: Path,
) -> tuple[dict[str, object], int, list[object]]:
    identity = _workflow_identity(
        repository, tag, expected_sha, workflow_sha, run_id, run_attempt
    )
    seal = _read_seal(seal_path)
    sealed_release = seal["release"]
    if not isinstance(sealed_release, dict):
        raise ReleaseContractError("release seal identity is malformed")
    sealed_workflow = seal["workflow"]
    if not isinstance(sealed_workflow, dict):
        raise ReleaseContractError("release seal workflow identity is malformed")
    _require_workflow_identity(
        sealed_workflow, identity, int(identity["creator_run_attempt"])
    )
    release_id = _positive_integer(sealed_release, "id")
    require_remote_tag_source(repository, tag, expected_sha)
    current = require_draft_release(repository, tag, release_id)
    actual_identity = _release_workflow_identity(
        current, identity, int(identity["creator_run_attempt"])
    )
    if _release_seal(current, directory, actual_identity) != seal:
        raise ReleaseContractError("draft release identity changed after verification")
    assets = seal["assets"]
    if not isinstance(assets, list):
        raise ReleaseContractError("release seal assets are malformed")
    _verify_asset_bytes(repository, assets)
    return seal, release_id, assets


def publish_verified_draft(
    repository: str,
    tag: str,
    expected_sha: str,
    workflow_sha: str,
    run_id: object,
    run_attempt: object,
    directory: Path,
    seal_path: Path,
) -> dict[str, object]:
    seal, release_id, assets = _verified_draft_for_publication(
        repository, tag, expected_sha, workflow_sha, run_id, run_attempt, directory, seal_path
    )
    require_remote_tag_source(repository, tag, expected_sha)
    published = _publish_by_id(repository, release_id, tag)
    try:
        require_remote_tag_source(repository, tag, expected_sha)
        _require_published_identity(published, seal)
        final = _current_release(repository, tag)
        _require_published_identity(final, seal)
        if final != published:
            raise ReleaseContractError("published release changed during final verification")
        _verify_asset_bytes(repository, assets)
    except ReleaseContractError as error:
        raise ReleaseContractError(
            "publication race detected after REST PATCH; release may already be public and "
            "requires incident response"
        ) from error
    return final


def check_workflows(release: Path, ci: Path, workflows: Path) -> None:
    validate_release_workflow(release.read_text(encoding="utf-8"))
    validate_ci_workflow(ci.read_text(encoding="utf-8"))
    validate_action_pins(workflows)


def _add_release_identity_arguments(command: argparse.ArgumentParser) -> None:
    command.add_argument("--github-repository", required=True)
    command.add_argument("--tag", required=True)
    command.add_argument("--expected-sha", required=True)
    command.add_argument("--workflow-sha", required=True)
    command.add_argument("--run-id", required=True)
    command.add_argument("--run-attempt", required=True)
    command.add_argument("--directory", type=Path, required=True)
    command.add_argument("--seal", type=Path, required=True)


def _add_release_state_parsers(commands: argparse._SubParsersAction) -> None:
    for name in ("require-missing-release", "require-draft-release"):
        release = commands.add_parser(name)
        release.add_argument("--github-repository", required=True)
        release.add_argument("--tag", required=True)
    prepare = commands.add_parser("prepare-draft-release")
    _add_release_identity_arguments(prepare)
    prepare.add_argument("--download-directory", type=Path, required=True)
    prepare.add_argument("--body-file", type=Path, required=True)
    capture = commands.add_parser("capture-draft-release")
    _add_release_identity_arguments(capture)
    capture.add_argument("--download-directory", type=Path, required=True)
    publish = commands.add_parser("publish-verified-draft")
    _add_release_identity_arguments(publish)
    remote = commands.add_parser("require-remote-tag-source")
    remote.add_argument("--github-repository", required=True)
    remote.add_argument("--tag", required=True)
    remote.add_argument("--expected-sha", required=True)


def _argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__, epilog=PUBLICATION_RESIDUAL)
    commands = parser.add_subparsers(dest="command", required=True)
    source = commands.add_parser("validate-source")
    source.add_argument("--repository", type=Path, default=Path("."))
    source.add_argument("--tag", required=True)
    source.add_argument("--workflow-sha", required=True)
    source.add_argument("--event-sha", required=True)
    source.add_argument("--event-ref", required=True)
    source.add_argument("--workflow-ref", required=True)
    source.add_argument("--github-repository", required=True)
    source.add_argument("--cargo-toml", type=Path, default=Path("Cargo.toml"))
    manifest = commands.add_parser("create-manifest")
    manifest.add_argument("--artifact", type=Path, required=True)
    manifest.add_argument("--manifest", type=Path, required=True)
    bundle = commands.add_parser("verify-bundle")
    bundle.add_argument("--directory", type=Path, required=True)
    workflow = commands.add_parser("check-workflows")
    workflow.add_argument("--release", type=Path, default=Path(RELEASE_WORKFLOW))
    workflow.add_argument("--ci", type=Path, default=Path(".github/workflows/ci.yml"))
    workflow.add_argument("--workflows", type=Path, default=Path(".github/workflows"))
    _add_release_state_parsers(commands)
    return parser


def _execute_release_state(arguments: argparse.Namespace) -> object:
    if arguments.command == "require-missing-release":
        require_missing_release(arguments.github_repository, arguments.tag)
        return "release tag is unpublished and unused"
    if arguments.command == "require-remote-tag-source":
        require_remote_tag_source(
            arguments.github_repository, arguments.tag, arguments.expected_sha
        )
        return "remote tag peels to exact workflow source"
    if arguments.command == "require-draft-release":
        return require_draft_release(arguments.github_repository, arguments.tag)
    if arguments.command == "prepare-draft-release":
        return prepare_draft_release(
            arguments.github_repository,
            arguments.tag,
            arguments.expected_sha,
            arguments.workflow_sha,
            arguments.run_id,
            arguments.run_attempt,
            arguments.directory,
            arguments.download_directory,
            arguments.body_file,
            arguments.seal,
        )
    if arguments.command == "capture-draft-release":
        return capture_draft_release(
            arguments.github_repository,
            arguments.tag,
            arguments.expected_sha,
            arguments.workflow_sha,
            arguments.run_id,
            arguments.run_attempt,
            arguments.directory,
            arguments.download_directory,
            arguments.seal,
        )
    return publish_verified_draft(
        arguments.github_repository,
        arguments.tag,
        arguments.expected_sha,
        arguments.workflow_sha,
        arguments.run_id,
        arguments.run_attempt,
        arguments.directory,
        arguments.seal,
    )


def _execute(arguments: argparse.Namespace) -> object:
    if arguments.command == "validate-source":
        return validate_tagged_source(
            arguments.repository,
            arguments.tag,
            arguments.workflow_sha,
            arguments.event_sha,
            arguments.event_ref,
            arguments.workflow_ref,
            arguments.github_repository,
            arguments.cargo_toml,
        )
    if arguments.command == "create-manifest":
        return create_manifest(arguments.artifact, arguments.manifest)
    if arguments.command == "verify-bundle":
        return verify_bundle(arguments.directory)
    if arguments.command == "check-workflows":
        check_workflows(arguments.release, arguments.ci, arguments.workflows)
        return "release workflows satisfy C5 contract"
    return _execute_release_state(arguments)


def main() -> int:
    arguments = _argument_parser().parse_args()
    try:
        value = _execute(arguments)
    except (OSError, ReleaseContractError) as error:
        raise SystemExit(str(error)) from error
    if isinstance(value, dict):
        print(json.dumps(value, sort_keys=True, separators=(",", ":")))
    else:
        print(value)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
