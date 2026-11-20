"""Regenerate fixtures/r55-c01-malformed-envelopes-v1.json.

The fixture pins the three R55-C01 unwind sites:
`workspace.completionMutation.v1` (workspace, previousWorkspace),
`workspace.legacyDependencyPlan.v1` (stored display queues) and
`workspace.legacyPreferences.v1` (retained row identity).

Six seed rows are kept verbatim from the fixture itself, because their inputs
came from outside the repository: four audit reproducers and two accepted
lifecycle requests harvested from tests/completion_lifecycle.rs and
tests/workspace_terminal_checker.rs. Every other row is derived from those
seeds and from fixtures/completion-lifecycle-request-v1.json and
fixtures/legacy-dependencies-v1.json. The native oracle supplies `expected`.
A malformed row must return its site message. A control row was captured from
the build before R55-C01, and the fix kept it byte-identical.

Usage, from pomodorough-core:
    cargo build --example artifact_parity_oracle
    python3 scripts/r55_c01_malformed_envelopes_fixture.py          # rewrite
    python3 scripts/r55_c01_malformed_envelopes_fixture.py --check  # compare
"""
from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "fixtures/r55-c01-malformed-envelopes-v1.json"
ORACLE = ROOT / "target/debug/examples/artifact_parity_oracle"
DESCRIPTION = ("R55-C01: envelopes that made three workspace operations unwind, and controls "
               "whose complete envelopes were captured before the fix.")
COMPLETION = "workspace.completionMutation.v1"
DEPENDENCIES = "workspace.legacyDependencyPlan.v1"
PREFERENCES = "workspace.legacyPreferences.v1"
WORKSPACE, PREVIOUS = "completionWorkspace", "completionPreviousWorkspace"
QUEUES, IDENTITY = "dependencyQueues", "preferenceIdentity"
MESSAGES = {
    WORKSPACE: "invalid shared-core input: workspace must be a JSON object",
    PREVIOUS: "invalid shared-core input: previousWorkspace must be a JSON object",
    QUEUES: "invalid shared-core input: missing raw legacy queue",
    IDENTITY: "invalid shared-core input: invalid retained legacy identity",
}
DOMAINS = ["commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations"]
SHAPES = [("array", []), ("boolean", True), ("number", 7), ("string", "x")]
VALID_AUTO_START_ID = "44444444-4444-4444-8444-444444444444"
FILLER_ID = "66666666-6666-4666-8666-666666666666"
SEEDS = (
    "completion-deferred-workspace-array",
    "completion-deferred-workspace-boolean",
    "dependencies-stored-queues-empty",
    "preferences-auto-start-id-missing",
    "completion-deferred-valid-centralized-opportunity",
    "completion-deferred-valid-iroh-opportunity",
)
COMMAND = {
    "deviceId": "legacy-device", "deviceSequence": 8, "hlcCounter": 3, "hlcWallMs": 1788177600000,
    "id": "legacy-finish", "observedElapsedMs": 0, "occurredAt": "2026-08-31T12:00:00.000Z", "phase": "focus",
    "plannedDurationMs": 1500000, "timerId": "legacy-focus", "type": "finish",
}
DROP = object()


def dump(value: object) -> str:
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


def repository_fixture(name: str) -> dict:
    return json.loads((ROOT / "fixtures" / f"{name}.json").read_text(encoding="utf-8"))


def changed(seed: dict, **fields: object) -> dict:
    item = copy.deepcopy(seed)
    for key, value in fields.items():
        if value is DROP:
            item.pop(key, None)
        else:
            item[key] = value
    return item


class Rows:
    def __init__(self, seeds: dict[str, str]) -> None:
        self.seeds = seeds
        self.rows: list[dict] = []

    def add(self, site: str, kind: str, name: str, operation: str, value: object) -> None:
        self.rows.append({"site": site, "kind": kind, "name": name, "operation": operation, "input": dump(value)})

    def seed(self, site: str, kind: str, name: str, operation: str) -> dict:
        self.rows.append({"site": site, "kind": kind, "name": name, "operation": operation,
                          "input": self.seeds[name]})
        return json.loads(self.seeds[name])


