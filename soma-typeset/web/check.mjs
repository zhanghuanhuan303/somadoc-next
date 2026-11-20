// Headless checks for the browser slice: node has no canvas, so the width table
// is generated here instead of measured. That is enough for W1/W2/W4/W5/W6;
// W3 (parity with the native fixture) runs in Chromium via `harness.html`.
//
//   node web/check.mjs web/soma-kp.wasm <gzip-bytes> <budget-bytes>

import { readFile } from 'node:fs/promises';
import { Kp, UNITS_PER_EM } from './soma-kp.js';

const [wasmPath, gzText, budgetText] = process.argv.slice(2);
if (!wasmPath) {
  console.error('usage: node web/check.mjs <wasm> [gzip-bytes] [budget-bytes]');
  process.exit(2);
}

const rows = [];
function report(id, ok, detail) {
  rows.push([id, ok ? 'ok' : 'FAIL', detail]);
  console.log(`${id}|${ok ? 'ok' : 'FAIL'}|${detail}`);
}

const bytes = await readFile(wasmPath);
const module = new WebAssembly.Module(bytes);
const imports = WebAssembly.Module.imports(module);
report('W1-imports', imports.length === 0, `${imports.length} imports, ${bytes.byteLength} bytes raw`);

if (gzText) {
  const gz = Number(gzText);
  const budget = Number(budgetText || 40000);
  report('W2-size', gz <= budget, `${gz} bytes gzip, budget ${budget}`);
}

const { instance } = await WebAssembly.instantiate(bytes, {});
const kp = Kp.fromInstance(instance);

// Deterministic widths: 1 em per CJK character, half an em per ASCII byte.
function widthOf(text) {
  let units = 0;
  for (const character of text) {
    units += /[ -~]/.test(character) ? 512 : 4096;
  }
  return units;
}

const paragraph = '中文English混排，数字 123，排版Engine v2。'.repeat(120);
const compiled = kp.compile(paragraph, {
  width: 40 * UNITS_PER_EM,
  lineHeight: Math.round(1.5 * UNITS_PER_EM),
});
const widths = Int32Array.from(compiled.atoms.map((atom) => widthOf(atom.text)));
const fixed = {
  widths,
  spaceWidth: 1024,
  hyphenWidth: 682,
  ascent: 3604,
  descent: 901,
  leading: 819,
};

const warm = kp.break(compiled, fixed);
const samples = [];
for (let i = 0; i < 9; i += 1) {
  const started = performance.now();
  kp.break(compiled, fixed);
  samples.push(performance.now() - started);
}
samples.sort((a, b) => a - b);
const ms = samples[Math.floor(samples.length / 2)];
report(
  'W4-time',
  compiled.atoms.length > 1500 && ms < 25,
  `${compiled.atoms.length} atoms, ${ms.toFixed(2)} ms median, ${warm.lines.length} lines`,
);

const before = kp.memory.buffer.byteLength;
for (let i = 0; i < 50; i += 1) {
  kp.break(compiled, fixed);
}
const after = kp.memory.buffer.byteLength;
report('W5-memory', before === after, `${before} -> ${after} bytes after 50 breaks`);

// A narrow measure that needs the ladder, then the host policy: greed.
const narrow = kp.compile(paragraph, {
  width: 20 * UNITS_PER_EM,
  lineHeight: Math.round(1.5 * UNITS_PER_EM),
});
const narrowWidths = Int32Array.from(narrow.atoms.map((atom) => widthOf(atom.text)));
const narrowFixed = { ...fixed, widths: narrowWidths };
const started = performance.now();
const kpResult = kp.break(narrow, narrowFixed);
const kpMs = performance.now() - started;
const greedyStarted = performance.now();
const greedy = kp.break(narrow, { ...narrowFixed, greedy: true });
const greedyMs = performance.now() - greedyStarted;
report(
  'W6-policy',
  greedy.lines.length > 0 && !greedy.fallback,
  `K-P ${kpMs.toFixed(2)} ms (fallback=${kpResult.fallback}), greedy ${greedyMs.toFixed(2)} ms, ` +
    `${greedy.lines.length} lines`,
);

// A 400-atom paragraph is the common case; keep it inside a frame even here.
const small = kp.compile('中文English混排，数字 123，排版Engine v2。'.repeat(12), {
  width: 40 * UNITS_PER_EM,
  lineHeight: Math.round(1.5 * UNITS_PER_EM),
});
const smallWidths = Int32Array.from(small.atoms.map((atom) => widthOf(atom.text)));
const smallStarted = performance.now();
kp.break(small, { ...fixed, widths: smallWidths });
const smallMs = performance.now() - smallStarted;
report('W4-typical', smallMs < 5, `${small.atoms.length} atoms, ${smallMs.toFixed(2)} ms`);

compiled.free();
narrow.free();
small.free();

const failed = rows.filter((row) => row[1] === 'FAIL');
if (failed.length) {
  console.error(`${failed.length} check(s) failed`);
  process.exit(1);
}
console.log('node checks passed');
