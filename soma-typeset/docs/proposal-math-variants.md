# 提案：用 OpenType MATH 变体字形重做数学的「大」与「根」

- 状态：**已拍板并实现**，决策记录见 ADR-0075（变体）与 ADR-0076（Typst 对齐：根号净空、轴向居中、重音锚点；表版本落到 `soma.metrics/7`）
- 日期：2026-11-24
- 范围：`tools/metrics-gen`、`soma.metrics/6`、`soma-core`（带默认值的度量钩子）、
  `soma-math`（大算符、根式、定界符）、`soma-layout`（适配器、`soma.pages/11`）、
  `soma-render-pdf`（按字形号绘制）。**不动 K-P、断行、模板、DocIR。**
- 相关：ADR-0026/0027/0030/0053/0070；D93

---

## 0. 一句话

`∑` 变粗和 `√` 「断腿」不是两个孤立的绘制 bug，而是同一个设计错误：
**我们用「把正文尺寸的字形整体放大」来代替字体里已经设计好的大号字形。**
本提案按 TeX / Typst 的通行做法接上 OpenType `MATH` 的 `MathVariants`：
排版挑「设计好的变体字形」，渲染按**字形号**绘制；同时修正根式的
vinculum 接缝与根指数定位（这两处还有独立的坐标错误）。

---

## 1. 现象与根因

在 `examples/book/out/math-v2.pdf` 第 1 页（DejaVu Math TeX Gyre，400 dpi）
可以逐条看到：

| 现象 | 代码位置 | 根因 |
|---|---|---|
| `∑`、`∏`、`∫` 笔画明显粗于周边字母 | `soma-math` `big()` | display 时把 1 em 的基础字形**整体放大**到 `displayOperatorMinHeight`（DejaVu 1333‰、Noto 2300‰）。笔画随比例变粗，而字体里本来就有 1.51 em / 1.34 em 的 display 变体字形 |
| `√` 的横线与根号之间有一条缝 | `radical()` 的 `body_x = surd_x + glyph_advance`，横线从 `body_x` 开始 | 字形的**墨迹右边界**并不在 advance 上。DejaVu `√` 的 advance 875、墨迹右端 719（gap 156‰）；高变体 gap 更大。横线应从墨迹右端（再压一条线宽）起笔，而不是从 advance 起笔 |
| `∛(a/b)` 的根号被放大到 ~2.5 em，底部伸到分数线以下 | `radical()` 的 `while` 循环每次 ×9/8 | 同 `∑`：放大基础字形，而不是选字体里的 1.4 / 1.8 / 2.1 / 2.5 em 设计变体 |
| 根指数飘在左上角远处 | `radical()`：`raise = scale(script_size, radical_degree_raise) / 1000`；`surd_x = self.cursor + index_width` | ① `radicalDegreeBottomRaisePercent` 是**根号高度的百分比**，不是 em 的长度，量纲错了（实际 raise≈0）；② 放置指数后 `cursor` 已经前进，再把它加进 `surd_x`，横向重复计数，于是根号和指数之间空出一大截 |
| `\left(…\right)` 的 3 em 括号同样变粗 | `fence_geometry()` / `delimiter()` | D93 的同一条：连续缩放，不取变体 |

字体侧的证据（`tools/math-variants-report.py` 的测量）：

```text
Noto Sans Math   displayOperatorMin 2300‰   √ 6 个纵向变体（1.06…2.87 em）
DejaVu Math      displayOperatorMin 1333‰   √ 7 个纵向变体（0.87…2.87 em）
Noto ∑ 变体      adv 954 (ink 954) → 1336 (ink 1336)
DejaVu ∑ 变体    adv 1051 (ink 1050) → 1513 (ink 1512)
```

三款已安装的数学字体（Noto Sans Math、DejaVu Math TeX Gyre、TeX Gyre
Pagella Math）的纵向 `GlyphAssembly` **part 数都是 0**。也就是说：当前的
内容完全能由「变体列表」覆盖，装配件没有可用测试对象（见 §7）。

---

## 2. 方案

### 2.1 数据：`soma.metrics/6` 读 `MathVariants`

`metrics-gen` 对 `family == math` 的面解析 `MATH → MathVariants →
vertGlyphCoverage/vertGlyphConstruction`，把每个**可达**基础字形（能通过
cmap 反查到码位）的变体列表写进表：

