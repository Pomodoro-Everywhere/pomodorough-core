import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const directory = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const fixturePath = new URL("../fixtures/legacy-dependency-residual-v1.json", import.meta.url);
const operation = "workspace.legacyDependencyPlan.v1";
const oracle = process.env.PWA10_NATIVE_ORACLE || new URL("../target/debug/examples/artifact_parity_oracle", import.meta.url).pathname;
const capture = process.argv.includes("--capture");
const captureComposition = process.argv.includes("--capture-composition");
if (capture) {
  assert.equal(existsSync(fixturePath), false, "residual baseline cannot be replaced");
  const original = JSON.parse(readFileSync(`${directory}/core-pwa10-recheck-adversarial.json`, "utf8"));
  assert.equal(original.summary.failures.length, 6);
  const rebases = JSON.parse(readFileSync(`${directory}/core-pwa10-recheck-rebase.json`, "utf8"));
  const cases = original.evidence.filter((row) => original.summary.failures.includes(row.name)).map((row) => {
    const rebase = rebases.find((candidate) => candidate.name === row.name);
    return { name: row.name, operation, inputRaw: row.inputRaw, originalEnvelope: row.actualEnvelope,
      rebaseInput: rebase?.rebaseInput ?? null, originalRebaseEnvelope: rebase?.rebased ?? null };
  });
  assert.equal(cases.length, 6);
  writeFileSync(fixturePath, JSON.stringify({ cases }, null, 2) + "\n");
}

function native(operation, input) {
  const result = spawnSync(oracle, [], { input: JSON.stringify({ operation, input: typeof input === "string" ? input : JSON.stringify(input) }) + "\n",
    encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  return { raw: result.stdout.trimEnd(), envelope: JSON.parse(result.stdout) };
}

const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
const template = JSON.parse(JSON.parse(readFileSync(new URL("../fixtures/reconciliation-terminal-v3.json", import.meta.url), "utf8")).http.responseRaw);
function bodyRebase(input, dependencies) {
  const response = { ...template, ...input.workspace.base, serverTime: input.workspace.now };
  Object.assign(response, { serverHlcWallMs: input.outgoing.sent.commands[0].hlcWallMs,
    serverHlcCounter: input.outgoing.sent.commands[0].hlcCounter });
  for (const field of ["acknowledgements", "taskAcknowledgements", "durationAcknowledgements", "autoStartAcknowledgements", "selectedTaskAcknowledgements"]) response[field] = [];
  if (input.workspace.canonicalHead) Object.assign(response, { serverHlcWallMs: input.workspace.canonicalHead.wallMs, serverHlcCounter: input.workspace.canonicalHead.counter });
  return { local: input.workspace.local, sent: Object.fromEntries(Object.keys(input.workspace.local).map((name) => [name, []])),
    response, timerDependencies: dependencies, neverSent: input.workspace.neverSent };
}
const receipts = [];
for (const item of fixture.cases) {
  const input = JSON.parse(item.inputRaw), result = native(operation, item.inputRaw);
  if (capture || captureComposition) assert.equal(result.raw, item.originalEnvelope);
  else {
    assert.equal(result.envelope.ok, true, result.envelope.error);
    const plan = result.envelope.value;
    assert.equal(plan.outcome, "blocked");
    assert.deepEqual(plan.workspace, input.workspace); assert.deepEqual(plan.outgoing, input.outgoing);
    assert.deepEqual(plan.metadataWrites, []); assert.equal(plan.timerDependencies, null);
    assert.equal(plan.recovery.blocksSync, true); assert.equal(plan.recovery.blocksMutations, true);
  }
  const reopened = native(operation, { ...input, workspace: result.envelope.value.workspace });
  if (!capture && !captureComposition) assert.equal(reopened.raw, result.raw, `${item.name}: blocked reopen drift`);
  const rebaseInput = item.rebaseInput || bodyRebase(input, result.envelope.value.timerDependencies);
  const composition = native("reconcile.rebase.v3", rebaseInput);
  if (item.rebaseInput) {
    assert.deepEqual(composition.envelope, item.originalRebaseEnvelope);
    assert.equal(composition.envelope.ok, false);
    assert.match(composition.envelope.error, /rewrite a possibly delivered operation/);
  } else assert.equal(composition.envelope.ok, capture || captureComposition, `${item.name}: ${composition.envelope.error}`);
  receipts.push({ ...item, actualEnvelope: result.raw, reopenedEnvelope: reopened.raw, compositionInput: rebaseInput, composition });
}
const mode = captureComposition ? "red-composition" : capture ? "red" : "green";
writeFileSync(`${directory}/core-pwa10-residual-${mode}-packet.json`, JSON.stringify({ mode, cases: receipts.length, receipts }, null, 2));
console.log(`${receipts.length} original residual inputs verified in ${mode} mode.`);