def deferred_workspace_rows(rows: Rows) -> None:
    deferred = rows.seed(WORKSPACE, "malformed", "completion-deferred-workspace-array", COMPLETION)
    rows.seed(WORKSPACE, "malformed", "completion-deferred-workspace-boolean", COMPLETION)
    for label, value in SHAPES[2:]:
        rows.add(WORKSPACE, "malformed", f"completion-deferred-workspace-{label}", COMPLETION,
                 changed(deferred, workspace=value))
    for label, value in [("null", None), ("empty-object", {})]:
        rows.add(WORKSPACE, "control", f"completion-deferred-workspace-{label}", COMPLETION,
                 changed(deferred, workspace=value))
    late = copy.deepcopy(deferred)
    late["clock"]["observedAt"] = "2026-07-20T12:00:16Z"
    rows.add(WORKSPACE, "control", "completion-deferred-workspace-array-late-observation", COMPLETION, late)


def expiry_workspace_rows(rows: Rows) -> None:
    expiry = repository_fixture("completion-lifecycle-request-v1")
    for field, prefix, site in [("workspace", "workspace", WORKSPACE),
                                ("previousWorkspace", "previous-workspace", PREVIOUS)]:
        for label, value in SHAPES:
            rows.add(site, "malformed", f"completion-expiry-{prefix}-{label}", COMPLETION,
                     changed(expiry, **{field: value}))
        for label, value in [("null", None), ("empty-object", {})]:
            rows.add(site, "control", f"completion-expiry-{prefix}-{label}", COMPLETION,
                     changed(expiry, **{field: value}))
    rows.add(WORKSPACE, "control", "completion-expiry-valid", COMPLETION, expiry)


def stored_queue_rows(rows: Rows) -> None:
    dependencies = rows.seed(QUEUES, "malformed", "dependencies-stored-queues-empty", DEPENDENCIES)

    def stored(value: object, profile: str | None = None) -> dict:
        item = copy.deepcopy(dependencies)
        item["workspace"]["displayContext"]["projectionPending"] = value
        if profile:
            item["profile"] = profile
        return item
    rows.add(QUEUES, "malformed", "dependencies-stored-queues-commands-only", DEPENDENCIES, stored({"commands": []}))
    for domain in DOMAINS:
        rows.add(QUEUES, "malformed", f"dependencies-stored-queues-without-{domain}", DEPENDENCIES,
                 stored({name: [] for name in DOMAINS if name != domain}))
    rows.add(QUEUES, "malformed", "dependencies-android-stored-queues-empty", DEPENDENCIES,
             stored({}, "androidCentralized"))
    rows.add(QUEUES, "control", "dependencies-stored-queues-null", DEPENDENCIES, stored(None))
    rows.add(QUEUES, "control", "dependencies-stored-queues-complete", DEPENDENCIES,
             stored({name: [] for name in DOMAINS}))
    rows.add(QUEUES, "control", "dependencies-stored-queues-null-member", DEPENDENCIES,
             stored({name: (None if name == "commands" else []) for name in DOMAINS}))
    stale = stored({})
    stale["ownership"]["expectedOwnerId"] = "account:v1:stale"
    rows.add(QUEUES, "control", "dependencies-stale-owner-stored-queues-empty", DEPENDENCIES, stale)
    receipt = next(row for row in repository_fixture("legacy-dependencies-v1")["receipts"]
                   if row["name"] == "all-five-raw")
    rows.rows.append({"site": QUEUES, "kind": "control", "name": "dependencies-stored-queues-complete-rows",
                      "operation": DEPENDENCIES, "input": receipt["inputRaw"]})


def retained_identity(preferences: dict, domain: str, index: int, mutate, stored_null: bool = False) -> dict:
    item = copy.deepcopy(preferences)
    if domain != "autoStartOperations":
        item["workspace"]["local"]["autoStartOperations"][0]["id"] = VALID_AUTO_START_ID
    queue = item["workspace"]["local"][domain]
    while len(queue) <= index:
        queue.append({"id": FILLER_ID})
    mutate(queue, index)
    if stored_null:
        item["workspace"]["displayContext"]["projectionPending"] = None
    return item


def set_id(value: object):
    return lambda queue, index: queue[index].__setitem__("id", value)


def drop_id(queue: list, index: int) -> None:
    queue[index].pop("id", None)


def empty_row(queue: list, index: int) -> None:
    queue[index] = {}


