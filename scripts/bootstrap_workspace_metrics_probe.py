#!/usr/bin/env python3
"""Compare bootstrap metrics with a pre-task report without refreshing snapshots."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
sys.path.insert(0, str(SUITE / "pomodorough-helpers/scripts"))
from complexity_report import braced_complexity, braced_declarations, source_files  # noqa: E402

ADDITIONS = {
    "src/bootstrap.rs": "pub(crate) mod workspace;\n\n",
    "src/lib.rs": '        "bootstrap.workspacePlan.v1" => bootstrap::workspace::plan_json(input),\n',
    "src/reconciliation/workspace.rs": "\npub(crate) mod bootstrap;\n",
}
NEW_SOURCES = {"src/bootstrap/workspace.rs", "src/bootstrap/workspace/classification.rs",
    "src/bootstrap/workspace/horizon.rs", "src/bootstrap/workspace/pwa.rs",
    "src/bootstrap/workspace/validation.rs", "src/reconciliation/workspace/bootstrap.rs",
    "examples/bootstrap_workspace_probe.rs"}


def metrics(source, suffix):
    return [(item.name, item.kind, item.end_line - item.start_line + 1,
             *braced_complexity(item.source, suffix == ".js"))
            for item in braced_declarations(source, suffix)]


def inspect():
    digest = hashlib.sha256()
    changes, additions = [], []
    for path in source_files(ROOT, False):
        relative = path.relative_to(ROOT).as_posix()
        source = path.read_text()
        current = metrics(source, path.suffix)
        if relative in NEW_SOURCES:
            additions.extend(dict(file=relative, name=row[0], lines=row[2], cyclomatic=row[3], cognitive=row[4])
                             for row in current)
            assert all(row[2] <= 50 for row in current), (relative, current)
            continue
        previous = source
        if relative in ADDITIONS:
            assert previous.count(ADDITIONS[relative]) == 1, relative
            previous = previous.replace(ADDITIONS[relative], "", 1)
        if relative == "src/reconciliation/workspace.rs":
            previous = previous.replace("pub(crate) fn validate_shape(", "fn validate_shape(", 1)
        digest.update(path.relative_to(SUITE).as_posix().encode())
        digest.update(b"\0")
        digest.update(previous.encode())
        baseline = metrics(previous, path.suffix)
        assert len(baseline) == len(current), relative
        for before, after in zip(baseline, current):
            assert before[:2] == after[:2] and before[3:] == after[3:], (relative, before, after)
            if before[2] != after[2]:
                changes.append(dict(file=relative, name=before[0], lineDelta=after[2] - before[2],
                                    cyclomaticDelta=0, cognitiveDelta=0))
    return digest.hexdigest(), changes, additions


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    args = parser.parse_args()
    before = json.loads(args.before.read_text())["projects"]["pomodorough-core"]
    fingerprint, changes, additions = inspect()
    assert fingerprint == before["source_sha256"], "Pre-task source differs beyond bootstrap additions"
    assert changes == [dict(file="src/lib.rs", name="dispatch_json", lineDelta=1,
                           cyclomaticDelta=0, cognitiveDelta=0)], changes
    print(json.dumps(dict(baselinePreserved=True, existingChanges=changes, additions=additions), sort_keys=True))


if __name__ == "__main__":
    main()
