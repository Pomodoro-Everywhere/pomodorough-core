"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), Module = require("node:module");
const directory = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const filename = path.join(directory, "core-pwa10-independent-check.cjs");
const originalOutput = filename.replace(".cjs", ".json");
const moduleInstance = new Module(filename, module);
moduleInstance.filename = filename;
moduleInstance.paths = Module._nodeModulePaths(directory);
const originalRequire = moduleInstance.require.bind(moduleInstance);
moduleInstance.require = (name) => name === "node:fs" ? { ...fs, writeFileSync(destination, content, ...args) {
  assert.equal(destination, originalOutput, "checker may write only its receipt");
  return fs.writeFileSync(path.join(directory, "core-pwa10-independent-green.json"), content, ...args);
} } : originalRequire(name);
moduleInstance._compile(fs.readFileSync(filename, "utf8"), filename);
