import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";

const directory = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const file = process.env.PWA10_PARITY_FILE || `${directory}/core-pwa10-preserved-envelopes.json`;
if (process.argv.includes("--capture")) {
  assert.equal(existsSync(file), false, "preservation baseline cannot be replaced");
  const { cases, expected } = nativeCorpus();
  writeFileSync(file, JSON.stringify({ cases, expected }, null, 2));
  console.log(`Captured ${cases.length} pre-PWA10 envelopes.`);
} else {
  const { cases, expected } = JSON.parse(readFileSync(file, "utf8"));
  const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
    cwd: new URL("../", import.meta.url), input: cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n",
    encoding: "utf8", timeout: 120000, maxBuffer: 64 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(result.stdout.trimEnd().split("\n"), expected, "existing operation envelope drift");
  console.log(`${cases.length} pre-PWA10 envelopes byte-identical.`);
}
