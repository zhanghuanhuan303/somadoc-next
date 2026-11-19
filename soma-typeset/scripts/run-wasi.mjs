// Minimal WASI runner for the soma-wasm fixture binary.
// Usage: node scripts/run-wasi.mjs target/wasm32-wasip1/release/soma-wasm.wasm
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";

const path = process.argv[2];
if (!path) {
  console.error("usage: node scripts/run-wasi.mjs <file.wasm>");
  process.exit(2);
}

const wasi = new WASI({
  version: "preview1",
  args: [],
  env: {},
});

const bytes = await readFile(path);
const module = await WebAssembly.compile(bytes);
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
wasi.start(instance);
