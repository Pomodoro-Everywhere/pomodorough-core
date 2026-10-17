import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { nativeResponses } from "./aggregate_artifact/native_oracle.mjs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";

const artifact = process.argv[2];
assert.equal(process.argv.length, 3, "usage: node tests/r49_c01_standalone_parity.mjs <official-v0.48.0.wasm>");
const bytes = readFileSync(artifact);
assert.equal(createHash("sha256").update(bytes).digest("hex"),
  "775d3d85cc4f6ced2243fd8b831e0057d6264e2202fdc2bf28d83b62e311e05a");
const { instance } = await WebAssembly.instantiate(bytes);
const timer = JSON.parse(readFileSync(new URL("../fixtures/workspace-intent-v1.json", import.meta.url))).timer;
const cases = [];
for (const status of ["running", "paused", "completed", "cancelled", "superseded"]) {
  for (const phase of ["focus", "short_break", "long_break"]) {
    for (const kind of ["start", "pause", "resume", "finish", "cancel", "clear", "unsupported", "retarget"]) {
      for (const target of ["existing-timer", "different-timer"]) {
        for (const now of ["2026-07-20T12:00:11Z", "2026-07-20T12:01:20Z", "2026-07-20T12:03:00Z"]) {
          const command = { id: "command-0001", deviceId: "device-local", deviceSequence: 2,
            timerId: target, taskId: "new-task", type: kind, phase, plannedDurationMs: 60000,
            occurredAt: "2026-07-20T12:01:20Z", hlcWallMs: 1784548880000, hlcCounter: 1,
            observedElapsedMs: 16000 };
          cases.push({ name: `${status}/${phase}/${kind}/${target}/${now}`, ok: true,
            operation: "timer.reduce.v1", input: JSON.stringify({
              canonicalTimer: { ...timer, status, phase, taskId: "old-task" }, commands: [command], now }) });
          const session = { timerId: timer.id, status, phase, taskId: "old-task",
            plannedDurationMs: 60000, elapsedAtAnchorMs: 5000, anchorAt: timer.anchorAt,
            startedAt: timer.anchorAt, startedByDeviceId: "device-local", lastCommandId: "" };
          if (["completed", "cancelled", "superseded"].includes(status)) session.endedAt = timer.anchorAt;
          cases.push({ name: `page/${status}/${phase}/${kind}/${target}/${now}`, ok: true,
            operation: "timer.replay.page.v1", input: JSON.stringify({ sessions: [session],
              currentTimerId: timer.id, after: null, commands: [command], now }) });
        }
      }
    }
  }
}
const native = nativeResponses(cases);
for (const [index, item] of cases.entries()) {
  assert.equal(native[index], invoke(instance.exports, item), `${item.name}: complete raw envelope`);
}
assert.equal(cases.length, 1440);
console.log("Standalone full/page contracts: 1440/1440 complete raw envelopes match official v0.48.0");

const binary = join(process.env.CARGO_TARGET_DIR ?? fileURLToPath(new URL("../target", import.meta.url)),
  "debug/examples/artifact_parity_oracle");
function parity(operation, input) {
  const item = { operation, input: JSON.stringify(input) };
  const run = spawnSync(binary, [], { input: JSON.stringify(item) + "\n", encoding: "utf8", timeout: 30000 });
  assert.equal(run.status, 0, run.stderr);
  const raw = run.stdout.trimEnd();
  assert.equal(raw, invoke(instance.exports, item), `${operation}: multipage raw envelope`);
  return JSON.parse(raw).value;
}
const commands = Array.from({ length: 257 }, (_, index) => ({ id: `command-${index.toString().padStart(4, "0")}`,
  deviceId: "device-local", deviceSequence: index + 1, timerId: "existing-timer",
  taskId: index === 0 ? "task-original" : `task-${index}`, type: index === 0 ? "start" : "retarget",
  phase: "focus", plannedDurationMs: 60000, observedElapsedMs: index === 0 ? 0 : 16000,
  occurredAt: index === 0 ? "2026-07-20T12:00:00Z" : "2026-07-20T12:01:20Z",
  hlcWallMs: index === 0 ? 1784548800000 : 1784548880000, hlcCounter: index }));
const now = "2026-07-20T12:00:11Z";
const full = parity("timer.reduce.v1", { commands, now });
for (const lengths of [[256, 1], [1, 256], [128, 128, 1], [...Array(36).fill(7), 5]]) {
  for (const clockOnly of [false, true]) {
    let state = { sessions: [], currentTimerId: null, after: null };
    let offset = 0;
    const outcomes = {};
    for (const [index, length] of lengths.entries()) {
      const input = { sessions: state.sessions, currentTimerId: state.currentTimerId, after: state.after,
        commands: commands.slice(offset, offset + length) };
      if (!clockOnly && index + 1 === lengths.length) input.now = now;
      state = parity("timer.replay.page.v1", input);
      Object.assign(outcomes, state.outcomes);
      offset += length;
    }
    if (clockOnly) state = parity("timer.replay.page.v1", { sessions: state.sessions,
      currentTimerId: state.currentTimerId, after: state.after, commands: [], now });
    delete state.currentTimerId;
    delete state.after;
    state.outcomes = outcomes;
    assert.deepEqual(state, full, `${lengths}/${clockOnly}: complete partition equality`);
  }
}
console.log("Official multipage parity: 257 commands, 4 partitions, final command/clock-only pages, exact raw returns");
