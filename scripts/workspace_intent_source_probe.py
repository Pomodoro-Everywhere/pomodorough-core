#!/usr/bin/env python3
"""Run production-source intent probes without importing client applications.

Extracted methods execute unchanged. Stubs provide storage, clocks, model
constants, and localization only. The native bridge runs the production Core
dispatcher. This is partial source parity, not a client integration test.
"""
import ast
import copy
from datetime import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile
import types

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT.parent
BRIDGE = ROOT / "target/debug/examples/completion_policy_probe"
FIXTURE = json.loads((ROOT / "fixtures/workspace-intent-v1.json").read_text())
SOURCES = {
    "apple": SUITE / "apple/Sources/TimerSessionController.swift",
    "skip": SUITE / "apple/Sources/AppModel.swift",
    "android": SUITE / "android/app/src/main/java/me/egigoka/pomodorough/data/TimerMutationCoordinator.kt",
    "desktop": SUITE / "desktop/src/pomodorough/terminal.py",
    "storage": SUITE / "desktop/src/pomodorough/storage.py",
    "pwa": SUITE / "server/web/app-storage.js",
    "pwa_clock": SUITE / "server/web/app-state.js",
    "pwa_view": SUITE / "server/web/app-view.js",
}


def request(profile, action, status="idle"):
    value = copy.deepcopy(FIXTURE["request"])
    value["compatibility"] = profile
    value["intent"] = {"kind": action}
    if status != "idle":
        timer = copy.deepcopy(FIXTURE["timer"])
        timer["status"] = status
        if status == "completed":
            timer["elapsedAtAnchorMs"] = timer["plannedDurationMs"]
        value["workspace"]["base"]["canonicalTimer"] = timer
        if (profile in ("desktopStorage", "desktopTerminal", "pwaStorage")
                and action in ("restart", "cancelAndClear")
                or profile == "desktopTerminal" and action == "cancel"):
            value["requestedTimer"] = copy.deepcopy(timer)
    return value


def core(value):
    payload = {"operation": "workspace.intent.v1", "input": value}
    result = subprocess.run([str(BRIDGE)], input=json.dumps(payload) + "\n",
                            text=True, capture_output=True, check=True)
    output = json.loads(result.stdout)
    assert "error" not in output, output
    return output


def between(path, start, end):
    text = path.read_text()
    begin = text.index(start)
    return text[begin:text.index(end, begin)]


def python_method(path, name):
    tree = ast.parse(path.read_text())
    method = next(node for node in ast.walk(tree)
                  if isinstance(node, ast.FunctionDef) and node.name == name)
    module = ast.Module(body=[method], type_ignores=[])
    namespace = {"Any": object, "ACTIVE_STATUSES": {"running", "paused"},
                 "TERMINAL_STATUSES": {"completed", "cancelled", "superseded"},
                 "InvalidAction": ValueError}
    exec(compile(module, str(path), "exec"), namespace)
    return namespace[name]


def checker_request(case):
    value = request(case["profile"], case["intent"], case["status"])
    value["replicationMode"] = case.get("replicationMode", "centralized")
    if case.get("retainedHistory"):
        value["workspace"]["base"]["canonicalTimer"] = None
        value["workspace"]["base"]["history"] = [copy.deepcopy(FIXTURE["retainedCompletion"])]
    if case.get("presentedAnchorAt"):
        value["requestedTimer"]["anchorAt"] = case["presentedAnchorAt"]
    if case.get("presentedTimerId"):
        value["requestedTimer"]["id"] = case["presentedTimerId"]
    return value


