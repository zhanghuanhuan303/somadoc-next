# soma-kp — 浏览器里的断行切片

`soma-typeset` 的行内 K-P 内核，编译成一个 **32 KB（gzip）、零 import** 的
WebAssembly 模块。规则层（标点类、禁则、行边裁剪、断词）一起打包，所以浏览器里
用的不是「另一套近似规则」，而是与 PDF 同一个引擎。

```
soma-kp.wasm     32 KB gzip   90 KB raw   0 imports
soma-kp.js       ~14 KB       无依赖 ES module
soma-kp.worker.js ~2 KB       Worker 包装
```

没有 npm 依赖、没有打包器、没有 CDN。`<script type="module">` 直接吃。

---

## 1. 快速开始

```html
<div id="page" style="font: 16px/1.65 serif; width: 512px"></div>

<script type="module">
  import { Kp, UNITS_PER_EM } from './soma-kp.js';

  const kp = await Kp.load('./soma-kp.wasm');

  const compiled = kp.compile('中文排版的核心是把规则变成可调的代价模型。', {
    width: 32 * UNITS_PER_EM,      // 栏宽 32 em
    firstIndent: 2 * UNITS_PER_EM, // 首行缩进
    lineHeight: Math.round(1.65 * UNITS_PER_EM),
  });

  const result = kp.break(compiled, { face: { font: '400 16px serif', sizePx: 16 } });
  kp.renderToDOM(document.getElementById('page'), compiled, result, { sizePx: 16 });

  compiled.free();
</script>
```

**必须用 HTTP 打开**（`python3 -m http.server` 即可）：`file://` 下浏览器禁止
`fetch` 一个 wasm。临时预览可以给 Chromium 加 `--allow-file-access-from-files`。

```sh
cd soma-typeset
python3 -m http.server 8000
# 打开 http://localhost:8000/web/demo.html
```

---

## 2. 单位

所有跨边界的宽度都是整数：**1 em = 4096 单位**。这是整个引擎的定点基准
（PDF 后端、canonical JSON 用的都是它），不是 web 层自造的。

`16px` 的字号下，1 个单位 ≈ 0.0039 px，量化误差远小于一个像素。

---

## 3. API

### `Kp.load(url)` / `Kp.fromBytes(buffer)` / `Kp.fromInstance(instance)`

三种加载方式。`fromBytes` 适合把 wasm 内联成 base64 的单文件发布。

### `kp.compile(input, style) -> CompiledParagraph`

`input` 是字符串，或 span 数组：

```js
kp.compile([
  { text: '普通文字，' },
  { text: '加粗片段', style: 1 },
  { text: '，' },
  { text: '斜体片段', style: 2 },
], { width: 512 * 4096 });
```

| span 字段 | 值 |
|---|---|
| `text` | 字符串（UTF-8 由模块处理） |
| `style` | 0–255，对应宿主自己的字体样式 id |
| `kind` | `'text'`（默认）/ `'code'` / `'object'` / `'hardBreak'` |
| `decor` | 位标记：1 下划线、2 删除线（只影响渲染，不影响断行） |

