import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { preservationMetadata } from "../tests/aggregate_artifact/pwa_selection_preservation.mjs";

const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const baseline = JSON.parse(readFileSync(`${directory}/core-pwa-selection-prior-2141.json`, "utf8"));
assert.equal(baseline.cases.length, 2141);
const destination = new URL("../fixtures/pwa-selection-preservation-v1.json", import.meta.url);
assert.equal(existsSync(destination), false, "The prior preservation digest cannot be replaced.");
writeFileSync(destination, JSON.stringify(preservationMetadata(baseline.cases, baseline.expected), null, 2) + "\n");
console.log("Captured immutable SHA256 checks for all 2141 pre-extension inputs and complete envelopes.");
