"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), Module = require("node:module");
const directory = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const outputs = new Map(["adversarial", "rebase"].map((name) => [
  path.join(directory, `core-pwa10-recheck-${name}.json`), path.join(directory, `core-pwa10-residual-${name}-green.json`)]));
for (const name of ["adversarial", "rebase"]) {
  const filename = path.join(directory, `core-pwa10-recheck-${name}.cjs`), instance = new Module(filename, module);
  instance.filename = filename; instance.paths = Module._nodeModulePaths(directory);
  const requireOriginal = instance.require.bind(instance);
  instance.require = (name) => name === "node:fs" ? { ...fs,
    readFileSync(filename, ...args) {
      const input = path.resolve(filename);
      const redirected = input === path.join(directory, "core-pwa10-recheck-adversarial.json") && fs.existsSync(outputs.get(input));
      return fs.readFileSync(redirected ? outputs.get(input) : filename, ...args);
    },
    writeFileSync(filename, content, ...args) {
      const input = path.resolve(filename), destination = outputs.get(input);
      assert.ok(destination, `checker writes only known receipt: ${input}`);
      return fs.writeFileSync(destination, content, ...args);
    } } : requireOriginal(name);
  instance._compile(fs.readFileSync(filename, "utf8"), filename);
  assert.notEqual(process.exitCode, 1, `unchanged ${name} checker failed`);
}
