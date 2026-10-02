#!/usr/bin/env python3
"""Execute Desktop and PWA production methods; pin Apple/Android source predicates.

Partial source parity, not client integration. Core output comes from native bridge.
"""
import ast
import copy
from datetime import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import os
import types

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
FIXTURE = json.loads((ROOT / "fixtures/read-model-v1.json").read_text())
BRIDGE = ROOT / "target/debug/examples/read_model_probe"


def core(request):
    result = subprocess.run([str(BRIDGE)], input=json.dumps({"operation": "workspace.readModel.v1", "input": request}) + "\n",
                            text=True, capture_output=True, check=True)
    value = json.loads(result.stdout)
    assert "error" not in value, value
    return value


def core_task_id():
    result = subprocess.run([str(BRIDGE)], input=json.dumps({"operation": "task.identity.v1", "input": {"title":"Study"}}) + "\n",
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)["id"]


def core_rejects(request):
    result = subprocess.run([str(BRIDGE)], input=json.dumps({"operation": "workspace.readModel.v1", "input": request}) + "\n",
                            text=True, capture_output=True, check=True)
    return "error" in json.loads(result.stdout)


def desktop_methods():
    path = SUITE / "desktop/src/pomodorough/core.py"
    tree = ast.parse(path.read_text())
    names = {"elapsed_ms", "completed_focus_count_for_day", "long_break_progress", "task_summaries_today", "timer_for_display"}
    methods = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    assert len(methods) == len(names)
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0),
                              *methods], type_ignores=[])
    namespace = {"datetime": datetime, "parse_timestamp_ms": lambda value: int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)}
    exec(compile(ast.fix_missing_locations(module), str(path), "exec"), namespace)
    timer_screen = SUITE / "desktop/src/pomodorough/timer_screen.py"
    tree = ast.parse(timer_screen.read_text())
    presentation = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == "presentation")
    presentation.decorator_list = []
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), presentation], type_ignores=[])
    namespace.update(PHASES={"focus": {}, "short_break": {}, "long_break": {}},
        _coerced_durations_ms=lambda settings: settings["durationsMs"],
        _coerced_planned_ms=lambda timer, fallback: timer["plannedDurationMs"],
        TimerRenderState=lambda *args: types.SimpleNamespace(status=args[2], elapsed=args[3], remaining=args[5]))
    exec(compile(ast.fix_missing_locations(module), str(timer_screen), "exec"), namespace)
    controller = SUITE / "desktop/src/pomodorough/timer_interaction_controller.py"
    tree = ast.parse(controller.read_text())
    action = next(node for node in ast.walk(tree) if isinstance(node, ast.FunctionDef) and node.name == "primary_action")
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), action], type_ignores=[])
    namespace.update(TERMINAL_STATUSES={"completed", "cancelled", "superseded"},
        done=lambda *effects: effects, LoadState=lambda: None, Render=lambda: None,
        Synchronize=lambda: None)
    exec(compile(ast.fix_missing_locations(module), str(controller), "exec"), namespace)
    return namespace


def desktop_gui_primary(action, status):
    calls = []
    context = types.SimpleNamespace(timer={"status": status},
        settings={"durationsMs": FIXTURE["request"]["source"]["value"]["base"]["durationsMs"]},
        store=types.SimpleNamespace(queue_restart=lambda *args: calls.append("restart")))
    owner = types.SimpleNamespace(_context=lambda: context,
        _current_timer_value=lambda current: current.timer,
        _claim_unfinished_completion=lambda current: current,
        _selected_phase_value=lambda current: "focus",
        _ports=types.SimpleNamespace(issue_command=lambda name, _: calls.append(name),
            mutation_blocked=lambda: False))
    action(owner)
    return calls


def apple_day_focus_totals(history, observed_at):
    source = (SUITE / "apple/Sources/AppStatePublisher.swift").read_text()
    start = source.index("    func dayFocusTotals(")
    method = source[start:source.index("\n    }", start) + len("\n    }")]
    program = '''import Foundation
enum TimerPhase: String, Decodable { case focus, short_break, long_break }
struct HistoryItem: Decodable {
    let phase: TimerPhase
    let status: String
    let completedAt: Date?
    let plannedDurationMs: Int64
}
struct Snapshot { let history: [HistoryItem] }
class SourceProbe {
''' + method + '''
}
var calendar = Calendar(identifier: .gregorian)
calendar.timeZone = TimeZone(identifier: "America/New_York")!
let decoder = JSONDecoder()
decoder.dateDecodingStrategy = .iso8601
let history = try! decoder.decode([HistoryItem].self, from: Data(''' + json.dumps(json.dumps(history)) + '''.utf8))
let reference = ISO8601DateFormatter().date(from: ''' + json.dumps(observed_at) + ''')!
let result = SourceProbe().dayFocusTotals(for: reference, calendar: calendar, snapshot: Snapshot(history: history))
print("\\(result.finishedPomodoros) \\(result.timeSpentMs)")
'''
    run = subprocess.run(["swift", "-"], input=program, text=True,
                         capture_output=True, check=True, timeout=90)
    return tuple(map(int, run.stdout.strip().split()))