def retained_identity_rows(rows: Rows) -> None:
    preferences = rows.seed(IDENTITY, "malformed", "preferences-auto-start-id-missing", PREFERENCES)
    local = lambda *arguments: retained_identity(preferences, *arguments)
    for label, mutate in [("id-null", set_id(None)), ("id-boolean", set_id(True)), ("id-number", set_id(7)),
                          ("row-empty-object", empty_row)]:
        rows.add(IDENTITY, "malformed", f"preferences-auto-start-{label}", PREFERENCES,
                 local("autoStartOperations", 0, mutate))
    for domain in ["taskOperations", "durationOperations", "selectedTaskOperations"]:
        rows.add(IDENTITY, "malformed", f"preferences-{domain}-id-missing", PREFERENCES, local(domain, 0, drop_id))
    for name, arguments in [
        ("preferences-auto-start-valid-row", ("autoStartOperations", 0, set_id(VALID_AUTO_START_ID))),
        ("preferences-auto-start-id-empty-string", ("autoStartOperations", 0, set_id(""))),
        ("preferences-stored-null-id-missing", ("autoStartOperations", 0, drop_id, True)),
        ("preferences-command-id-missing", ("commands", 0, drop_id)),
        ("preferences-later-duration-id-missing", ("durationOperations", 1, drop_id)),
    ]:
        rows.add(IDENTITY, "control", name, PREFERENCES, local(*arguments))
    incomplete = copy.deepcopy(preferences)
    incomplete["workspace"]["displayContext"]["projectionPending"] = {"commands": []}
    rows.add(IDENTITY, "control", "preferences-stored-incomplete-id-missing", PREFERENCES, incomplete)


def accepted_lifecycle_rows(rows: Rows) -> tuple[dict, dict, dict]:
    centralized = rows.seed(WORKSPACE, "control", "completion-deferred-valid-centralized-opportunity", COMPLETION)
    iroh = rows.seed(WORKSPACE, "control", "completion-deferred-valid-iroh-opportunity", COMPLETION)
    expiry = repository_fixture("completion-lifecycle-request-v1")
    rows.add(WORKSPACE, "malformed", "completion-deferred-iroh-workspace-array", COMPLETION, changed(iroh, workspace=[]))
    rows.add(WORKSPACE, "malformed", "completion-deferred-workspace-object-text", COMPLETION,
             changed(centralized, workspace=dump(centralized["workspace"])))
    rows.add(WORKSPACE, "malformed", "completion-deferred-workspace-array-of-object", COMPLETION,
             changed(centralized, workspace=[centralized["workspace"]]))
    rows.add(WORKSPACE, "malformed", "completion-expiry-both-workspaces-array", COMPLETION,
             changed(expiry, workspace=[], previousWorkspace=[]))
    rows.add(WORKSPACE, "control", "completion-deferred-workspace-absent", COMPLETION,
             changed(centralized, workspace=DROP))
    with_now = copy.deepcopy(centralized)
    with_now["workspace"]["now"] = "2026-07-20T12:00:00Z"
    rows.add(WORKSPACE, "control", "completion-deferred-workspace-with-now", COMPLETION, with_now)
    return centralized, iroh, expiry


def previous_workspace_rows(rows: Rows, centralized: dict, iroh: dict, expiry: dict) -> None:
    for label, value in SHAPES:
        rows.add(PREVIOUS, "malformed", f"completion-deferred-previous-workspace-{label}", COMPLETION,
                 changed(iroh, previousWorkspace=value))
    rows.add(PREVIOUS, "control", "completion-deferred-previous-workspace-null", COMPLETION,
             changed(iroh, previousWorkspace=None))
    rows.add(PREVIOUS, "control", "completion-deferred-centralized-previous-workspace-array", COMPLETION,
             changed(centralized, previousWorkspace=[]))
    previous_now = copy.deepcopy(expiry)
    previous_now["previousWorkspace"]["now"] = "2026-07-20T12:00:00Z"
    rows.add(PREVIOUS, "control", "completion-expiry-previous-workspace-with-now", COMPLETION, previous_now)


def undecorated_queue_rows(rows: Rows) -> None:
    populated = json.loads(next(row["input"] for row in rows.rows
                                if row["name"] == "dependencies-stored-queues-complete-rows"))
    for profile, name in [("pwaStorage", "pwa"), ("androidCentralized", "android")]:
        item = copy.deepcopy(populated)
        item["profile"] = profile
        for queues in (item["workspace"]["local"], item["workspace"]["displayContext"]["projectionPending"]):
            for domain in DOMAINS:
                for operation in queues[domain]:
                    if domain != "commands" or profile == "androidCentralized":
                        operation.pop("deviceId", None)
        rows.add(QUEUES, "control", f"dependencies-{name}-rows-without-device-identity", DEPENDENCIES, item)


