import { readFile } from "node:fs/promises";
import { nativeCorpus } from "./aggregate_artifact/corpus.mjs";
import { exerciseArtifact } from "./aggregate_artifact/abi_host.mjs";

const argument = process.argv[2];
if (process.argv.length !== 3 || !argument) {
  throw new Error("usage: node tests/aggregate_wasm_parity.mjs <exact-release.wasm|--native-only>");
}
const corpus = nativeCorpus();
const { cases, expected, hits } = corpus;
console.log(`Semantic branch hits: ${JSON.stringify(hits)}`);
if (argument === "--native-only") {
  console.log(`Aggregate native oracle: ${cases.length} cases passed; WASM not exercised`);
} else {
  const { instance } = await WebAssembly.instantiate(await readFile(argument));
  exerciseArtifact(instance.exports, cases, expected, corpus);
  console.log(`Aggregate official artifact parity: ${cases.length} cases passed, 5 passes, memory stable`);
}
