import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { invoke } from "./aggregate_artifact/abi_host.mjs";

const artifact = process.env.R49_C01_OFFICIAL_WASM;
assert.ok(artifact, "R49_C01_OFFICIAL_WASM must identify the official v0.48.0 artifact");
const bytes = readFileSync(artifact);
assert.equal(createHash("sha256").update(bytes).digest("hex"),
  "775d3d85cc4f6ced2243fd8b831e0057d6264e2202fdc2bf28d83b62e311e05a",
  "official v0.48.0 digest");
const { instance } = await WebAssembly.instantiate(bytes);
for await (const line of createInterface({ input: process.stdin })) {
  console.log(invoke(instance.exports, JSON.parse(line)));
}