def desktop_checker_cases():
    context = python_method(SOURCES["storage"], "_terminal_action_context")
    retained = python_method(SOURCES["storage"], "_retained_terminal_context")
    fingerprint = python_method(SOURCES["storage"], "_timer_fingerprint")
    ownership = python_method(SOURCES["storage"], "_apply_timer_command_side_effects")
    count = 0
    for case in FIXTURE["checkerCases"]:
        value = checker_request(case)
        output = core(value)
        actual = [command["type"] for command in output["commands"]]
        assert actual == case["commands"], (case["name"], actual)
        if case["intent"] == "start":
            writes = []
            owner = types.SimpleNamespace(replication_mode=case["replicationMode"], device_id="device-local",
                _set_meta=lambda key, data: writes.append((key, data)), _apply_finish_command=lambda *_: None)
            ownership(owner, output["commands"][0], None)
            source = any(key == "centralizedTimerOwnership" for key, _ in writes)
            planned = any(write["kind"] == "recordStart" for write in output["ownershipWrites"])
            assert source == planned == case["centralOwnership"], case["name"]
        else:
            history = value["workspace"]["base"]["history"]
            state = {"snapshot": {"history": history}, "pending": []}
            projected = types.SimpleNamespace(canonical_timer=value["workspace"]["base"]["canonicalTimer"], tasks=[])
            owner = types.SimpleNamespace(load=lambda **_: state, projected_state=lambda **_: projected,
                projected_settings=lambda *_: {"selectedPhase": "focus", "durationsMs": value["workspace"]["base"]["durationsMs"], "selectedTaskId": None},
                _timer_fingerprint=fingerprint, _retained_terminal_context=retained)
            allowed = {"running", "paused"} if case["intent"] == "cancel" else {"completed", "cancelled", "superseded"}
            try:
                context(owner, value["requestedTimer"], 1784548810000, allowed, "stale")
                accepted = True
            except ValueError:
                accepted = False
            assert accepted == bool(actual), (case["name"], accepted, actual)
        count += 1
    return count


def desktop():
    validate = python_method(SOURCES["desktop"], "_validated_command")
    owner = types.SimpleNamespace(strings=types.SimpleNamespace(text=lambda *args, **kw: "invalid"))
    count = 0
    for action in ("start", "pause", "resume", "cancel", "clear"):
        for status in FIXTURE["statuses"]:
            try:
                validate(owner, action, {"status": status})
                accepted = True
            except ValueError:
                accepted = False
            output = core(request("desktopTerminal", action, status))
            assert accepted == bool(output["commands"]), (action, status)
            count += 1
    select = python_method(SOURCES["storage"], "set_selected_phase")
    select.__globals__["PHASES"] = ("focus", "short_break", "long_break")
    from contextlib import nullcontext
    for before in (5, 2**63 - 1, 10**25 - 1):
        saved = {"settings": {"selectedPhase": "focus"}, "selectedPhaseVersion": before}
        owner = types.SimpleNamespace(_immediate_transaction=nullcontext,
            _ensure_no_pending_resolution=lambda: None, _normalize_settings=lambda value: value,
            get_meta=lambda key, default: saved.get(key, default),
            _set_meta=lambda key, value: saved.__setitem__(key, value),
            _capture_iroh_after_mutation_locked=lambda: None)
        select(owner, "short_break")
        value = request("desktopStorage", "selectPhase")
        value["intent"]["phase"] = "short_break"
        value["selection"]["generation"] = str(before)
        assert core(value)["selection"]["generation"] == str(saved["selectedPhaseVersion"])
        count += 1
    return count


def apple(temporary):
    generation = between(SOURCES["apple"], "    static func nextPhaseGeneration(", "    static func derivedNextPhase(")
    skip = between(SOURCES["skip"], "    func skipDestinationFromFocus()", "    private func enqueue(")
    program = "enum TimerPhase: String { case shortBreak = \"short_break\", longBreak = \"long_break\" }\n"
    program += "struct Probe { var completedFocusCountToday: Int\n" + generation + skip + "}\n"
    program += 'for i in 0...12 { print(Probe(completedFocusCountToday: i).skipDestinationFromFocus().rawValue) }\n'
    program += 'print(Probe.nextPhaseGeneration(after: Int64.max))\n'
    path = temporary / "probe.swift"
    path.write_text(program)
    results = subprocess.run(["swift", str(path)], text=True, capture_output=True, check=True).stdout.splitlines()
    for count, phase in enumerate(results[:13]):
        value = request("appleWorkspace", "skip")
        value["workspace"]["base"]["history"] = [dict(id=f"h-{i}", timerId=f"t-{i}",
            phase="focus", status="completed", plannedDurationMs=60000,
            completedAt="2026-07-20T10:00:00Z") for i in range(count)]
        assert core(value)["selection"]["phase"] == phase
    value = request("appleWorkspace", "selectPhase")
    value["intent"]["phase"] = "focus"
    value["selection"]["generation"] = str(2**63 - 1)
    assert core(value)["selection"]["generation"] == results[-1]
    return 14