def compare_runtime():
    request = copy.deepcopy(FIXTURE["request"])
    request["source"]["value"]["base"]["canonicalTimer"] = FIXTURE["timer"]
    result = core(request)
    desktop = desktop_methods()
    for profile in ("desktopStorage", "desktopTerminal"):
        for row in FIXTURE["desktopPrimary"]:
            case = copy.deepcopy(FIXTURE["request"])
            case["profile"] = profile
            status = row["status"]
            if status != "idle":
                case["source"]["value"]["base"]["canonicalTimer"] = {**FIXTURE["timer"], "status": status}
                if status == "completed":
                    case["source"]["value"]["base"]["canonicalTimer"]["elapsedAtAnchorMs"] = FIXTURE["timer"]["plannedDurationMs"]
            primary = desktop_gui_primary(desktop["primary_action"], status)
            available = core(case)["availableIntents"]
            assert available == row["intents"], (profile, status, available, primary)
            assert primary == (["start"] if status == "idle" else ["restart"])
    elapsed = desktop["elapsed_ms"](FIXTURE["timer"], int(datetime.fromisoformat(request["observedAt"].replace("Z", "+00:00")).timestamp() * 1000))
    assert elapsed == result["canonical"]["elapsedMs"] == 180000
    pwa = json.loads(subprocess.check_output(["node", str(ROOT / "scripts/read_model_probe_pwa.cjs")], text=True))
    assert pwa["elapsed"] == elapsed
    assert pwa["remaining"] == result["canonical"]["remainingMs"]
    assert pwa["progress"] == result["canonical"]["progress"]
    cancelled = FIXTURE["timer"].copy()
    cancelled["status"] = "cancelled"
    request["profile"] = "desktopStorage"
    request["source"]["value"]["base"]["canonicalTimer"] = cancelled
    measured = desktop["presentation"](cancelled, selected_phase="focus",
        settings={"durationsMs": request["source"]["value"]["base"]["durationsMs"]},
        now_ms=int(datetime.fromisoformat(request["observedAt"].replace("Z", "+00:00")).timestamp() * 1000))
    assert core(request)["display"]["elapsedMs"] == measured.elapsed == 0
    completed = {**FIXTURE["timer"], "status": "completed", "elapsedAtAnchorMs": FIXTURE["timer"]["plannedDurationMs"]}
    request["source"]["value"]["base"]["canonicalTimer"] = completed
    assert core(request)["canonical"]["remainingMs"] == 0
    superseded = {**FIXTURE["timer"], "status": "superseded"}
    request["profile"] = "appleWorkspace"
    request["selectedPhase"] = "short_break"
    request["source"]["value"]["base"]["canonicalTimer"] = superseded
    display = core(request)["display"]
    assert (display["status"], display["phase"], display["elapsedMs"]) == ("idle", "short_break", 0)
    request["selectedPhase"] = "focus"
    request["source"]["value"]["base"]["canonicalTimer"] = FIXTURE["timer"]
    for row, measured in zip(FIXTURE["cadence"], pwa["counts"], strict=True):
        count = row["count"]
        history = [{"id": f"h{i}", "timerId": f"t{i}", "phase": "focus", "status": "completed",
                    "plannedDurationMs": 1500000, "completedAt": "2026-03-08T05:00:00Z"} for i in range(count)]
        request["source"]["value"]["base"]["history"] = history
        canonical = core(request)["cadence"]
        assert canonical["completedFocusToday"] == measured["count"] == count
        assert desktop["completed_focus_count_for_day"](history, request["observedAt"]) == count
        assert canonical["longBreakProgress"] == measured["progress"] == desktop["long_break_progress"](count)
        assert canonical["skipDestination"] == row["skip"]
    task_id = core_task_id()
    history = [{**item, "taskId": task_id if item["taskId"] == "current" else item["taskId"]}
               for item in FIXTURE["checkerHistory"]]
    now = datetime.fromisoformat(request["observedAt"].replace("Z", "+00:00"))
    desktop_task = desktop["task_summaries_today"]([{"id": task_id}], history, now)[task_id]
    request["source"]["value"]["base"]["tasks"] = [{"id": task_id, "title": "Study"}]
    request["source"]["value"]["base"]["history"] = history
    read = core(request)
    expected = FIXTURE["checkerDailyTotals"]
    assert desktop_task == {"finished": expected["currentTaskCount"],
        "timeMs": expected["currentTaskPlannedDurationMs"]}
    assert read["tasks"]["completedFocusTodayByTask"][task_id] == {"count": desktop_task["finished"],
        "plannedDurationMs": desktop_task["timeMs"]}
    assert pwa["tasks"]["current"] == {"count": desktop_task["finished"], "durationMs": desktop_task["timeMs"]}
    assert pwa["tasks"]["removed"] == {"count": 1, "durationMs": 300000}
    assert not pwa["tasks"]["unassigned"]
    assert read["cadence"]["completedFocusToday"] == expected["focusCount"]
    apple_count, apple_duration = apple_day_focus_totals(history, request["observedAt"])
    assert (apple_count, apple_duration) == (expected["focusCount"], expected["focusPlannedDurationMs"])
    assert (read["cadence"]["completedFocusToday"],
        read["cadence"]["completedFocusTodayPlannedDurationMs"]) == (apple_count, apple_duration)
    assert "removed" not in read["tasks"]["completedFocusTodayByTask"]
    request["source"]["value"]["base"]["history"] = []
    for row in pwa["readiness"]:
        timer = FIXTURE["timer"].copy()
        timer["status"] = row["status"]
        if row["status"] == "completed":
            timer["elapsedAtAnchorMs"] = timer["plannedDurationMs"]
        request["source"]["value"]["base"]["canonicalTimer"] = timer
        request["profile"] = "pwaStorage"
        available = core(request)["availableIntents"]
        assert row["finish"] == ("finish" in available and not row["blocked"])
        assert row["cancel"] == ("cancel" in available and not row["blocked"])
        assert row["clear"] == ("clear" in available and not row["blocked"])
        assert row["toggle"] == (not row["blocked"])
    rejected = 0
    for variant in ["projection", "timer", "history", "queue", "shortDay", "longDay"]:
        broken = copy.deepcopy(FIXTURE["request"])
        if variant in ("projection", "timer", "history", "queue"):
            broken["source"] = {"kind": "projectionResult", "value": {"projectionPending": {
                "commands": [{"garbage": True}] if variant == "queue" else []},
                "workspace": {"canonicalTimer": {"id": "forged"} if variant == "timer" else None,
                    "history": [{"id": "forged"}] if variant == "history" else []}}}
        else:
            broken["calendarIntervals"] = [{"start": "2026-03-08T07:30:00Z",
                "end": "2026-03-08T07:30:01Z" if variant == "shortDay" else "2026-03-10T07:30:00Z"}]
        assert core_rejects(broken), variant
        rejected += 1
    return 13 * 4 + 3 + len(pwa["readiness"]) * 4 + 6 + 4 + rejected + len(FIXTURE["desktopPrimary"]) * 2 + 2


