"""Run Desktop's persisted-state projection and bootstrap method bodies."""
import ast
import contextlib
import copy
import dataclasses
import json
from pathlib import Path
import sys
import types

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT.parent / "desktop/src/pomodorough"


def extract_class(path, name, methods, namespace):
    tree = ast.parse(path.read_text(), filename=str(path))
    definition = next(node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == name)
    definition.body = [node for node in definition.body if isinstance(node, ast.FunctionDef) and node.name in methods]
    assert {node.name for node in definition.body} == methods
    module = ast.Module(body=[ast.ImportFrom(module="__future__", names=[ast.alias(name="annotations")], level=0), definition], type_ignores=[])
    exec(compile(ast.fix_missing_locations(module), str(path), "exec"), namespace)
    return namespace[name]


def adapter():
    path = DESKTOP / "shared_core.py"
    tree = ast.parse(path.read_text())
    tree.body = [node for node in tree.body if not (isinstance(node, ast.ImportFrom) and node.module == "wasmtime")]
    module = types.ModuleType("bootstrap_desktop_native_adapter")
    sys.modules[module.__name__] = module
    exec(compile(tree, str(path), "exec"), module.__dict__)
    return module


def desktop(rows, temporary, dispatch):
    actual_adapter = adapter()
    constants = ast.parse((DESKTOP / "core.py").read_text())
    phases = next(node.value for node in constants.body if isinstance(node, ast.Assign)
                  and any(isinstance(target, ast.Name) and target.id == "PHASES" for target in node.targets))
    namespace = dict(PHASES=ast.literal_eval(phases), apply_projection_v2=actual_adapter.apply_projection_v2,
                     SharedCoreOperationError=actual_adapter.SharedCoreOperationError)
    sync = extract_class(DESKTOP / "storage_sync.py", "SyncStorage", {
        "bootstrap_resolution_plan", "_empty_sync_request", "_has_bootstrap_state",
        "_completed_history_count", "_validated_bootstrap_plan", "_bootstrap_strategy"}, namespace)
    store = extract_class(DESKTOP / "storage.py", "Store", {
        "_project_operation", "_projection_pending", "_projection_input", "_with_device_id"}, namespace)
    return [run(value, sync, store, dispatch) for _, value in rows]


def run(value, sync, store, dispatch):
    workspace = value["local"]["workspace"]
    state = dict(snapshot=copy.deepcopy(workspace["base"]), settings=copy.deepcopy(value["local"]["preferences"]))
    for old, queue in zip(("pending", "pendingTasks", "pendingDurations", "pendingAutoStarts", "pendingSelectedTasks"),
        ("commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations")):
        state[old] = copy.deepcopy(workspace["local"][queue])
    observation_raw = json.dumps(state)
    calls = []
    core = types.SimpleNamespace(dispatch=lambda operation, request: dispatch(operation, request, calls))
    storage = object.__new__(store)
    storage.device_id = "device-a"
    storage._shared_core = core
    storage.load = lambda: copy.deepcopy(state)
    instance = object.__new__(sync)
    projections = []
    def project(*args, **kwargs):
        result = storage._project_operation(*args, **kwargs)
        projections.append(result)
        return result
    instance._dependencies = types.SimpleNamespace(shared_core=lambda: core,
        validate_sync_response=lambda response, request: response, transaction=contextlib.nullcontext,
        preflight_pending_queues=lambda: None, response_clock_sample=lambda *args: (None, None),
        load_state=lambda: copy.deepcopy(state), project_operation=project)
    remote = dict(value["remote"], revision=0, serverTime=workspace["now"], serverTimeMs=1)
    try:
        complete = instance.bootstrap_resolution_plan(remote)
    except (ValueError, RuntimeError) as error:
        return dict(productionError=str(error))
    projection = projections[0]
    projection_call, plan_call = calls
    plan = plan_call["decoded"]
    projected_raw = json.dumps(dataclasses.asdict(projection))
    return dict(hasLocalState=sync._has_bootstrap_state(state, projection.canonical_timer),
        hasRemoteState=sync._has_bootstrap_state(remote),
        localHistory=projection.history, localDisplayHistoryCount=sync._completed_history_count(projection.history),
        remoteDisplayHistoryCount=sync._completed_history_count(remote["history"]), plan=plan,
        strategy=complete["strategy"], completeReturn=complete, emittedInput=plan_call["inputDecoded"],
        projectionCall=projection_call, projectionRaw=projected_raw, projectionDecoded=json.loads(projected_raw),
        observationRaw=observation_raw, observationDecoded=json.loads(observation_raw))
