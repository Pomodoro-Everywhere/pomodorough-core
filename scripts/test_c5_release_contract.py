from __future__ import annotations

import importlib.util
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = Path(__file__).with_name("c5_release_contract.py")
RELEASE_WORKFLOW = ROOT / ".github" / "workflows" / "release.yml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def load_contract():
    spec = importlib.util.spec_from_file_location("c5_release_contract", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load C5 release contract")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def git(repository: Path, *arguments: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repository), *arguments],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def commit_file(repository: Path, name: str, contents: str, message: str) -> str:
    path = repository / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(contents, encoding="utf-8")
    git(repository, "add", name)
    git(repository, "commit", "-q", "-m", message)
    return git(repository, "rev-parse", "HEAD")


class TaggedSourceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_contract()
        self.temporary = tempfile.TemporaryDirectory()
        self.repository = Path(self.temporary.name)
        git(self.repository, "init", "-q")
        git(self.repository, "config", "user.email", "c5@example.invalid")
        git(self.repository, "config", "user.name", "C5 Test")
        commit_file(self.repository, "Cargo.toml", '[package]\nname="core"\nversion="1.2.3"\n', "source")
        self.source = commit_file(
            self.repository,
            ".github/workflows/release.yml",
            "name: Release\n",
            "workflow",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def validate(self, tag: str, event_sha: str, workflow_sha: str | None = None) -> str:
        return self.contract.validate_tagged_source(
            self.repository,
            tag,
            workflow_sha or self.source,
            event_sha,
            f"refs/tags/{tag}",
            f"example/core/.github/workflows/release.yml@refs/tags/{tag}",
            "example/core",
            self.repository / "Cargo.toml",
        )

    def test_accepts_lightweight_and_annotated_tags_by_peeled_commit(self) -> None:
        git(self.repository, "tag", "v1.2.3-light", self.source)
        with self.assertRaises(self.contract.ReleaseContractError):
            self.validate("v1.2.3-light", self.source)
        git(self.repository, "tag", "v1.2.3", self.source)
        self.assertEqual(self.validate("v1.2.3", self.source), self.source)
        git(self.repository, "tag", "-d", "v1.2.3")
        git(self.repository, "tag", "-a", "v1.2.3", "-m", "annotated", self.source)
        tag_object = git(self.repository, "rev-parse", "refs/tags/v1.2.3^{object}")
        self.assertEqual(self.validate("v1.2.3", tag_object), self.source)

    def test_rejects_descendant_workflow_source(self) -> None:
        git(self.repository, "tag", "v1.2.3", self.source)
        descendant = commit_file(self.repository, "README.md", "later\n", "later")
        with self.assertRaisesRegex(
            self.contract.ReleaseContractError, "tag, event, workflow, and checkout source differ"
        ):
            self.validate("v1.2.3", descendant, descendant)

    def test_rejects_moved_tag_even_when_original_source_is_checked_out(self) -> None:
        git(self.repository, "tag", "v1.2.3", self.source)
        moved = commit_file(self.repository, "README.md", "moved\n", "move tag")
        git(self.repository, "tag", "-f", "v1.2.3", moved)
        git(self.repository, "checkout", "-q", "--detach", self.source)
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "source differ"):
            self.validate("v1.2.3", self.source)

    def test_rejects_wrong_workflow_ref_and_version_metadata(self) -> None:
        git(self.repository, "tag", "v1.2.3", self.source)
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "workflow ref"):
            self.contract.validate_tagged_source(
                self.repository,
                "v1.2.3",
                self.source,
                self.source,
                "refs/tags/v1.2.3",
                "example/core/.github/workflows/release.yml@refs/heads/main",
                "example/core",
                self.repository / "Cargo.toml",
            )
        (self.repository / "Cargo.toml").write_text(
            '[package]\nname="core"\nversion="1.2.4"\n', encoding="utf-8"
        )
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "Cargo"):
            self.validate("v1.2.3", self.source)


class ManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_contract()

    def test_artifact_and_strict_manifest_identity(self) -> None:
        with tempfile.TemporaryDirectory() as directory_name:
            directory = Path(directory_name)
            artifact = directory / self.contract.ARTIFACT_NAME
            manifest = directory / self.contract.MANIFEST_NAME
            artifact.write_bytes(b"canonical wasm")
            digest = self.contract.create_manifest(artifact, manifest)
            self.assertEqual(self.contract.verify_bundle(directory), digest)
            self.assertEqual(manifest.read_text(encoding="ascii"), f"{digest}  pomodorough_core.wasm\n")
            artifact.write_bytes(b"different artifact")
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "mismatch"):
                self.contract.verify_bundle(directory)

    def test_rejects_noncanonical_manifests_and_extra_assets(self) -> None:
        invalid_lines = (
            f"{'a' * 64} *pomodorough_core.wasm\n",
            f"{'A' * 64}  pomodorough_core.wasm\n",
            f"{'a' * 64}  ./pomodorough_core.wasm\n",
            f"{'a' * 64}  pomodorough_core.wasm\nextra\n",
        )
        for line in invalid_lines:
            with self.subTest(line=line), tempfile.TemporaryDirectory() as directory_name:
                directory = Path(directory_name)
                (directory / self.contract.ARTIFACT_NAME).write_bytes(b"artifact")
                manifest = directory / self.contract.MANIFEST_NAME
                manifest.write_text(line, encoding="ascii")
                with self.assertRaisesRegex(self.contract.ReleaseContractError, "strict"):
                    self.contract.verify_bundle(directory)
        with tempfile.TemporaryDirectory() as directory_name:
            directory = Path(directory_name)
            artifact = directory / self.contract.ARTIFACT_NAME
            artifact.write_bytes(b"artifact")
            self.contract.create_manifest(artifact, directory / self.contract.MANIFEST_NAME)
            (directory / "unexpected.txt").write_text("no", encoding="utf-8")
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "entries differ"):
                self.contract.verify_bundle(directory)


class WorkflowContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_contract()
        self.release = RELEASE_WORKFLOW.read_text(encoding="utf-8")
        self.ci = CI_WORKFLOW.read_text(encoding="utf-8")

    def test_current_workflows_satisfy_contract_and_action_pins(self) -> None:
        self.contract.validate_release_workflow(self.release)
        self.contract.validate_ci_workflow(self.ci)
        self.contract.validate_action_pins(ROOT / ".github" / "workflows")

    def test_rejects_publication_moved_before_draft_verification(self) -> None:
        marker = "      - name: Publish ID-bound verified draft\n"
        prefix, final_step = self.release.split(marker, 1)
        insertion = prefix.index("      - name: Verify ID-bound draft assets")
        mutated = prefix[:insertion] + marker + final_step + prefix[insertion:]
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "out of order"):
            self.contract.validate_release_workflow(mutated)

    def test_rejects_mutable_action_and_inexact_attestation_binding(self) -> None:
        mutable = self.release.replace(
            "actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
            "actions/checkout@v4",
            1,
        )
        with tempfile.TemporaryDirectory() as directory_name:
            workflow = Path(directory_name) / "release.yml"
            workflow.write_text(mutable, encoding="utf-8")
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "mutable"):
                self.contract.validate_action_pins(workflow.parent)
        inexact = self.release.replace(
            '--source-digest "$GITHUB_WORKFLOW_SHA"',
            '--source-digest "$GITHUB_SHA"',
        )
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "lacks required"):
            self.contract.validate_release_workflow(inexact)

    def test_rejects_non_draft_or_overwrite_release_behavior(self) -> None:
        non_draft = self.release.replace("            --draft \\", "", 1)
        with self.assertRaises(self.contract.ReleaseContractError):
            self.contract.validate_release_workflow(non_draft)
        overwrite = self.release + '\n      - run: gh release upload "$GITHUB_REF_NAME" --clobber\n'
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "unsafe"):
            self.contract.validate_release_workflow(overwrite)
        tag_publish = self.release.replace(
            "python3 scripts/c5_release_contract.py publish-verified-draft",
            'gh release edit "$GITHUB_REF_NAME" --draft=false',
            1,
        )
        with self.assertRaises(self.contract.ReleaseContractError):
            self.contract.validate_release_workflow(tag_publish)

    def test_rejects_weakened_ci(self) -> None:
        for required in (
            "pull_request:",
            "cargo +1.97.1 test --all-targets --locked",
            "cargo +1.97.1 clippy --all-targets --locked -- -D warnings",
            "python3 scripts/verify_wasm_artifact.py",
        ):
            with self.subTest(required=required):
                weakened = self.ci.replace(required, "removed", 1)
                with self.assertRaisesRegex(self.contract.ReleaseContractError, "CI workflow"):
                    self.contract.validate_ci_workflow(weakened)

    def test_rejects_conditions_on_required_jobs_or_steps(self) -> None:
        mutations = (
            self.release.replace(
                "  verify-publish:\n",
                "  verify-publish:\n    if: false\n",
                1,
            ),
            self.release.replace(
                "      - name: Publish ID-bound verified draft\n",
                "      - name: Publish ID-bound verified draft\n        if: false\n",
                1,
            ),
        )
        for mutated in mutations:
            with self.subTest(kind="release"), self.assertRaisesRegex(
                self.contract.ReleaseContractError, "must not have an if condition"
            ):
                self.contract.validate_release_workflow(mutated)
        ci_mutations = (
            self.ci.replace("  rust:\n", "  rust:\n    if: false\n", 1),
            self.ci.replace(
                "      - name: Test all targets\n",
                "      - name: Test all targets\n        if: false\n",
                1,
            ),
        )
        for mutated in ci_mutations:
            with self.subTest(kind="ci"), self.assertRaisesRegex(
                self.contract.ReleaseContractError, "must not have an if condition"
            ):
                self.contract.validate_ci_workflow(mutated)

    def test_rejects_header_and_job_configuration_mutations(self) -> None:
        mutations = (
            (self.ci.replace("  pull_request:", "  disabled_pull_request:", 1), "ci"),
            (
                self.release.replace("  contents: read", "  contents: write", 1),
                "release",
            ),
            (
                self.ci.replace("    runs-on: ubuntu-24.04", "    runs-on: ubuntu-latest", 1),
                "ci",
            ),
        )
        for mutated, kind in mutations:
            mutated += "\n# preserved lookalike: pull_request: contents: read ubuntu-24.04\n"
            validator = (
                self.contract.validate_ci_workflow
                if kind == "ci"
                else self.contract.validate_release_workflow
            )
            with self.subTest(kind=kind), self.assertRaises(self.contract.ReleaseContractError):
                validator(mutated)

    def test_rejects_continue_on_error_for_required_jobs_and_steps(self) -> None:
        mutations = (
            self.release.replace(
                "  verify-publish:\n", "  verify-publish:\n    continue-on-error: true\n", 1
            ),
            self.release.replace(
                "      - name: Publish ID-bound verified draft\n",
                "      - name: Publish ID-bound verified draft\n        continue-on-error: true\n",
                1,
            ),
        )
        for mutated in mutations:
            with self.subTest(), self.assertRaisesRegex(
                self.contract.ReleaseContractError, "must not continue on error"
            ):
                self.contract.validate_release_workflow(mutated)

    def test_rejects_shell_gate_bypasses(self) -> None:
        mutations = (
            self.ci.replace(
                "run: cargo +1.97.1 test --all-targets --locked",
                "run: cargo +1.97.1 test --all-targets --locked || true",
                1,
            ),
            self.ci.replace(
                "run: cargo +1.97.1 test --all-targets --locked",
                "run: cargo +1.97.1 test --all-targets --locked | cat",
                1,
            ),
            self.release.replace(
                "python3 scripts/c5_release_contract.py publish-verified-draft \\",
                "python3 scripts/c5_release_contract.py publish-verified-draft; true \\",
                1,
            ),
            self.release.replace(
                "          set -euo pipefail\n          python3 scripts/c5_release_contract.py publish-verified-draft",
                "          set +e\n          python3 scripts/c5_release_contract.py publish-verified-draft",
                1,
            ),
        )
        for mutated in mutations:
            validator = (
                self.contract.validate_ci_workflow
                if mutated.startswith("name: CI")
                else self.contract.validate_release_workflow
            )
            with self.subTest(), self.assertRaises(self.contract.ReleaseContractError):
                validator(mutated)

    def test_rejects_replaced_action_and_create_adopt_branches(self) -> None:
        replaced_action = self.ci.replace(
            "        uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4",
            "        run: echo actions/checkout@11d5960a326750d5838078e36cf38b85af677262",
            1,
        )
        with self.assertRaises(self.contract.ReleaseContractError):
            self.contract.validate_ci_workflow(replaced_action)
        for before, after in (("            create)", "            disabled)"), ("            adopt) ;;", "            adopt) exit 0 ;;")):
            mutated = self.release.replace(before, after, 1) + f"\n# {before.strip()}\n"
            with self.subTest(branch=before), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(mutated)

    def test_rejects_spoofed_action_inputs_and_permissions(self) -> None:
        mutations = (
            self.release.replace(
                "          ref: ${{ github.workflow_sha }}",
                "          ref: refs/heads/main # ref: ${{ github.workflow_sha }}",
                1,
            ),
            self.release.replace(
                "          overwrite: false",
                "          overwrite: true # overwrite: false",
                1,
            ),
            self.release.replace(
                "      id-token: write",
                "      id-token: read # id-token: write",
                1,
            ),
            self.ci.replace(
                "          retention-days: 7",
                "          retention-days: 1 # retention-days: 7",
                1,
            ),
        )
        for mutated in mutations:
            validator = (
                self.contract.validate_ci_workflow
                if mutated.startswith("name: CI")
                else self.contract.validate_release_workflow
            )
            with self.subTest(), self.assertRaises(self.contract.ReleaseContractError):
                validator(mutated)

    def test_rejects_spoofed_or_injected_step_environment(self) -> None:
        wrong_token = self.release.replace(
            "          GH_TOKEN: ${{ github.token }}",
            "          GH_TOKEN: attacker # GH_TOKEN: ${{ github.token }}",
            1,
        )
        injected = self.release.replace(
            "      - name: Publish ID-bound verified draft\n",
            "      - name: Publish ID-bound verified draft\n"
            "        env:\n"
            "          BASH_ENV: /tmp/disable-gate\n",
            1,
        )
        for mutated in (wrong_token, injected):
            with self.subTest(), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(mutated)

    def test_rejects_replaced_moved_or_disabled_required_steps(self) -> None:
        replaced = self.release.replace(
            "python3 scripts/c5_release_contract.py publish-verified-draft",
            "echo publish-verified-draft",
            1,
        )
        moved = replaced.replace(
            "          set -euo pipefail\n          python3 scripts/c5_release_contract.py validate-source",
            "          set -euo pipefail\n          python3 scripts/c5_release_contract.py publish-verified-draft\n"
            "          python3 scripts/c5_release_contract.py validate-source",
            1,
        )
        wrong_dependency = self.release.replace(
            "    needs: build-test-attest", "    needs: []", 1
        )
        renamed = self.release.replace(
            "      - name: Verify ID-bound draft assets",
            "      - name: Optional draft verification",
            1,
        )
        for mutated in (replaced, moved, wrong_dependency, renamed):
            with self.subTest(), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.validate_release_workflow(mutated)

    def test_rejects_command_lookalike_outside_required_ci_step(self) -> None:
        removed = self.ci.replace(
            "        run: cargo +1.97.1 test --all-targets --locked",
            "        run: echo skipped",
            1,
        )
        moved = removed.replace(
            "        run: cargo +1.97.1 fmt --all -- --check",
            "        run: |\n          cargo +1.97.1 fmt --all -- --check\n"
            "          cargo +1.97.1 test --all-targets --locked",
            1,
        )
        with self.assertRaisesRegex(self.contract.ReleaseContractError, "required step"):
            self.contract.validate_ci_workflow(moved)

    def test_documents_non_atomic_rest_publication_limit(self) -> None:
        self.assertIn("no conditional release PATCH", self.contract.PUBLICATION_RESIDUAL)
        self.assertIn("immediately before and after publication", self.contract.PUBLICATION_RESIDUAL)
        self.assertIn("already-public release", self.contract.PUBLICATION_RESIDUAL)


class ReleaseStateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.contract = load_contract()
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.repository = "example/core"
        self.tag = "v1.2.3"
        self.source = "a" * 40
        self.run_id = 900
        self.run_attempt = 1
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        artifact = self.bundle / self.contract.ARTIFACT_NAME
        artifact.write_bytes(b"canonical wasm")
        self.contract.create_manifest(artifact, self.bundle / self.contract.MANIFEST_NAME)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def result(self, returncode: int, stdout: str = "", stderr: str = ""):
        return subprocess.CompletedProcess(["gh"], returncode, stdout, stderr)

    def bytes_result(self, contents: bytes):
        return subprocess.CompletedProcess(["gh"], 0, contents, b"")

    def identity(self, attempt: int = 1) -> dict[str, object]:
        return self.contract._workflow_identity(
            self.repository,
            self.tag,
            self.source,
            self.source,
            self.run_id,
            attempt,
        )

    def body(self, attempt: int = 1) -> str:
        return self.contract.draft_release_body(self.identity(attempt))

    def body_with(self, **updates: object) -> str:
        identity = self.identity()
        identity.update(updates)
        return self.contract.draft_release_body(identity)

    def tag_result(self, source: str | None = None):
        value = source or self.source
        return self.result(0, stdout=json.dumps({"object": {"type": "commit", "sha": value}}))

    def immutable_result(self, enabled: bool = True):
        value = {"enabled": enabled, "enforced_by_owner": False}
        return self.result(0, stdout=json.dumps(value))

    def asset(self, name: str, asset_id: int) -> dict[str, object]:
        contents = (self.bundle / name).read_bytes()
        return {
            "id": asset_id,
            "name": name,
            "state": "uploaded",
            "size": len(contents),
            "digest": f"sha256:{hashlib.sha256(contents).hexdigest()}",
            "created_at": "2026-08-31T12:00:00Z",
            "updated_at": "2026-08-31T12:00:01Z",
        }

    def assets(self) -> list[dict[str, object]]:
        return [
            self.asset(self.contract.ARTIFACT_NAME, 101),
            self.asset(self.contract.MANIFEST_NAME, 102),
        ]

    def release(
        self,
        *,
        release_id: int = 7,
        tag: str | None = None,
        draft: bool = True,
        prerelease: bool = False,
        immutable: bool | None = None,
        updated_at: str = "2026-08-31T12:00:02Z",
        assets: list[dict[str, object]] | None = None,
        body: str | None = None,
    ) -> dict[str, object]:
        release_tag = tag or self.tag
        return {
            "id": release_id,
            "node_id": f"R_{release_id}",
            "tag_name": release_tag,
            "name": release_tag,
            "body": self.body() if body is None else body,
            "draft": draft,
            "prerelease": prerelease,
            "immutable": not draft if immutable is None else immutable,
            "created_at": "2026-08-31T12:00:00Z",
            "updated_at": updated_at,
            "published_at": None if draft else "2026-08-31T12:00:03Z",
            "assets": self.assets() if assets is None else assets,
        }

    def list_result(self, *releases: dict[str, object]):
        return self.result(0, stdout=json.dumps([list(releases)]))

    def object_result(self, value: dict[str, object]):
        return self.result(0, stdout=json.dumps(value))

    def download(self, arguments: list[str]):
        asset_id = int(arguments[-1].rsplit("/", 1)[1])
        asset = next(asset for asset in self.assets() if asset["id"] == asset_id)
        return self.bytes_result((self.bundle / str(asset["name"])).read_bytes())

    def write_seal(self, release: dict[str, object]) -> Path:
        normalized = self.contract._normalize_release(release, self.tag)
        workflow = self.contract._parse_draft_body(str(normalized["body"]))
        seal = self.contract._release_seal(normalized, self.bundle, workflow)
        path = self.root / "release-seal.json"
        path.write_text(json.dumps(seal), encoding="utf-8")
        return path

    def capture(self, download: Path, seal: Path) -> dict[str, object]:
        return self.contract.capture_draft_release(
            self.repository,
            self.tag,
            self.source,
            self.source,
            self.run_id,
            self.run_attempt,
            self.bundle,
            download,
            seal,
        )

    def publish(self, seal: Path, attempt: int = 1) -> dict[str, object]:
        return self.contract.publish_verified_draft(
            self.repository,
            self.tag,
            self.source,
            self.source,
            self.run_id,
            attempt,
            self.bundle,
            seal,
        )

    def publish_responses(
        self, draft: dict[str, object], published: dict[str, object]
    ) -> list[subprocess.CompletedProcess[str]]:
        return [
            self.tag_result(),
            self.list_result(draft),
            self.object_result(draft),
            self.immutable_result(),
            self.tag_result(),
            self.object_result(published),
            self.tag_result(),
            self.immutable_result(),
            self.list_result(published),
            self.object_result(published),
        ]

    def prepare(
        self,
        download: Path,
        body: Path,
        seal: Path,
        attempt: int = 1,
    ) -> str:
        return self.contract.prepare_draft_release(
            self.repository,
            self.tag,
            self.source,
            self.source,
            self.run_id,
            attempt,
            self.bundle,
            download,
            body,
            seal,
        )

    def test_missing_release_uses_paginated_draft_aware_list(self) -> None:
        missing = self.list_result()
        with mock.patch.object(self.contract, "_run_gh", return_value=missing) as run:
            self.contract.require_missing_release(self.repository, self.tag)
        self.assertEqual(
            run.call_args.args[0],
            ["api", "--paginate", "--slurp", "repos/example/core/releases?per_page=100"],
        )
        network_error = self.result(1, stderr="connection refused")
        with mock.patch.object(self.contract, "_run_gh", return_value=network_error):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "failed closed"):
                self.contract.require_missing_release(self.repository, self.tag)

    def test_immutable_release_configuration_is_exact_and_fail_closed(self) -> None:
        with mock.patch.object(
            self.contract, "_run_gh", return_value=self.immutable_result()
        ) as run:
            self.contract.require_immutable_releases(self.repository)
        self.assertEqual(
            run.call_args.args[0],
            [
                "api",
                "-H",
                self.contract.API_VERSION_HEADER,
                "repos/example/core/immutable-releases",
            ],
        )
        invalid = (
            self.immutable_result(False),
            self.result(0, stdout='{"enabled":true}'),
            self.result(0, stdout='{"enabled":true,"enforced_by_owner":false,"extra":1}'),
            self.result(1, stderr="unavailable"),
        )
        for response in invalid:
            with self.subTest(response=response), mock.patch.object(
                self.contract, "_run_gh", return_value=response
            ), self.assertRaises(self.contract.ReleaseContractError):
                self.contract.require_immutable_releases(self.repository)

    def test_remote_lightweight_and_annotated_tags_peel_to_exact_source(self) -> None:
        source = "a" * 40
        tag_object = "b" * 40
        lightweight = self.result(
            0, stdout=f'{{"object":{{"type":"commit","sha":"{source}"}}}}'
        )
        with mock.patch.object(self.contract, "_run_gh", return_value=lightweight):
            self.contract.require_remote_tag_source(self.repository, self.tag, source)
        annotated = [
            self.result(0, stdout=f'{{"object":{{"type":"tag","sha":"{tag_object}"}}}}'),
            self.result(0, stdout=f'{{"object":{{"type":"commit","sha":"{source}"}}}}'),
        ]
        with mock.patch.object(self.contract, "_run_gh", side_effect=annotated):
            self.contract.require_remote_tag_source(self.repository, self.tag, source)

    def test_remote_moved_or_unavailable_tag_fails_closed(self) -> None:
        source = "a" * 40
        moved_source = "b" * 40
        moved = self.result(
            0, stdout=f'{{"object":{{"type":"commit","sha":"{moved_source}"}}}}'
        )
        with mock.patch.object(self.contract, "_run_gh", return_value=moved):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "does not peel"):
                self.contract.require_remote_tag_source(self.repository, self.tag, source)
        unavailable = self.result(1, stderr="connection refused")
        with mock.patch.object(self.contract, "_run_gh", return_value=unavailable):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "failed closed"):
                self.contract.require_remote_tag_source(self.repository, self.tag, source)

    def test_existing_draft_or_public_release_is_never_overwritten(self) -> None:
        for existing in (self.release(), self.release(draft=False)):
            with self.subTest(draft=existing["draft"]), mock.patch.object(
                self.contract, "_run_gh", return_value=self.list_result(existing)
            ):
                with self.assertRaisesRegex(self.contract.ReleaseContractError, "refusing overwrite"):
                    self.contract.require_missing_release(self.repository, self.tag)

    def test_duplicate_drafts_and_published_same_tag_are_rejected(self) -> None:
        first = self.release(release_id=7)
        second = self.release(release_id=8)
        with mock.patch.object(
            self.contract, "_run_gh", return_value=self.list_result(first, second)
        ):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "exactly one"):
                self.contract.require_draft_release(self.repository, self.tag)
        published = self.release(draft=False)
        with mock.patch.object(
            self.contract,
            "_run_gh",
            side_effect=[self.list_result(published), self.object_result(published)],
        ):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "unpublished draft"):
                self.contract.require_draft_release(self.repository, self.tag)

    def test_exact_draft_is_selected_and_bound_to_id(self) -> None:
        draft = self.release()
        responses = [self.list_result(draft), self.object_result(draft)]
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses):
            self.assertEqual(
                self.contract.require_draft_release(self.repository, self.tag, 7)["id"], 7
            )
        responses = [self.list_result(draft), self.object_result(draft)]
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "ID changed"):
                self.contract.require_draft_release(self.repository, self.tag, 8)

    def test_release_change_between_list_and_id_lookup_is_rejected(self) -> None:
        listed = self.release()
        changed = self.release(updated_at="2026-08-31T12:00:09Z")
        with mock.patch.object(
            self.contract,
            "_run_gh",
            side_effect=[self.list_result(listed), self.object_result(changed)],
        ):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "between"):
                self.contract.require_draft_release(self.repository, self.tag)

    def test_prepare_missing_release_writes_exact_identity_body_only(self) -> None:
        body = self.root / "body.txt"
        download = self.root / "download"
        seal = self.root / "seal.json"
        with mock.patch.object(self.contract, "_run_gh", return_value=self.list_result()):
            self.assertEqual(self.prepare(download, body, seal), "create")
        self.assertEqual(body.read_text(encoding="utf-8"), self.body() + "\n")
        self.assertFalse(download.exists())
        self.assertFalse(seal.exists())

    def test_prepare_adopts_exact_prior_attempt_draft_by_id_and_bytes(self) -> None:
        draft = self.release(body=self.body(1))
        responses = [self.list_result(draft), self.list_result(draft), self.object_result(draft)]
        download = self.root / "download"
        seal = self.root / "seal.json"
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), mock.patch.object(
            self.contract, "_run_gh_bytes", side_effect=self.download
        ):
            self.assertEqual(self.prepare(download, self.root / "body", seal, 2), "adopt")
        sealed = json.loads(seal.read_text(encoding="utf-8"))
        self.assertEqual(sealed["release"]["id"], 7)
        self.assertEqual(sealed["workflow"], self.identity(1))
        self.assertEqual(self.contract.verify_bundle(download), self.contract.verify_bundle(self.bundle))

    def test_prepare_rejects_duplicate_public_or_foreign_draft(self) -> None:
        variants = (
            [self.release(), self.release(release_id=8)],
            [self.release(draft=False)],
            [self.release(body="unsealed")],
            [self.release(body=self.body().replace(self.source, "b" * 40))],
            [self.release(body=self.body(2))],
        )
        for records in variants:
            responses = [self.list_result(*records)]
            if len(records) == 1:
                responses.extend([self.list_result(*records), self.object_result(records[0])])
            with self.subTest(records=records), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ), self.assertRaises(self.contract.ReleaseContractError):
                self.prepare(self.root / "download", self.root / "body", self.root / "seal")

    def test_prepare_rejects_list_to_id_or_output_path_mutation(self) -> None:
        draft = self.release()
        changed = self.release(updated_at="2026-08-31T12:00:09Z")
        responses = [self.list_result(draft), self.list_result(draft), self.object_result(changed)]
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "between"):
                self.prepare(self.root / "download", self.root / "body", self.root / "seal")
        body = self.root / "body"
        body.write_text("occupied", encoding="utf-8")
        with mock.patch.object(self.contract, "_run_gh", return_value=self.list_result()):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "already exists"):
                self.prepare(self.root / "download", body, self.root / "seal")

    def test_prepare_rejects_each_workflow_identity_mutation(self) -> None:
        mutations = (
            {"repository": "other/core"},
            {"tag": "v1.2.4"},
            {"source_sha": "b" * 40},
            {"workflow_sha": "b" * 40},
            {"run_id": self.run_id + 1},
            {"creator_run_attempt": self.run_attempt + 1},
        )
        for index, mutation in enumerate(mutations):
            draft = self.release(body=self.body_with(**mutation))
            responses = [self.list_result(draft), self.list_result(draft), self.object_result(draft)]
            seal = self.root / f"identity-{index}.json"
            with self.subTest(mutation=mutation), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ), self.assertRaises(self.contract.ReleaseContractError):
                self.prepare(self.root / f"download-{index}", self.root / f"body-{index}", seal)
            self.assertFalse(seal.exists())

    def test_prepare_rejects_asset_or_download_mutation_without_seal(self) -> None:
        changed_assets = [dict(asset) for asset in self.assets()]
        changed_assets[0]["digest"] = f"sha256:{'b' * 64}"
        draft = self.release(body=self.body(1), assets=changed_assets)
        responses = [self.list_result(draft), self.list_result(draft), self.object_result(draft)]
        seal = self.root / "asset-mutation.json"
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), self.assertRaises(
            self.contract.ReleaseContractError
        ):
            self.prepare(self.root / "asset-download", self.root / "asset-body", seal, 2)
        self.assertFalse(seal.exists())
        draft = self.release(body=self.body(1))
        responses = [self.list_result(draft), self.list_result(draft), self.object_result(draft)]
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), mock.patch.object(
            self.contract, "_run_gh_bytes", return_value=self.bytes_result(b"mutated")
        ), self.assertRaises(self.contract.ReleaseContractError):
            self.prepare(self.root / "byte-download", self.root / "byte-body", seal, 2)
        self.assertFalse(seal.exists())

    def test_new_draft_capture_downloads_assets_by_id_and_seals_identity(self) -> None:
        draft = self.release()
        responses = [self.list_result(draft), self.object_result(draft)]
        downloaded = self.root / "downloaded"
        seal_path = self.root / "release-seal.json"
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), mock.patch.object(
            self.contract, "_run_gh_bytes", side_effect=self.download
        ) as download:
            seal = self.capture(downloaded, seal_path)
        self.assertEqual(seal["release"]["id"], 7)
        self.assertEqual(seal["workflow"], self.identity())
        self.assertEqual(self.contract.verify_bundle(downloaded), self.contract.verify_bundle(self.bundle))
        self.assertEqual(len(download.call_args_list), 2)
        self.assertEqual(json.loads(seal_path.read_text(encoding="utf-8")), seal)

    def test_changed_release_id_or_update_timestamp_rejects_publication(self) -> None:
        draft = self.release()
        seal_path = self.write_seal(draft)
        variants = (
            self.release(release_id=8),
            self.release(updated_at="2026-08-31T12:00:09Z"),
        )
        for current in variants:
            responses = [self.tag_result(), self.list_result(current), self.object_result(current)]
            with self.subTest(current=current), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ):
                with self.assertRaisesRegex(self.contract.ReleaseContractError, "changed"):
                    self.publish(seal_path)

    def test_asset_replacement_extra_and_missing_assets_reject_publication(self) -> None:
        draft = self.release()
        seal_path = self.write_seal(draft)
        replacement = [dict(asset) for asset in self.assets()]
        replacement[0]["id"] = 999
        variants = (
            self.release(assets=replacement),
            self.release(assets=self.assets() + [self.asset("SHA256SUMS", 999)]),
            self.release(assets=self.assets()[:1]),
        )
        for current in variants:
            responses = [self.tag_result(), self.list_result(current), self.object_result(current)]
            with self.subTest(assets=current["assets"]), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ):
                with self.assertRaises(self.contract.ReleaseContractError):
                    self.publish(seal_path)

    def test_tag_source_drift_rejects_before_release_lookup(self) -> None:
        seal_path = self.write_seal(self.release())
        moved = self.result(
            0, stdout=json.dumps({"object": {"type": "commit", "sha": "b" * 40}})
        )
        with mock.patch.object(self.contract, "_run_gh", return_value=moved) as run:
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "does not peel"):
                self.publish(seal_path)
        self.assertEqual(len(run.call_args_list), 1)

    def test_publication_revalidates_bytes_then_patches_exact_release_id(self) -> None:
        draft = self.release()
        published = self.release(draft=False, updated_at="2026-08-31T12:00:04Z")
        seal_path = self.write_seal(draft)
        responses = self.publish_responses(draft, published)
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses) as run, mock.patch.object(
            self.contract, "_run_gh_bytes", side_effect=self.download
        ) as download:
            final = self.publish(seal_path)
        patch = run.call_args_list[5].args[0]
        self.assertEqual(
            patch,
            ["api", "--method", "PATCH", "repos/example/core/releases/7", "-F", "draft=false"],
        )
        self.assertEqual(final["id"], 7)
        self.assertEqual(len(download.call_args_list), 4)
        calls = [call.args[0] for call in run.call_args_list]
        tag = ["api", "repos/example/core/git/ref/tags/v1.2.3"]
        immutable = [
            "api",
            "-H",
            self.contract.API_VERSION_HEADER,
            "repos/example/core/immutable-releases",
        ]
        self.assertEqual(calls[0], tag)
        self.assertEqual(calls[3:8], [immutable, tag, patch, tag, immutable])

    def test_prior_attempt_seal_publishes_but_future_attempt_fails_closed(self) -> None:
        draft = self.release(body=self.body(1))
        published = self.release(
            body=self.body(1), draft=False, updated_at="2026-08-31T12:00:04Z"
        )
        with mock.patch.object(
            self.contract, "_run_gh", side_effect=self.publish_responses(draft, published)
        ), mock.patch.object(self.contract, "_run_gh_bytes", side_effect=self.download):
            self.assertEqual(self.publish(self.write_seal(draft), attempt=2)["id"], 7)
        future = self.release(body=self.body(2))
        with mock.patch.object(self.contract, "_run_gh") as run, self.assertRaises(
            self.contract.ReleaseContractError
        ):
            self.publish(self.write_seal(future), attempt=1)
        run.assert_not_called()

    def test_final_public_identity_mutation_is_rejected(self) -> None:
        draft = self.release()
        published = self.release(draft=False, updated_at="2026-08-31T12:00:04Z")
        changed = self.release(
            body=self.body_with(run_id=self.run_id + 1),
            draft=False,
            updated_at="2026-08-31T12:00:05Z",
        )
        responses = self.publish_responses(draft, published)[:-2]
        responses.extend([self.list_result(changed), self.object_result(changed)])
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), mock.patch.object(
            self.contract, "_run_gh_bytes", side_effect=self.download
        ), self.assertRaisesRegex(self.contract.ReleaseContractError, "identity differs"):
            self.publish(self.write_seal(draft))

    def test_mutable_publication_response_is_rejected(self) -> None:
        draft = self.release()
        published = self.release(
            draft=False,
            immutable=False,
            updated_at="2026-08-31T12:00:04Z",
        )
        responses = self.publish_responses(draft, published)
        with mock.patch.object(
            self.contract, "_run_gh", side_effect=responses
        ), mock.patch.object(self.contract, "_run_gh_bytes", side_effect=self.download):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "not immutable"):
                self.contract.publish_verified_draft(
                    self.repository,
                    self.tag,
                    self.source,
                    self.source,
                    self.run_id,
                    self.run_attempt,
                    self.bundle,
                    self.write_seal(draft),
                )

    def test_asset_change_after_publish_is_detected(self) -> None:
        draft = self.release()
        published = self.release(draft=False, updated_at="2026-08-31T12:00:04Z")
        changed_assets = [dict(asset) for asset in self.assets()]
        changed_assets[0]["id"] = 999
        changed = self.release(
            draft=False,
            updated_at="2026-08-31T12:00:05Z",
            assets=changed_assets,
        )
        responses = self.publish_responses(draft, published)[:-2]
        responses.extend([self.list_result(changed), self.object_result(changed)])
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses), mock.patch.object(
            self.contract, "_run_gh_bytes", side_effect=self.download
        ):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "assets"):
                self.publish(self.write_seal(draft))

    def test_asset_byte_change_immediately_before_publish_blocks_patch(self) -> None:
        draft = self.release()
        responses = [self.tag_result(), self.list_result(draft), self.object_result(draft)]
        with mock.patch.object(self.contract, "_run_gh", side_effect=responses) as run, mock.patch.object(
            self.contract, "_run_gh_bytes", return_value=self.bytes_result(b"replaced")
        ):
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "bound identity"):
                self.publish(self.write_seal(draft))
        self.assertEqual(len(run.call_args_list), 3)

    def test_prepublication_immutable_or_tag_change_blocks_patch(self) -> None:
        draft = self.release()
        seal = self.write_seal(draft)
        prefixes = (
            [self.tag_result(), self.list_result(draft), self.object_result(draft), self.immutable_result(False)],
            [self.tag_result(), self.list_result(draft), self.object_result(draft), self.immutable_result(), self.tag_result("b" * 40)],
        )
        for responses in prefixes:
            with self.subTest(calls=len(responses)), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ) as run, mock.patch.object(self.contract, "_run_gh_bytes", side_effect=self.download):
                with self.assertRaises(self.contract.ReleaseContractError):
                    self.publish(seal)
            self.assertFalse(any("PATCH" in call.args[0] for call in run.call_args_list))

    def test_postpublication_tag_or_immutable_change_reports_non_cas_race(self) -> None:
        draft = self.release()
        published = self.release(draft=False, updated_at="2026-08-31T12:00:04Z")
        variants = (
            self.publish_responses(draft, published)[:6] + [self.tag_result("b" * 40)],
            self.publish_responses(draft, published)[:7] + [self.immutable_result(False)],
        )
        for responses in variants:
            with self.subTest(calls=len(responses)), mock.patch.object(
                self.contract, "_run_gh", side_effect=responses
            ) as run, mock.patch.object(self.contract, "_run_gh_bytes", side_effect=self.download):
                with self.assertRaisesRegex(
                    self.contract.ReleaseContractError, "may already be public"
                ):
                    self.publish(self.write_seal(draft))
            self.assertEqual(len(run.call_args_list), len(responses))
            self.assertEqual(
                sum("PATCH" in call.args[0] for call in run.call_args_list), 1
            )

    def test_publication_rejects_changed_workflow_seal_identity(self) -> None:
        draft = self.release()
        seal_path = self.write_seal(draft)
        seal = json.loads(seal_path.read_text(encoding="utf-8"))
        seal["workflow"]["run_id"] = self.run_id + 1
        seal_path.write_text(json.dumps(seal), encoding="utf-8")
        with mock.patch.object(self.contract, "_run_gh") as run:
            with self.assertRaisesRegex(self.contract.ReleaseContractError, "workflow identity"):
                self.publish(seal_path)
        run.assert_not_called()


class StrictSemVerTests(unittest.TestCase):
    def test_accepts_only_strict_release_tags(self) -> None:
        contract = load_contract()
        for tag in ("v0.0.0", "v1.2.3", "v10.20.30"):
            with self.subTest(tag=tag):
                self.assertEqual(contract.strict_release_version(tag), tag[1:])
        for tag in ("1.2.3", "v01.2.3", "v1.02.3", "v1.2.03", "v1.2", "v1.2.3-rc.1", "v1.2.3+meta", "v1.2.3\n"):
            with self.subTest(tag=tag), self.assertRaises(contract.ReleaseContractError):
                contract.strict_release_version(tag)


if __name__ == "__main__":
    unittest.main()