def android(temporary):
    valid = between(SOURCES["android"], "    private fun validTransition(", "    private fun project(")
    valid = valid.replace("private fun validTransition", "fun validTransition", 1)
    cancel = between(SOURCES["android"], "    fun cancelAndClearTypes(", "    fun acceptsDuration(")
    constants = 'object CommandType { ' + '; '.join(f'const val {s.title()} = "{s}"' for s in
        ["start", "pause", "resume", "finish", "cancel", "clear"]) + ' }\n'
    constants += 'object TimerStatus { ' + '; '.join(f'const val {s.title()} = "{s}"' for s in
        FIXTURE["statuses"]) + ' }\n'
    program = constants + 'data class CanonicalTimer(val status: String)\nclass Probe {\n'
    program += 'val ActiveStatuses = setOf("running", "paused")\n' + valid + cancel + '}\nfun main() { val p = Probe()\n'
    cases = [(a, s) for a in ("start", "pause", "resume", "cancel", "clear", "cancelAndClear")
             for s in FIXTURE["statuses"]]
    for action, status in cases:
        timer = "null" if status == "idle" else f'CanonicalTimer("{status}")'
        call = (f'p.cancelAndClearTypes({timer}).joinToString(",")' if status != "idle" else '""') if action == "cancelAndClear" else f'p.validTransition("{action}", {timer}).toString()'
        program += f'println({call})\n'
    path = temporary / "probe.kt"
    path.write_text(program + '}\n')
    compiler = os.environ.get("KOTLINC", "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc")
    java = os.environ.get("JAVA_HOME", "/Applications/Android Studio.app/Contents/jbr/Contents/Home")
    jar = temporary / "probe.jar"
    subprocess.run([compiler, str(path), "-include-runtime", "-d", str(jar)], env=dict(os.environ, JAVA_HOME=java), check=True, capture_output=True)
    results = subprocess.run([str(Path(java) / "bin/java"), "-jar", str(jar)], text=True, capture_output=True, check=True).stdout.splitlines()
    for (action, status), expected in zip(cases, results, strict=True):
        commands = core(request("androidCoordinator", action, status))["commands"]
        actual = ",".join(c["type"] for c in commands) if action == "cancelAndClear" else str(bool(commands)).lower()
        assert actual == expected, (action, status, actual, expected)
    return len(cases)


def pwa(temporary):
    methods = between(SOURCES["pwa"], "    timerCommandContext(", "    async persistCommand(")
    timing = between(SOURCES["pwa_clock"], "  function positiveNumber(", "  function bindActions(")
    elapsed = between(SOURCES["pwa_clock"], "    elapsedFor(timer,", "\n  }\n\n  class SharedTaskCore")
    display = between(SOURCES["pwa_view"], "    timerDisplayView(timer,", "    renderTimerClock(")
    program = timing + 'class ClockProbe {\n' + elapsed + '\n}\n'
    program += 'class ViewProbe {\n' + display + '}\n'
    program += 'class Probe {\n' + methods + '}\n'
    program += (ROOT / "scripts/workspace_intent_probe_pwa.cjs").read_text()
    path = temporary / "probe.cjs"
    path.write_text(program)
    cases = []
    for action, status in [("start", "idle"), ("pause", "running"), ("resume", "paused"), ("cancel", "running"), ("clear", "completed")]:
        for phase in ("focus", "short_break", "long_break"):
            value = request("pwaStorage", action, status)
            value["selection"]["phase"] = phase
            output = core(value)
            cases.append({"input": value, "command": output["commands"][0]})
    sequences = pwa_active_sequences()
    result = subprocess.run(["node", str(path)], input=json.dumps({"cases": cases, "sequences": sequences}), text=True,
                             capture_output=True, check=True)
    observed = json.loads(result.stdout)
    assert observed["ordinary"] == [case["command"] for case in cases]
    for sequence, actual in zip(sequences, observed["active"], strict=True):
        for step, source in zip(sequence, actual, strict=True):
            expected = step["command"] or pwa_display(step["reading"])
            assert source == expected, (step["input"]["intent"], source, expected)
    return len(cases) + sum(len(sequence) for sequence in sequences)


def pwa_display(reading):
    remaining = reading["remainingMs"]
    seconds = math.ceil(remaining / 1000)
    return {"elapsedMs": reading["elapsedMs"], "remainingMs": remaining,
            "totalSeconds": seconds, "timeText": f"{seconds // 60:02d}:{seconds % 60:02d}"}


