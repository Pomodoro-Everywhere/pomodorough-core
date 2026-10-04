import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const file = new URL("../fixtures/pwa-completion-shapes-v1.json", import.meta.url);
assert.equal(process.argv[2], "--capture");
assert.deepEqual(JSON.parse(readFileSync(file, "utf8")), { fields: {} }, "shape fixture cannot be refreshed");
const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "test", "--locked", "--lib", "structural_fixture_guards_cover_every_shared_schema_path", "--", "--nocapture"], {
  cwd: new URL("../", import.meta.url), encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024,
  env: { ...process.env, PWA12_PRINT_SHAPES: "1", RUST_BACKTRACE: "0" } });
assert.equal(result.status, 101, result.stderr);
const line = result.stdout.split("\n").find((line) => line.startsWith("PWA12_SHAPES "));
assert.ok(line, result.stdout + result.stderr);
const manifest = JSON.parse(line.slice("PWA12_SHAPES ".length));
assert.ok(Object.keys(manifest.fields).length > 200);
writeFileSync(file, JSON.stringify(manifest, null, 2) + "\n");
console.log(`Captured ${Object.keys(manifest.fields).length} shared schema paths. Native metadata guard forbids uncovered field changes.`);