```text
（v5 段的 math glyphs 之后）
u16 math_variant_base_count
per base:
  varint(codepoint delta)
  u16 variant_count
  per variant:
    i32 width         (横向 advance，1/4096 em)
    i16 top           (墨迹在基线上方)
    i16 bottom        (墨迹在基线下方)
    i16 right_gap     (advance - 墨迹右端，<0 时记 0)
```

- 变体字形通常**不在 cmap 里**（Noto 119 个变体里只有 13 个可编码，且都是
  基础字形本身），所以表里存的是**序号 + 度量**，不是码位。
- 只有 `family == 2` 的面写非零计数；文本面写 `u16 0`，成本 2 字节。
- 版本号升到 6；v1–v5 的旧表继续可读（没有变体段，行为与今天一致）。

### 2.2 度量缝：`soma-core` 增加两个**带默认值**的钩子

```rust
pub struct MathVariant {
    pub advance: Width,
    pub above: Width,
    pub below: Width,
    pub right_gap: Width,
}

fn math_variant_count(&self, _text: &str, _style: StyleId) -> u16 { 0 }
fn math_variant(&self, _text: &str, _index: u16, _style: StyleId) -> Option<MathVariant> { None }
```

默认返回「没有变体」，因此任何自定义 `Metrics` 宿主的输出字节不变
（ADR-0007 允许带默认值的扩展）。`soma-layout` 的两个适配器从表的
math face 回答；没有数学表时保持旧路径。

### 2.3 `soma-math`：选择算法（TeX 的规则）

**大算符（`big()`）**

```text
needed = display ? scale(size, display_operator_min) : 0
从 0 号变体起找第一个 (above + below) >= needed；
都不够就用最后一个（字体最大的设计字形，不做放大）。
```

挑中变体后：

- 字形按**设计尺寸** `size` 绘制（不是 `needed`），笔画是设计好的粗细；
- 横向 advance、纵向 ink 都取变体表的值；
- 上下限的位置用变体的 `above/below`；
- 没有变体（宿主没实现钩子 / 旧表）→ 完全保持今天的行为：基础字形放到
  `scale(size, display_operator_min)`。

**根式（`radical()`）**

```text
needed  = 被开方数墨迹高 + 深 + 线宽 + radicalExtraAscender
变体    = 第一个 ink 高 >= needed 的纵向变体，否则最后一个
surd_x  = start + before + index_width + after      // start 是进入根式前的 cursor
bar_y   = baseline - body_h - rule
surd.y  = bar_y + above                              // 墨迹顶贴横线
body_x  = surd_x + advance
bar_x   = surd_x + advance - right_gap - rule        // 从墨迹右端起笔，压一条线宽
bar_w   = body_x + body_w - bar_x
```

根指数（OpenType MATH 的定义）：

```text
raise          = (above + below) * radicalDegreeBottomRaisePercent / 1000
degree_bottom  = bar_y + above + below - raise
degree_baseline= degree_bottom - index_depth
degree_x       = start + before
```

`radical_kern_before/after` 改为**有符号**（字体给的是 +51 / −450‰，
现在的 `u16` 把负值夹成 0，等于丢掉字体给的收进量）。

**定界符（`fence_geometry()` / `delimiter()`）**

同一个选择器：按内容墨迹高度挑变体；每侧按自己的变体算基线偏移
（把字形墨迹中心对到内容墨迹中心）。无变体时退回今天的连续缩放。

**回退尺寸**

SVG/HTML 输出的是文本，浏览器无法寻址未编码字形。为此每个 item 带一个
`size`（**画 `text` 的字号**）和一个 `design`（**画选中设计字形的字号**）：

```text
design = size（公式基准尺寸）
size   = design * variant_ink_height / base_ink_height   （取整）
```

PDF 用 `design` + 字形号；SVG/HTML 继续用 `size` 画基础字形，视觉高度与
变体接近（今天的 SVG/HTML 路径不做任何改动）。PDF 在字体缺该变体时也退回 `size`。

### 2.4 `MathItem` 与 canonical

```rust
pub enum MathGlyph { Text, Vertical(u16) }

pub struct MathItem {
    pub text: String,
    pub x: Width,
    pub y: Width,
    pub size: Width,      // 画 text 的字号（回退）
    pub design: Width,    // 画变体字形的字号；Text 时等于 size
    pub glyph: MathGlyph,
    pub stretch: u16,
    pub color: Option<u32>,
}
```