`'hardBreak'` 会切成两个强制段（像 Markdown 的 `\` 换行）；`'object'` 用于行内
公式/图片——宽度由你自己的 `widths[idx]` 给出。

`style` 参数：

| 字段 | 单位 | 说明 |
|---|---|---|
| `width` | 1/4096 em | **必填**，> 0 |
| `firstIndent` | 1/4096 em | 默认 0 |
| `hang` | 1/4096 em | 悬挂缩进（列表）；与 `firstIndent` 不同时为 0 |
| `tracking` | 1/4096 em | 字距；加在每个原子之间，不加在行尾 |
| `lineHeight` | 1/4096 em | 默认 `width / 8` |
| `align` | — | `'justify'`（默认）或 `'left'` |

### `kp.measure(compiled, face)`

用 canvas 逐原子量宽，结果缓存（键是 `style|text`）在 `kp.widthCache` 里。

| face 字段 | 说明 |
|---|---|
| `font` | CSS font 简写，例如 `'700 16px "Noto Serif CJK SC", serif'` |
| `sizePx` | 与 `font` 里的字号一致；量化要用它 |
| `ascent` / `descent` / `leading` | 可选的纵向度量（1/4096 em），默认 0.88 / 0.22 / 0.2 em |

`compiled.widths` 是这次测量的结果，`break()` 会复用它——**改宽度不用重量，改字号要重量**。

### `kp.break(compiled, options) -> result`

`options` 全部可选：

| 字段 | 说明 |
|---|---|
| `widths` | `Int32Array`，跳过 canvas，直接用这份宽度（测试、`faithful` 模式） |
| `spaceWidth` / `hyphenWidth` / `ascent` / `descent` / `leading` | 同上 |
| `greedy` | `true` 用 Greedy-C（降级路径） |

返回：

```js
{
  lines: [{ start, end, trimLeft, trimRight, adjustment,
            ascent, descent, leading, flags }],
  boxes: [{ atom, x, advance }],
  fallback: boolean,       // 走了回退阶梯（见 §5）
  lengthLimited: boolean,  // 超长段被降级成 greedy
  widths,                  // 实际使用的宽度表
}
```

`lines[i].start/end` 是原子下标；`boxes` 是扁平的、按行顺序排列的
`{atom, x, advance}`，坐标单位 1/4096 em。**渲染器直接用它，不要自己算 x**。

`flags` 位：1 overfull（溢宽）、2 emergency、4 forced_end、8 hyphenated。

### 渲染

```js
kp.renderToDOM(el, compiled, result, { sizePx: 16 });   // 一行一个 <div>
kp.renderToCanvas(ctx, compiled, result, { sizePx: 16 }); // 按 box 坐标画
kp.lineText(compiled, line);                            // 某行的文本（含连字符）
```

DOM 模式：每行文本都**恰好**放得下（DP 保证），所以浏览器不会二次换行——
你看到的断点就是引擎的断点。溢宽的 emergency 行会带 `data-overfull="1"`。

Canvas 模式：坐标来自 `boxes`，与 PDF/SVG 后端同一组数字。

---

## 4. 缓存与性能

两级缓存，对应两种交互：

| 变化 | 需要重做什么 | 实测（1800 atoms，40 em） |
|---|---|---|
| 改字号/字体 | `measure` + `break` | 0.3 ms + 7.2 ms |
| 改栏宽 | 只 `break` | 7.2 ms |
| 改文字 | 重新 `compile` + `measure` + `break` | 0.2 ms + 0.3 ms + 7.2 ms |
| 典型段（180 atoms） | 全部 | 0.18 ms |

注：上面是 node 里对 **标点密集** 的合成段（每行 ~41 个原子）的实测；
纯中文 2000 atoms 约 1.3 ms。`web/check.mjs` 每次跑门禁都会重新测。

`break()` 只调用一次 wasm：输出缓冲按 `12 × atoms` 个词一次分配
（每行 9 + 每 box 3），不问引擎要大小——**问一次会把 DP 跑两遍**。

---

## 5. 回退策略（D94）

引擎有四级回退：正常 → 放宽容差 → 允许 emergency 行 → 允许禁则断点。
第三、四级在窄栏上曾经是 O(n²)：2000 个原子评估 **1,255,320** 次候选，浏览器里 40 ms。
现在候选窗口被 `emergency_stretch` 界住（同段 30,316 次，2.5 ms，断点逐字不变，见 ADR-0072）。

即便如此，宿主要做的事只有一件：

```js
let result = kp.break(compiled);
if (result.fallback) {
  result = kp.break(compiled, { greedy: true });  // ≤1.5 ms，视觉差异在一段内不可辨
}
```

`web/soma-kp.worker.js` 已经内置这条策略。

**主线程阈值**：段落 ≤ 800 atoms 直接同步跑；更长或需要回退的，放 Worker
（`new Worker(url, { type: 'module' })`，消息格式见文件头）。

---

## 6. 给非 JS 宿主：C ABI

模块只有这几个导出，全部整数、全部线性内存：

| 导出 | 用途 |
|---|---|
| `soma_alloc(len) -> ptr` / `soma_dealloc(ptr, len)` | 输入缓冲 |
| `soma_para_new(style, spans, count, blob, blobLen) -> handle` | 编译段落 |
| `soma_para_words(handle) -> words` | 原子表大小 |
| `soma_para_dump(handle, out, cap) -> words` | 原子表 + UTF-8 文本块 |
| `soma_break(handle, input, inLen, out, outCap) -> words` | K-P |
| `soma_break_greedy(...)` | Greedy-C |
| `soma_para_free(handle)` | 释放 |
| `soma_last_error() -> code` | 0 成功、5 缓冲不足、6 参数错… |

原子表布局（`u32`，每原子 6 个词）：

```
[0] atom_count    [1] blob_byte_len    [2] blob_word_offset
每个原子: kind|class<<8|style<<16, text_byte_start, text_byte_len,
          text_utf16_len, compress_left, compress_right
之后: UTF-8 文本拼接（按 word 补齐）
```

断行输入：`space, hyphen, ascent, descent, leading, 0, 0, 0`，然后每原子一个
`advance`。输出：`line_count, box_count, flags, 0`，然后每行 9 个词、每 box 3 个词。
`crates/soma-web/src/lib.rs` 的文档注释是权威版本。

---

## 7. 重新构建与门禁

```sh
bash scripts/web.sh
```

它做六件事（W1–W6）：

| 编号 | 检查 | 当前 |
|---|---|---|
| W1 | `WebAssembly.Module.imports().length == 0` | 0 |
| W2 | gzip ≤ 40 KB | 32.7 KB |
| W3 | 与 native 断点逐项一致（`web/parity.txt`） | 34 atoms，6 行 |
| W4 | 1800 atoms ≤ 25 ms、180 atoms ≤ 5 ms | 7.2 ms / 0.18 ms |
| W5 | 连续 50 次 break 内存不增长 | 1638400 → 1638400 |
| W6 | 窄栏能降级到 greedy | 4.2 ms → 1.8 ms |

`web/parity.txt` 是三个消费者共用的同一份事实：Rust 的 ABI 测试、
Chromium 自检页、以及生成它的 native 例子。改了规则层或 DP 之后：

```sh
cargo run -p soma-web --example fixture > web/parity.txt
```

并在提交信息里解释断点变化——否则浏览器和 PDF 会悄悄跑偏。

单独看浏览器自检：

```sh
chromium --headless --allow-file-access-from-files \
  --virtual-time-budget=20000 --dump-dom \
  "file://$PWD/web/harness.html" | grep -oE 'W[1-6]-[a-z]+\|(ok|FAIL)\|[^<]*'
```

---

## 8. 边界（写清楚，免得误用）

- **只做行内断行。** 分页、脚注、目录、参考文献都在服务端/构建期。
- **preview 口径。** 宽度来自浏览器自己的 `measureText`，所以「所见即所排」；
  但它与 PDF 的 `.smt` 度量表可能有 1–2% 的差（D95），断点因此可能不同。
  `break(p, { widths })` 已经允许换成任何度量源，`faithful` 模式留给后续。
- **不做增量断行。** 改一个字要重编整段；两级缓存已经把常见的拖拽/改字号降到只跑 DP。
- **不支持 bidi、竖排、多栏绕排。**
- **`style` 上限 255**（原子表里只留 8 位）。一篇文章的字体样式远到不了。