def pwa_step(value, timer, command=None, reading=None, reset=False):
    return {"input": value, "timer": timer, "command": command,
            "reading": reading, "resetClock": reset}


def pwa_at(value, at, monotonic, identity="browser-session-1"):
    value = copy.deepcopy(value)
    value["clock"].update(physicalNow=at, observedAt=at, occurredAt=at,
                          monotonicNowMs=monotonic, continuityId=identity)
    wall = int(datetime.fromisoformat(at.replace("Z", "+00:00")).timestamp() * 1000)
    wall = max(wall, value["allocation"]["hlc"]["wallMs"])
    stamp = f"{wall:012x}"
    value["identities"]["commandUuids"] = [f"{stamp[:8]}-{stamp[8:]}-7000-8000-000000000001"]
    return value


def pwa_active_sequences():
    seed = request("pwaStorage", "selectPhase", "running")
    seed["intent"]["phase"] = "focus"
    seed = pwa_at(seed, "2026-07-20T12:00:10Z", 100)
    sampled = core(seed)
    timer = seed["workspace"]["base"]["canonicalTimer"]
    assert sampled["observation"]["monotonicAnchor"]["sampledTrustedNowMs"] == 1784548810000
    sequence = [pwa_step(seed, timer, reading=sampled["timerObservation"])]
    value = copy.deepcopy(seed)
    value["observation"] = sampled["observation"]
    value["intent"] = {"kind": "pause"}
    pause = pwa_at(value, "2026-07-20T12:01:20Z", 1100)
    paused = core(pause)
    assert paused["commands"][0]["observedElapsedMs"] == 16000
    sequence.append(pwa_step(pause, timer, paused["commands"][0]))
    paused_timer = paused["projection"]["canonicalTimer"]
    sequence.append(pwa_step(pause, paused_timer, reading=paused["timerObservation"]))
    value["workspace"] = paused["workspace"]
    value["allocation"] = paused["allocation"]
    value["observation"] = paused["observation"]
    value["intent"] = {"kind": "resume"}
    resume = pwa_at(value, "2026-07-20T12:01:21Z", 2100)
    resumed = core(resume)
    sequence.append(pwa_step(resume, paused_timer, resumed["commands"][0]))
    running = resumed["projection"]["canonicalTimer"]
    sequence.append(pwa_step(resume, running, reading=resumed["timerObservation"]))
    value["workspace"] = resumed["workspace"]
    value["allocation"] = resumed["allocation"]
    value["observation"] = resumed["observation"]
    value["intent"] = {"kind": "pause"}
    backwards = pwa_at(value, "2026-07-20T11:59:20Z", 3100)
    backwards["identities"]["commandUuids"] = ["019f7f66-f268-7000-8000-000000000002"]
    reverse = core(backwards)
    assert reverse["commands"][0]["observedElapsedMs"] == 17000
    sequence.append(pwa_step(backwards, running, reverse["commands"][0]))
    return [sequence, *pwa_discontinuous_sequences(seed, sampled, timer),
            *pwa_absent_sequences(seed, sampled, timer),
            pwa_fractional_sequence(), pwa_deadline_sequence()]


def pwa_fractional_sequence():
    seed = request("pwaStorage", "selectPhase", "running")
    seed["intent"]["phase"] = "focus"
    seed = pwa_at(seed, "2026-07-20T12:00:10Z", 100.25)
    sampled = core(seed)
    timer = seed["workspace"]["base"]["canonicalTimer"]
    value = copy.deepcopy(seed)
    value["intent"] = {"kind": "pause"}
    value["observation"] = sampled["observation"]
    value["clock"]["monotonicNowMs"] = 1099.75
    reading = copy.deepcopy(value)
    reading["intent"] = {"kind": "selectPhase", "phase": "focus"}
    live = core(reading)
    output = core(value)
    assert output["commands"][0]["observedElapsedMs"] == 16000
    return [pwa_step(seed, timer, reading=sampled["timerObservation"]),
            pwa_step(reading, timer, reading=live["timerObservation"]),
            pwa_step(value, timer, output["commands"][0])]


