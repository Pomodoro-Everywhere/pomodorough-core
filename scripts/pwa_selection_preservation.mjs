import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { nativeResponses } from "../tests/aggregate_artifact/native_oracle.mjs";

const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const file = `${directory}/core-pwa-selection-prior-2141.json`;
if (process.argv.includes("--capture")) {
  assert.equal(existsSync(file), false, "The preservation baseline cannot be replaced.");
  const { cases, expected } = nativeCorpus();
  assert.equal(cases.length, 2141);
  writeFileSync(file, JSON.stringify({ cases, expected }, null, 2));
  console.log("Captured 2141 pre-selection envelopes.");
} else {
  const { cases, expected } = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(cases.length, 2141);
  assert.deepEqual(nativeResponses(cases), expected, "Existing operation envelope drift.");
  const current = nativeCorpus();
  const byName = new Map(current.cases.map((item, index) => [`${item.operation}/${item.name}`, index]));
  for (const [index, item] of cases.entries()) {
    const position = byName.get(`${item.operation}/${item.name}`);
    assert.notEqual(position, undefined, item.name);
    assert.equal(JSON.stringify(current.cases[position]), JSON.stringify(item));
    assert.equal(current.expected[position], expected[index], item.name);
  }
  console.log(`All 2141 old inputs and envelopes preserved; ${current.cases.length - 2141} new cases only.`);
}
