// soma-kp.js — the browser side of the soma-typeset K-P slice.
//
// The wasm module has no imports and no glue. This file is the only glue:
// it writes an integer span table into linear memory, measures the compiled
// atoms with canvas `measureText`, and calls the DP.
//
// Public surface:
//
//   const kp = await Kp.load('./soma-kp.wasm');
//   const p  = kp.compile('这一段中文 English 混排。', { width, lineHeight });
//   const r  = kp.break(p);                 // measures, then runs K-P
//   kp.renderToDOM(el, p, r);               // one <div> per line
//   p.free();
//
// Units: every width crossing the boundary is an integer in 1/4096 em.

export const UNITS_PER_EM = 4096;

const SPAN_WORDS = 5;
const ATOM_WORDS = 6;
const IN_HEADER_WORDS = 8;
const LINE_WORDS = 9;
const BOX_WORDS = 3;
const OUT_HEADER_WORDS = 4;

const SPAN_TEXT = 0;
const SPAN_CODE = 1;
const SPAN_OBJECT = 2;
const SPAN_HARD_BREAK = 3;

export const RESULT_FALLBACK = 1;
export const RESULT_LENGTH_LIMITED = 2;
export const LINE_OVERFULL = 1;
export const LINE_EMERGENCY = 2;
export const LINE_FORCED_END = 4;
export const LINE_HYPHENATED = 8;

const ERR_NAMES = {
  0: 'ok',
  1: 'invalid handle',
  2: 'invalid utf-8',
  3: 'rules rejected the paragraph',
  4: 'no feasible break',
  5: 'buffer too small',
  6: 'invalid parameters',
};

/**
 * A compiled paragraph: atoms plus their compression limits.
 *
 * `break()` does not recompile it, so typing inside one paragraph costs one
 * `compile` and one break per layout width.
 */
export class CompiledParagraph {
  constructor(kp, handle, atoms, text, style) {
    this.kp = kp;
    this.handle = handle;
    this.atoms = atoms;
    this.text = text;
    this.style = style;
    this.widths = null;
    this.spaceWidth = 0;
    this.hyphenWidth = 0;
    this.vertical = null;
  }

  /** Release the wasm handle. Safe to call twice. */
  free() {
    if (this.handle) {
      this.kp.exports.soma_para_free(this.handle);
      this.handle = 0;
    }
  }
}