def pwa_deadline_sequence():
    seed = request("pwaStorage", "selectPhase", "running")
    seed["intent"]["phase"] = "focus"
    seed = pwa_at(seed, "2026-07-20T12:00:10Z", 100.25)
    sampled = core(seed)
    near = copy.deepcopy(seed)
    near["observation"] = sampled["observation"]
    near["clock"]["monotonicNowMs"] = 45099.75
    live = core(near)
    timer = seed["workspace"]["base"]["canonicalTimer"]
    return [pwa_step(seed, timer, reading=sampled["timerObservation"]),
            pwa_step(near, timer, reading=live["timerObservation"])]


def pwa_absent_sequences(seed, sampled, timer):
    missing = copy.deepcopy(seed)
    missing["observation"] = sampled["observation"]
    missing["clock"].pop("monotonicNowMs")
    missing["clock"].pop("continuityId")
    missing["clock"].update(physicalNow="2026-07-20T12:00:20Z",
                            observedAt="2026-07-20T12:00:20Z", occurredAt="2026-07-20T12:00:20Z")
    absent = core(missing)
    resumed = pwa_at(missing, "2026-07-20T12:00:40Z", 2100)
    resumed["observation"] = absent["observation"]
    live = core(resumed)
    backward = pwa_at(missing, "2026-07-20T12:00:40Z", 50)
    backward["observation"] = absent["observation"]
    reset = core(backward)
    prefix = [pwa_step(seed, timer, reading=sampled["timerObservation"]),
              pwa_step(missing, timer, reading=absent["timerObservation"])]
    reboot = copy.deepcopy(missing)
    reboot["clock"]["continuityId"] = "browser-session-2"
    restarted = core(reboot)
    next_boot = pwa_at(reboot, "2026-07-20T12:00:40Z", 2100, "browser-session-2")
    next_boot["observation"] = restarted["observation"]
    next_reading = core(next_boot)
    return [
        [*prefix, pwa_step(resumed, timer, reading=live["timerObservation"])],
        [*prefix, pwa_step(backward, timer, reading=reset["timerObservation"])],
        [prefix[0], pwa_step(reboot, timer, reading=restarted["timerObservation"], reset=True),
         pwa_step(next_boot, timer, reading=next_reading["timerObservation"])],
    ]


def pwa_discontinuous_sequences(seed, sampled, timer):
    sequences = []
    for name, monotonic, identity, anchor in [
        ("null", 1100, "browser-session-1", None),
        ("backwards", 50, "browser-session-1", sampled["observation"]["monotonicAnchor"]),
        ("restart", 1100, "browser-session-2", sampled["observation"]["monotonicAnchor"]),
    ]:
        value = copy.deepcopy(seed)
        value["intent"] = {"kind": "pause"}
        value["observation"]["monotonicAnchor"] = anchor
        value = pwa_at(value, "2026-07-20T12:00:12Z", monotonic, identity)
        output = core(value)
        assert output["commands"][0]["observedElapsedMs"] == 17000, name
        if name == "backwards":
            sequences.append([pwa_step(seed, timer, reading=sampled["timerObservation"]),
                              pwa_step(value, timer, output["commands"][0])])
        else:
            sequences.append([pwa_step(value, timer, output["commands"][0], name == "restart")])
    replacement = copy.deepcopy(seed)
    replacement["intent"] = {"kind": "pause"}
    replacement["observation"] = sampled["observation"]
    replacement["workspace"]["base"]["canonicalTimer"]["id"] = "replacement-timer"
    replacement = pwa_at(replacement, "2026-07-20T12:00:12Z", 1100)
    output = core(replacement)
    assert output["commands"][0]["observedElapsedMs"] == 17000
    sequences.append([pwa_step(seed, timer, reading=sampled["timerObservation"]), pwa_step(replacement,
        replacement["workspace"]["base"]["canonicalTimer"], output["commands"][0])])
    return sequences


def main():
    with tempfile.TemporaryDirectory(prefix="workspace-intent-probe-") as directory:
        temporary = Path(directory)
        for name, count in [("desktop", desktop()), ("desktop checker", desktop_checker_cases()), ("apple", apple(temporary)),
                            ("android", android(temporary)), ("pwa", pwa(temporary))]:
            print(f"{name}: {count} production-source parity cases passed")
    for path in SOURCES.values():
        print(f"{path.relative_to(SUITE)} sha256={hashlib.sha256(path.read_bytes()).hexdigest()}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or "")
        print(error.stderr or "")
        raise