def later_identity_rows(rows: Rows) -> None:
    preferences = json.loads(next(row["input"] for row in rows.rows
                                  if row["name"] == "preferences-auto-start-valid-row"))

    def auto_start(value: object) -> dict:
        item = copy.deepcopy(preferences)
        item["workspace"]["local"]["autoStartOperations"][0]["id"] = value
        return item
    rows.add(IDENTITY, "malformed", "preferences-auto-start-id-array", PREFERENCES, auto_start([]))
    rows.add(IDENTITY, "malformed", "preferences-auto-start-id-object", PREFERENCES, auto_start({}))
    # The selector decodes commands in order and reads no identity value, so an
    # empty command identity must not hide the decode error of the next row.
    empty = copy.deepcopy(preferences)
    empty["workspace"]["local"]["commands"] = [{**COMMAND, "id": ""}, {}]
    rows.add(IDENTITY, "control", "preferences-command-id-empty-string-before-undecodable-command", PREFERENCES, empty)
    # A later row is reached once the selector passes the first row: stored
    # membership skips it, or never-sent proof above a null head admits it.
    member = copy.deepcopy(preferences)
    durations = member["workspace"]["local"]["durationOperations"]
    member["workspace"]["displayContext"]["projectionPending"]["durationOperations"] = [copy.deepcopy(durations[0])]
    del durations[1]["id"]
    rows.add(IDENTITY, "malformed", "preferences-later-duration-id-missing-after-stored-row", PREFERENCES, member)
    admitted = copy.deepcopy(preferences)
    admitted["workspace"]["canonicalHead"] = None
    del admitted["workspace"]["local"]["durationOperations"][1]["id"]
    rows.add(IDENTITY, "malformed", "preferences-later-duration-id-missing-after-admitted-row", PREFERENCES, admitted)


def build(seeds: dict[str, str]) -> list[dict]:
    """Row order: the fixer's rows, then the R55-C01 test writer's rows."""
    rows = Rows(seeds)
    deferred_workspace_rows(rows)
    expiry_workspace_rows(rows)
    stored_queue_rows(rows)
    retained_identity_rows(rows)
    previous_workspace_rows(rows, *accepted_lifecycle_rows(rows))
    undecorated_queue_rows(rows)
    later_identity_rows(rows)
    return rows.rows


def envelopes(oracle: Path, rows: list[dict]) -> list[str]:
    lines = "".join(json.dumps({"operation": row["operation"], "input": row["input"]}) + "\n" for row in rows)
    result = subprocess.run([str(oracle)], input=lines, capture_output=True, text=True, check=True)
    output = result.stdout.split("\n")
    if output[-1] != "" or len(output) != len(rows) + 1:
        raise ValueError(f"oracle returned {len(output) - 1} lines for {len(rows)} rows")
    return output[:-1]


def render(oracle: Path, seeds: dict[str, str]) -> str:
    rows = build(seeds)
    if len({row["name"] for row in rows}) != len(rows):
        raise ValueError("duplicate row name")
    for row, envelope in zip(rows, envelopes(oracle, rows)):
        if row["kind"] == "malformed" and envelope != dump({"error": MESSAGES[row["site"]], "ok": False}):
            raise ValueError(f"{row['name']}: {envelope}")
        row["expected"] = envelope
    return json.dumps({"description": DESCRIPTION, "rows": rows}, indent=1, ensure_ascii=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="fail when the fixture differs instead of rewriting it")
    parser.add_argument("--oracle", type=Path, default=ORACLE, help="native artifact_parity_oracle binary")
    arguments = parser.parse_args()
    current = FIXTURE.read_text(encoding="utf-8")
    seeds = {row["name"]: row["input"] for row in json.loads(current)["rows"] if row["name"] in SEEDS}
    if len(seeds) != len(SEEDS):
        raise ValueError(f"missing seed rows: {sorted(set(SEEDS) - set(seeds))}")
    text = render(arguments.oracle, seeds)
    if arguments.check:
        if text != current:
            print(f"{FIXTURE.relative_to(ROOT)} differs from its generator", file=sys.stderr)
            return 1
        print(f"{FIXTURE.relative_to(ROOT)} matches its generator")
        return 0
    FIXTURE.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