export class Kp {
  /** Fetch and instantiate the module. */
  static async load(url) {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`soma-kp: cannot load ${url} (${response.status})`);
    }
    return Kp.fromBytes(await response.arrayBuffer());
  }

  /** Instantiate from an ArrayBuffer (a bundler or an inline base64 blob). */
  static async fromBytes(bytes) {
    const { instance } = await WebAssembly.instantiate(bytes, {});
    return new Kp(instance);
  }

  static fromInstance(instance) {
    return new Kp(instance);
  }

  constructor(instance) {
    this.exports = instance.exports;
    this.memory = instance.exports.memory;
    this.encoder = new TextEncoder();
    this.decoder = new TextDecoder();
    this.ctx = null;
    this.widthCache = new Map();
  }

  _alloc(bytes) {
    const ptr = this.exports.soma_alloc(bytes);
    if (!ptr) {
      throw new Error(`soma-kp: soma_alloc(${bytes}) failed`);
    }
    return ptr;
  }

  _free(ptr, bytes) {
    if (ptr) {
      this.exports.soma_dealloc(ptr, bytes);
    }
  }

  _error() {
    const code = this.exports.soma_last_error();
    return new Error(`soma-kp: ${ERR_NAMES[code] || `error ${code}`}`);
  }

  // --------------------------------------------------------------- compile

  /**
   * Compile text (or a styled span list) into atoms.
   *
   * @param {string|Array<{text: string, style?: number, kind?: string,
   *        decor?: number}>} input
   * @param {object} style
   *   `width` (1/4096 em, required), `firstIndent`, `hang`, `tracking`,
   *   `lineHeight` (all 1/4096 em), `align` ('justify' | 'left').
   */
  compile(input, style = {}) {
    const spans = typeof input === 'string' ? [{ text: input }] : input.slice();
    const width = style.width;
    if (!(width > 0)) {
      throw new Error('soma-kp: style.width must be positive (1/4096 em)');
    }
    const lineHeight = style.lineHeight || Math.round(width / 8);

    // One UTF-8 blob with per-span offsets.
    const encoded = spans.map((span) => this.encoder.encode(span.text ?? ''));
    const blobLength = encoded.reduce((sum, bytes) => sum + bytes.length, 0);
    const blob = new Uint8Array(blobLength);
    const table = new Uint32Array(Math.max(spans.length, 1) * SPAN_WORDS);
    const styleWords = new Int32Array(7);
    let offset = 0;
    spans.forEach((span, index) => {
      const bytes = encoded[index];
      blob.set(bytes, offset);
      table[index * SPAN_WORDS] = offset;
      table[index * SPAN_WORDS + 1] = bytes.length;
      const kind =
        span.kind === 'code'
          ? SPAN_CODE
          : span.kind === 'object'
            ? SPAN_OBJECT
            : span.kind === 'hardBreak'
              ? SPAN_HARD_BREAK
              : SPAN_TEXT;
      table[index * SPAN_WORDS + 2] = (span.style || 0) | (kind << 8);
      table[index * SPAN_WORDS + 3] = span.decor || 0;
      table[index * SPAN_WORDS + 4] = 0;
      offset += bytes.length;
    });
    styleWords[0] = width;
    styleWords[1] = style.firstIndent || 0;
    styleWords[2] = style.hang || 0;
    styleWords[3] = style.tracking || 0;
    styleWords[4] = lineHeight;
    styleWords[5] = style.styleId || 0;
    styleWords[6] = style.align === 'left' ? 1 : 0;

    const blobPtr = blobLength ? this._alloc(blobLength) : 0;
    const tablePtr = this._alloc(table.byteLength);
    const stylePtr = this._alloc(styleWords.byteLength);
    try {
      if (blobLength) {
        new Uint8Array(this.memory.buffer, blobPtr, blobLength).set(blob);
      }
      new Uint32Array(this.memory.buffer, tablePtr, table.length).set(table);
      new Int32Array(this.memory.buffer, stylePtr, 7).set(styleWords);
      const handle = this.exports.soma_para_new(
        stylePtr,
        tablePtr,
        spans.length,
        blobPtr,
        blobLength,
      );
      if (!handle) {
        throw this._error();
      }
      const words = this.exports.soma_para_words(handle);
      const dumpPtr = this._alloc(words * 4);
      let atoms;
      let text;
      try {
        const written = this.exports.soma_para_dump(handle, dumpPtr, words);
        if (!written) {
          throw this._error();
        }
        // Views are created after the call: the module may have grown memory.
        const out = new Uint32Array(this.memory.buffer, dumpPtr, written);
        const atomCount = out[0];
        const blobWords = out[2];
        text = this.decoder.decode(
          new Uint8Array(this.memory.buffer, dumpPtr + blobWords * 4, out[1]),
        );
        atoms = new Array(atomCount);
        let cursor = 3;
        let textCursor = 0;
        for (let i = 0; i < atomCount; i += 1) {
          const meta = out[cursor];
          const utf16 = out[cursor + 3];
          atoms[i] = {
            kind: meta & 0xff,
            punct: (meta >> 8) & 0xff,
            style: (meta >> 16) & 0xff,
            text: text.slice(textCursor, textCursor + utf16),
            compressLeft: out[cursor + 4] | 0,
            compressRight: out[cursor + 5] | 0,
          };
          textCursor += utf16;
          cursor += ATOM_WORDS;
        }
      } finally {
        this._free(dumpPtr, words * 4);
      }
      return new CompiledParagraph(this, handle, atoms, text, {
        width,
        firstIndent: style.firstIndent || 0,
        hang: style.hang || 0,
        tracking: style.tracking || 0,
        lineHeight,
        align: style.align || 'justify',
      });
    } finally {
      this._free(tablePtr, table.byteLength);
      this._free(stylePtr, styleWords.byteLength);
      this._free(blobPtr, blobLength);
    }
  }

  // --------------------------------------------------------------- measure

  /** The 2D context used for `measureText` (created lazily). */
  get context() {
    if (!this.ctx) {
      const canvas =
        typeof OffscreenCanvas !== 'undefined'
          ? new OffscreenCanvas(8, 8)
          : typeof document !== 'undefined'
            ? document.createElement('canvas')
            : null;
      if (!canvas) {
        throw new Error('soma-kp: no canvas available; pass widths to break() instead');
      }
      this.ctx = canvas.getContext('2d');
    }
    return this.ctx;
  }

  /** Quantise a CSS-pixel advance to 1/4096 em. */
  static quantise(px, sizePx) {
    return Math.round((px / sizePx) * UNITS_PER_EM);
  }

  /**
   * Measure every atom of `compiled`.
   *
   * @param {{font: string, sizePx: number, ascent?: number, descent?: number,
   *          leading?: number}} face  `font` is a CSS font shorthand at
   *          `sizePx`; the vertical numbers are in 1/4096 em and default to
   *          ascent 0.88 em, descent 0.22 em, leading 0.2 em.
   */
  measure(compiled, face) {
    const ctx = this.context;
    const cache = this.widthCache;
    const widths = new Int32Array(compiled.atoms.length);
    ctx.font = face.font;
    for (let i = 0; i < compiled.atoms.length; i += 1) {
      const atom = compiled.atoms[i];
      const key = `${atom.style}|${atom.text}`;
      let units = cache.get(key);
      if (units === undefined) {
        units = Kp.quantise(ctx.measureText(atom.text).width, face.sizePx);
        cache.set(key, units);
      }
      widths[i] = units;
    }
    compiled.spaceWidth = Kp.quantise(ctx.measureText(' ').width, face.sizePx);
    compiled.hyphenWidth = Kp.quantise(ctx.measureText('-').width, face.sizePx);
    compiled.widths = widths;
    compiled.vertical = {
      ascent: face.ascent ?? Math.round(UNITS_PER_EM * 0.88),
      descent: face.descent ?? Math.round(UNITS_PER_EM * 0.22),
      leading: face.leading ?? Math.round(UNITS_PER_EM * 0.2),
    };
    return compiled;
  }

  // ----------------------------------------------------------------- break

  /**
   * Break a compiled paragraph.
   *
   * @param {CompiledParagraph} compiled
   * @param {object} [options]
   *   `widths` (Int32Array) and `spaceWidth`, `hyphenWidth`, `ascent`,
   *   `descent`, `leading` — skip measuring and use these values (the harness
   *   and any faithful-metrics host do this);
   *   `greedy` — use Greedy-C instead of K-P.
   */
  break(compiled, options = {}) {
    if (compiled.handle === 0) {
      throw new Error('soma-kp: paragraph was freed');
    }
    const atomCount = compiled.atoms.length;
    const input = new Int32Array(IN_HEADER_WORDS + atomCount);
    let widths = options.widths || compiled.widths;
    if (!widths) {
      this.measure(compiled, options.face || defaultFace());
      widths = compiled.widths;
    }
    const vertical = compiled.vertical || {
      ascent: Math.round(UNITS_PER_EM * 0.88),
      descent: Math.round(UNITS_PER_EM * 0.22),
      leading: Math.round(UNITS_PER_EM * 0.2),
    };
    input[0] = options.spaceWidth ?? compiled.spaceWidth;
    input[1] = options.hyphenWidth ?? compiled.hyphenWidth;
    input[2] = options.ascent ?? vertical.ascent;
    input[3] = options.descent ?? vertical.descent;
    input[4] = options.leading ?? vertical.leading;
    input.set(widths, IN_HEADER_WORDS);

    const inputPtr = this._alloc(input.byteLength);
    try {
      new Int32Array(this.memory.buffer, inputPtr, input.length).set(input);
      const run = options.greedy ? this.exports.soma_break_greedy : this.exports.soma_break;
      // Every line holds at least one atom, so `12 * atoms` words covers the
      // worst case (9 per line + 3 per box). Asking the engine for the size
      // first would run the DP twice — that doubled the cost of every break.
      const capacity = OUT_HEADER_WORDS + 12 * atomCount + 8;
      const outPtr = this._alloc(capacity * 4);
      try {
        let written = run(compiled.handle, inputPtr, input.length, outPtr, capacity);
        if (!written) {
          throw this._error();
        }
        if (written > capacity) {
          // The module reports the size it needs instead of writing; treat it
          // as a bug, because the bound above is exact.
          throw new Error(`soma-kp: output buffer too small (${written} > ${capacity})`);
        }
        const out = new Int32Array(this.memory.buffer, outPtr, written);
        const lineCount = out[0];
        const boxCount = out[1];
        const flags = out[2];
        const lines = new Array(lineCount);
        let cursor = OUT_HEADER_WORDS;
        for (let i = 0; i < lineCount; i += 1) {
          lines[i] = {
            start: out[cursor],
            end: out[cursor + 1],
            trimLeft: out[cursor + 2],
            trimRight: out[cursor + 3],
            adjustment: out[cursor + 4],
            ascent: out[cursor + 5],
            descent: out[cursor + 6],
            leading: out[cursor + 7],
            flags: out[cursor + 8],
          };
          cursor += LINE_WORDS;
        }
        const boxes = new Array(boxCount);
        for (let i = 0; i < boxCount; i += 1) {
          boxes[i] = { atom: out[cursor], x: out[cursor + 1], advance: out[cursor + 2] };
          cursor += BOX_WORDS;
        }
        return {
          lines,
          boxes,
          fallback: (flags & RESULT_FALLBACK) !== 0,
          lengthLimited: (flags & RESULT_LENGTH_LIMITED) !== 0,
          widths,
        };
      } finally {
        this._free(outPtr, capacity * 4);
      }
    } finally {
      this._free(inputPtr, input.byteLength);
    }
  }

  /** Convenience: compile, measure, break, free. */
  layout(input, style, options = {}) {
    const compiled = this.compile(input, style);
    try {
      return this.break(compiled, options);
    } finally {
      compiled.free();
    }
  }

  // --------------------------------------------------------------- render

  /** The atoms of one line, with the hyphen the engine counted appended. */
  lineText(compiled, line) {
    let text = '';
    for (let i = line.start; i < line.end; i += 1) {
      text += compiled.atoms[i].text;
    }
    return line.flags & LINE_HYPHENATED ? `${text}-` : text;
  }

  /**
   * Render one `<div>` per line.
   *
   * Every line already fits the measure (the DP guarantees it, and an overfull
   * emergency line is marked), so the browser has no reason to re-wrap: the
   * break you see is the break the engine chose.
   */
  renderToDOM(container, compiled, result, options = {}) {
    const sizePx = options.sizePx || 16;
    const doc = container.ownerDocument || document;
    container.textContent = '';
    const justify = (compiled.style.align || 'justify') !== 'left';
    result.lines.forEach((line, index) => {
      const element = doc.createElement('div');
      element.className = 'soma-line';
      element.textContent = this.lineText(compiled, line);
      const last = index === result.lines.length - 1 || (line.flags & LINE_FORCED_END) !== 0;
      if (justify && !last) {
        element.style.textAlign = 'justify';
        element.style.textAlignLast = 'justify';
      }
      if (line.trimLeft) {
        element.style.marginLeft = `${(line.trimLeft / UNITS_PER_EM) * sizePx}px`;
      }
      if (line.trimRight) {
        element.style.marginRight = `${(-line.trimRight / UNITS_PER_EM) * sizePx}px`;
      }
      if (line.flags & LINE_OVERFULL) {
        element.dataset.overfull = '1';
        element.style.whiteSpace = 'nowrap';
      }
      container.appendChild(element);
    });
  }

  /** Render by absolute box coordinates, the same numbers the PDF gets. */
  renderToCanvas(ctx, compiled, result, options = {}) {
    const sizePx = options.sizePx || 16;
    const scale = sizePx / UNITS_PER_EM;
    const lineGap = options.lineGap ?? compiled.style.lineHeight;
    ctx.font = options.font || defaultFace().font;
    ctx.textBaseline = 'alphabetic';
    let y = options.y || 0;
    let boxIndex = 0;
    for (const line of result.lines) {
      const baseline = y + line.ascent * scale;
      for (; boxIndex < result.boxes.length; boxIndex += 1) {
        const box = result.boxes[boxIndex];
        if (box.atom < line.start) continue;
        if (box.atom >= line.end) break;
        ctx.fillText(compiled.atoms[box.atom].text, (options.x || 0) + box.x * scale, baseline);
      }
      y += lineGap * scale;
    }
  }
}

function defaultFace() {
  return {
    font: '400 16px "Noto Serif CJK SC", "Source Han Serif SC", serif',
    sizePx: 16,
  };
}

export default Kp;
