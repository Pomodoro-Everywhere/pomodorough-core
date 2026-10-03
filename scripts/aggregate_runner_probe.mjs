// Execute the real gate modules with native process and WASM replaced by transfer-only test doubles.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import * as nodeUrl from "node:url";
import { operations } from "../tests/aggregate_artifact/cases.mjs";
import { requiredHits, requiredRejections } from "../tests/aggregate_artifact/semantics.mjs";
import { fakeHost } from "./aggregate_artifact_fake_host.mjs";

const root = new URL("../tests/", import.meta.url);
const success = '{"ok":true,"value":{"marker":1,"preserved":"native"}}';
const failure = '{"error":"invalid shared-core input: rejected","ok":false}';

function probeCases() {
  const cases = Object.entries(requiredHits).flatMap(([hit, count]) => Array.from({ length: count }, (_, i) => ({
    operation: operations[i % operations.length], name: `${hit}-${i}`, input: JSON.stringify({ hit, i }),
    ok: true, hit, checks: { equals: { marker: 1 } },
  })));
  // Each operation still needs failed dispatch in the real envelope validator.
  cases.push(...operations.map((operation) => ({ operation, name: "negative", input: "{", ok: false })));
  cases.push(...Object.entries(requiredRejections).flatMap(([rejectionHit, count]) => Array.from({ length: count }, (_, i) => ({
    operation: "workspace.ownershipPlan.v1", name: `${rejectionHit}-${i}`, input: "{", ok: false, rejectionHit }))));
  return cases;
}

function probeState(argument, mismatch) {
  const all = probeCases();
  const groups = {
    generated: ["generated", "childPause", "childFinish", "barrierRebase", "barrier", "rejection"],
    deferred: ["desktopDeferred", "appleExplicit"], batch: ["cursorRotation", "batchDrain"],
    terminal: ["terminalBarrier", "terminalRejection", "terminalPromotion", "terminalComposition", "terminalNormalization"],
    display: ["pwaLifecycle", "pwaRebase", "pwaParity"], admission: ["pwaAdmission"],
    ownership: ["pwaLeaseBoundary", "pwaOwnerOrigin"],
  };
  const dynamic = new Set(Object.values(groups).flat());
  const staticCases = all.filter((item) => !dynamic.has(item.hit));
  const phases = Object.fromEntries(Object.entries(groups).map(([name, hits]) =>
    [name, all.filter((item) => hits.includes(item.hit))]));
  const cases = [...staticCases, ...Object.values(phases).flat()];
  const envelopes = cases.map((item) => item.ok ? success : failure);
  const returned = [...envelopes];
  if (mismatch) returned[0] = success.replace('"native"', '"changed"');
  const host = fakeHost(returned);
  const receipt = { native: 0, reads: 0, dispatches: 0, cases: cases.length,
    nativeCalls: 1 + cases.length - staticCases.length };
  const dispatch = host.exports.pomodorough_dispatch;
  host.exports.pomodorough_dispatch = (...args) => { receipt.dispatches += 1; return dispatch(...args); };
  const context = createContext({ TextEncoder, TextDecoder, URL, structuredClone,
    console: { log() {} }, process: { argv: ["node", "aggregate_wasm_parity.mjs", argument] },
    WebAssembly: { async instantiate() { return { instance: { exports: host.exports } }; } } });
  return { context, cases, staticCases, phases, envelopes, receipt };
}

function replacements(state) {
  const { cases, envelopes, receipt } = state;
  const responses = new Map(cases.map((item, i) => [JSON.stringify({ operation: item.operation, input: item.input }), envelopes[i]]));
  return {
    "node:assert/strict": { default: assert }, "node:url": nodeUrl,
    "node:fs/promises": { readFile: async (path) => { assert.equal(path, "official.wasm"); receipt.reads += 1; return new Uint8Array(); } },
    "node:child_process": { spawnSync: (command, args, options) => {
      assert.equal(command, "rustup");
      assert.deepEqual(Array.from(args), ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"]);
      receipt.native += 1;
      const output = options.input.trimEnd().split("\n").map((line) => {
        assert.ok(responses.has(line), "mock oracle received unknown request");
        return responses.get(line);
      });
      return { error: null, status: 0, stdout: output.join("\n"), stderr: "" };
    } },
    "aggregate_artifact/cases.mjs": { operations, vector: (operation, name, input, ok = true) =>
      ({ operation, name, input: JSON.stringify(input), ok }) },
    "aggregate_artifact/catalog.mjs": { aggregateCases: () => structuredClone(state.staticCases) },
    "aggregate_artifact/generated_scenarios.mjs": { generatedScenarios: (call) => structuredClone(state.phases.generated).forEach(call) },
    "aggregate_artifact/deferred_scenarios.mjs": { deferredScenarios: (call) => structuredClone(state.phases.deferred).forEach(call) },
    "aggregate_artifact/batch_scenarios.mjs": { batchScenarios: (call) => structuredClone(state.phases.batch).forEach(call) },
    "aggregate_artifact/terminal_scenarios.mjs": { terminalScenarios: (call) => structuredClone(state.phases.terminal).forEach(call) },
    "aggregate_artifact/pwa_display_scenarios.mjs": { displayScenarios: (call) => structuredClone(state.phases.display).forEach(call) },
    "aggregate_artifact/pwa_display_admission.mjs": { admissionScenarios: (call) => structuredClone(state.phases.admission).forEach(call) },
    "aggregate_artifact/pwa_ownership_cases.mjs": { ownershipScenarios: (call) => structuredClone(state.phases.ownership).forEach(call) },
  };
}

async function loadRunner(state, mutations) {
  const stubs = replacements(state);
  const cache = new Map();
  const load = (name) => {
    if (cache.has(name)) return cache.get(name);
    const pending = (async () => {
      const exports = stubs[name];
      const identifier = name.startsWith("node:") ? name : new URL(name, root).href;
      const module = exports ? new SyntheticModule(Object.keys(exports), function () {
        for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
      }, { context: state.context, identifier }) : new SourceTextModule(
        mutations[name]?.(readFileSync(new URL(name, root), "utf8")) ?? readFileSync(new URL(name, root), "utf8"),
        { context: state.context, identifier, initializeImportMeta: (meta) => { meta.url = identifier; } });
      await module.link((specifier, referring) => load(specifier.startsWith("node:") ? specifier
        : new URL(specifier, referring.identifier).href.slice(root.href.length)));
      return module;
    })();
    cache.set(name, pending);
    return pending;
  };
  return load("aggregate_wasm_parity.mjs");
}

export async function probeRunner(mutations = {}, { argument = "official.wasm", mismatch = false } = {}) {
  const state = probeState(argument, mismatch);
  const module = await loadRunner(state, mutations);
  await module.evaluate();
  assert.equal(state.receipt.native, state.receipt.nativeCalls, "actual runner skipped native oracle");
  assert.equal(state.receipt.dispatches, argument === "--native-only" ? 0 : state.receipt.cases * 5,
    "actual runner skipped required dispatches");
  assert.equal(state.receipt.reads, argument === "--native-only" ? 0 : 1, "actual runner skipped exact artifact");
  return state.receipt;
}

export async function verifyRunner(mutations = {}) {
  await probeRunner(mutations);
  await probeRunner(mutations, { argument: "--native-only" });
  await assert.rejects(() => probeRunner(mutations, { mismatch: true }), /preserved|raw envelope/,
    "actual runner accepted artifact/native mismatch");
}