canonical 数学项增加 `"design"` 与 `"glyph"` 两个字段，页面结果升到
`soma.pages/11`；`Gate A` 的 schema 断言同步。`item_ink()` 对
`Vertical(i)` 用变体的 ink，而不是基础字形。

### 2.5 PDF：按字形号绘制

`PdfFont` 增加一个只读的 `MATH → MathVariants` 解析（coverage format 1/2，
只要纵向构造），在 `from_bytes` 时建立 `base gid → [variant gid]`。
`paint_math`：

- `Text` → 今天的路径；
- `Vertical(i)` → `variant_gid(base_cp, i)`，字号 `design`；
- 字体缺变体 → 基础字形 + `size`。

`collect_math` 同步把变体字形加入子集，否则 PDF 里是空白。
`Identity-H` 与 gid 保留的嵌入方式（ADR-0046）让这条路径不需要改编码。

### 2.6 为什么不是「把 glyph id 写进 canonical」

变体字形的 gid 只在**某一版字体文件**里稳定。布局表来自 Noto Sans Math，
用户完全可能用 DejaVu 渲染（`--font-math`）。写 gid 会让「换字体」直接画错字。
序号是字体无关的：两份字体都按尺寸从小到大列出变体，第 i 个变体的语义一致；
PDF 在**实际字体**里解析同一序号。这也让 canonical 保持「只看表与输入」。

---

## 3. 兼容性与确定性

- 内核：只有带默认值的新钩子，没实现钩子的宿主的断行/分页字节不变。
- 页面结果：**所有数学项**多两个字段 → `soma.pages/10 → 11`；语料基线里
  含数学的文档（`math-page`）哈希会变，要在 `docs/corpus-calibration.md` 说明。
- 度量表：`soma.metrics/5 → 6`，内嵌表重新生成；`metricsHash` 变化是预期的。
- 全部仍是整数、无浮点；变体选择只依赖表里的 i32 与固定循环顺序。
- Native 与 wasm 同哈希不变（没有新增浮点或哈希序依赖）。

---

## 4. 验收

1. `metrics-gen`：对 Noto Sans Math 解析出 `∑` 2 个、`√` 6 个纵向变体；
   对没有 MATH 的文本面输出 0；对 CFF 面无 `glyf` 时用 charstring 墨迹。
2. `soma-metrics`：手工构造 v6 blob，断言 delta 编码、排序、`right_gap`
   与旧版兼容（v5 表无变体）。
3. `soma-math`（假 `MathMetrics`）：
   - `∑` 在 display 下选 1.34 em 变体，text 下不选；
   - 目标超过最大变体时选最后一个，不放大；
   - 根号横线起点 = 墨迹右端 − 线宽；根指数底边按百分比抬高；
   - 无变体宿主走旧路径（同样的输入 → 同样的坐标，除本提案明确修正的
     根式接缝/指数位置）。
4. PDF：`math-v2.pdf` 400 dpi 目检 `∑`、`∫`、`∛`、嵌套根号；字形子集包含
   变体 gid（`pdffonts` / 文本抽取不出现 `.notdef`）。
5. `cargo test --workspace`、`bash scripts/gates.sh`、`cargo run -p soma-eval
   -- corpus`；语料只允许 `math-page` 相关差异，逐项解释。

---

## 5. 不做（本提案）

- **GlyphAssembly（装配件）**：三款字体都没有纵向装配件；等有真实字体需求
  再做（D97）。超过最大变体时按 TeX 的做法用最大变体。
- **横向变体**：`\overbrace` / `\underbrace` 仍按 `stretch` 横向缩放；
  横向变体列表不进 v6（D97）。
- **MathKernInfo / 顶部重音锚点**：仍按 `accentBaseHeight` 的近似（D98）。
- 不改 K-P、不断行、不改模板；不引入字体解析到 `soma-core`。

---

## 6. 风险

| 风险 | 对策 |
|---|---|
| 表来自 Noto、渲染用 DejaVu，序号对不上 | 序号语义与字体无关；缺变体退回基础字形 + 回退字号；样张脚本用同一字体生成表与 PDF |
| 变体字形的墨迹尺寸因字体而异，横线接缝可能差一点 | 用表里的 `right_gap` 定位；PDF 只按布局坐标绘制，不重新量字 |
| canonical 升版导致 golden 大面积更新 | 只改数学项字段；非数学页面字节不变；Gate A 同步 |
| 无变体时根式坐标仍会变（修复了接缝与指数） | 这是预期修复；在 ADR 与 corpus 记录中单列 |