def source_predicates():
    paths = {
        "apple": SUITE / "apple/Sources/AppModel.swift",
        "apple_tasks": SUITE / "apple/Sources/AppStatePublisher.swift",
        "watch": SUITE / "apple/Sources/WatchSyncPayload.swift",
        "dial": SUITE / "apple/Sources/Views/TimerDial.swift",
        "machine": SUITE / "apple/Sources/Views/TimerMachineCard.swift",
        "android": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/domain/TimerPresentation.kt",
        "hero": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/ui/TimerHero.kt",
        "android_tasks": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/ui/PatternHistoryViews.kt",
        "controls": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/ui/UiComponents.kt",
        "desktop_screen": SUITE / "desktop/src/pomodorough/timer_screen.py",
        "desktop_controller": SUITE / "desktop/src/pomodorough/timer_interaction_controller.py",
        "pwa_view": SUITE / "server/web/app-view.js",
    }
    predicates = {
        "apple": ["completedFocusCountToday % 4 == 3", "((completedFocusCountToday - 1) % 4) + 1", "selectedPhase.isBreak ? .focus", "timer.remaining(at: effectivePhysicalNow() ?? now())", "canonicalTimer?.status == .running || canonicalTimer?.status == .paused"],
        "apple_tasks": ["calendar.isDate(completedAt, inSameDayAs: date)", "current.timeMs + item.plannedDurationMs", "func dayFocusTotals(", "timeMs += item.plannedDurationMs"],
        "watch": ["if isRunning {", "anchored + max(0, date.timeIntervalSince(anchorAt))"],
        "dial": ["let elapsed = model.elapsedForDisplay(timer)", "let remaining = model.remainingForDisplay(timer)"],
        "machine": ["if let timer = model.activeTimer", "IdleTimerDial("],
        "android": ["it.completedAt ?: it.endedAt", "((completedFocusCount - 1) % 4) + 1", "timer.status == TimerStatus.Running"],
        "hero": ["timer.status == TimerStatus.Completed) 0", "enabled = state.ready", "status == TimerStatus.Running || status == TimerStatus.Paused"],
        "android_tasks": ["items.sumOf { it.plannedDurationMs }", "taskId == null -> stringResource(R.string.unassigned)"],
        "controls": ["timer?.status == TimerStatus.Completed", "settings.durationMsFor(phase)"],
        "desktop_screen": ['if status == "cancelled":', "elapsed = 0"],
        "desktop_controller": ['elif status == "idle":', 'elif status in TERMINAL_STATUSES:', 'context.store.queue_restart('],
        "pwa_view": ["the clear control stops the", "use.stopCompletionAlert();"],
    }
    for name, path in paths.items():
        text = path.read_text()
        for predicate in predicates[name]:
            assert predicate in text, (name, predicate)
        print(f"{path.relative_to(SUITE)} sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")
    return sum(map(len, predicates.values()))


if __name__ == "__main__":
    os.environ["TZ"] = "America/New_York"
    if hasattr(__import__("time"), "tzset"):
        __import__("time").tzset()
    print(f"runtime parity checks: {compare_runtime()}")
    print(f"production source predicates: {source_predicates()}")
