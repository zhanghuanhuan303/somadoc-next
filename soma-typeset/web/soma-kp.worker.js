// soma-kp.worker.js — run compile + measure + break off the main thread.
//
// Messages in:  { id, text, spans, style, face }
// Messages out: { id, lines: [{ text, flags, trimLeft, trimRight }], ms, fallback, error }
//
// The worker owns its own OffscreenCanvas, so measurement happens with the same
// font it will be drawn with. Load it with `new Worker(url, { type: 'module' })`.

import { Kp, UNITS_PER_EM } from './soma-kp.js';

let kp = null;

async function ensure(wasmUrl) {
  if (!kp) {
    kp = await Kp.load(wasmUrl);
  }
  return kp;
}

self.onmessage = async (event) => {
  const { id, wasmUrl, text, spans, style, face, greedy } = event.data;
  try {
    const engine = await ensure(wasmUrl);
    const compiled = engine.compile(spans || text, style);
    try {
      const started = performance.now();
      let result = engine.break(compiled, { face, greedy });
      if (!greedy && result.fallback) {
        // D94 policy: the emergency stage can be an order of magnitude slower
        // than Greedy-C and looks nearly the same on one paragraph.
        result = engine.break(compiled, { face, greedy: true });
      }
      const ms = performance.now() - started;
      self.postMessage({
        id,
        ms,
        fallback: result.fallback,
        lines: result.lines.map((line) => ({
          text: engine.lineText(compiled, line),
          flags: line.flags,
          trimLeft: line.trimLeft,
          trimRight: line.trimRight,
          adjustment: line.adjustment,
          ascent: line.ascent,
          height: line.ascent + line.descent + line.leading,
        })),
        unitsPerEm: UNITS_PER_EM,
      });
    } finally {
      compiled.free();
    }
  } catch (error) {
    self.postMessage({ id, error: String(error && error.message ? error.message : error) });
  }
};
