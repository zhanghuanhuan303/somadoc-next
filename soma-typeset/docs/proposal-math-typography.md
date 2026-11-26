# 提案：数学排版对标 LaTeX（Math v2）

- 状态：**已拍板并实现**（M1–M7），决策记录见 ADR-0070
- 日期：2026-11-21
- 范围：`soma-math`、`soma-layout` 的数学路径、`soma-metrics`（新增 MATH 段）、
  `tools/metrics-gen`、默认模板的数学跳距。**不动行内 K-P 的 DP**。
- 相关：ADR-0026（显示数学）、ADR-0027（行内数学）、ADR-0030（数学补全）、
  ADR-0054/0056（斜体变量、`\tag`）、D71；本提案通过后新增 **ADR-0070**。

---

## 0. 一句话

现在的问题不是「缺几个符号」，而是三件事叠在一起：**间距表只实现了 Bin/Rel
两类**、**纵向几何用整张字面的 ascent/descent 而不是字形墨迹**、**style 只有
字号缩放，没有 TeX 的 cramped 与收敛**。于是公式能排出来，但节奏和比例不像
LaTeX。本提案按 TeXbook 附录 G 的口径重建数学排版，并把所有几何常数参数化。

---

## 1. 现状：丑在哪里（有代码或样例坐标）

以下每条都能在 `examples/book/latex-default.pdf` 第 4 页复现，或在代码里定位。

### 1.1 间距只做了两类原子

`Class` 只有 `Ord/Bin/Rel/Open/Close/Punct`（`crates/soma-math/src/lib.rs:284`），
`gap()` 只对 Bin、Rel 加空隙，其余一律 0（`:1960`）：

- `\sin x`：`\sin` 被解析成 `Class::Ord` 的普通符号（`:610`），Ord–Ord = 0，
  于是排成 **`sinx`**；TeX 的 `\sin` 是 Op，Op–Ord 有 3mu 细空。
- `\sum a`：`Node::Big::class()` 返回 `Ord`（`:407`），大算符后没有细空，
  `∑a` 挤在一起。
- `a,b`：逗号是 Punct，Punct–Ord 也是 0，逗号后没有细空。
- `x = -1`：Rel–Bin 被当成「右边是 Bin」加 4mu；TeX 先把 Rel 后的 Bin
  归一化成 Ord，`=` 后只有一个关系空。
- **`\!` 是空操作**：解析成 `Space(0)`（`:618`），而 `docs/math.md` 写的是
  −167‰；`Space(u16)` 本身也存不了负值。

### 1.2 纵向几何用整张字面的 em 盒，不是字形墨迹

`MathMetrics` 只有 `advance/ascent(size)/descent(size)`（`:32`），
`measure_impl` 对每个 item 都取 `metrics.ascent(size)` / `descent(size)`
（`:1556`），最终盒高也用同一组值（`:1179`）。后果：

- **根式的横线浮在半空**：`\sqrt{\pi}` 的 vinculum 按「字面 ascent + 线宽」
  定位（`:1306`），π 的实际墨迹只有 ~0.44em，横线被抬到 1.1em。样例里
  `\frac{\sqrt{\pi}}{2}` 的根号明显「长腿」。
- **`\left( … \right)` 偏大**：`fence_geometry` 用字面 ascent+descent 判断
  需要多大（`:1600`），括弧按 em 盒而非内容墨迹选尺寸。
- **分数上下留白不对**：分子按 `gap + 字面 descent` 抬（`:1276`），分母按
  `gap + 字面 ascent` 压（`:1277`）。对没有降部的 π、h 这类分子是白抬，
  对浅的 `2`、`+` 这类分母是白压。

### 1.3 常数是单标量，没有 TeX 的 num/denom/sup/sub 规则

`MathStyle` 只有 `frac_gap/sup_shift/sub_shift/big_scale` 一组数
（`:119`）。TeX 的分数要 num1/num2/num3 与 denom1/denom2（显示/文本/脚本
各一套，且保证最小净空），上下标要 sup drop / sub drop / sup1..sup3 /
sub1..sub2 与「上下标同时存在时的最小间隙」。现在：

- 分子分母只按一个 `frac_gap` 退让，嵌套时靠字面值兜底；
- 上下标固定 0.4em / 0.2em，基线有深降部时下标会碰字；
- 大算符的上下限用 `frac_gap` 当间距，`\sum` 的 `n`、`i=1` 贴着算子。

### 1.4 style 只有字号，没有收敛和 cramped

`script_size()` 永远乘 `script_scale`（`:1973`），`scriptscript_scale`
**从未被读取**。`x^{y^{z^w}}` 会一级级缩到 0.7⁴；TeX 到 scriptscript
（10pt 下 5pt）就停。分母没有 cramped 变体，`\frac` 里再放上标时，
上标离分数线过近。`\displaystyle`/`\textstyle` 也不存在。

### 1.5 断行与多行

- 显示公式是一个不可拆的盒子（`crates/soma-layout/src/lib.rs:3583`），
  这本身与 LaTeX 的 `\[...\]` 一致（TeX 也不自动断显示公式），但
  `multline`/`split` 的行距只有 `frac_gap` 的量级，没有 `\jot`；
- 行内公式是一个 object atom，TeX 允许在 Rel/Bin 处断行（`\relpenalty=500`、
  `\binoppenalty=700`），我们现在一律不可断；
- 公式前后的垂直跳距是模板里的固定 0.8em，没有 TeX 的
  `\abovedisplayskip`（10pt ± …）与 short-skip 规则。

### 1.6 语法缺口（影响"像不像 LaTeX"的常用项）

`\binom`、`\overline`/`\underline`、`\hat/\bar/\vec/\tilde/\widehat`、
`\operatorname`、`\displaystyle/\textstyle`、`\limits/\nolimits`、
`\mathop/\mathbin/\mathrel/\mathopen/\mathclose/\mathpunct/\mathinner`、
`\stackrel`、`\genfrac`、`\cfrac`、`\:` 全部缺失。

### 1.7 字体

样例正文是思源宋体，公式用 NotoSansMath（无衬线），风格不匹配。
"对标 LaTeX"首先是 **Latin Modern Math / STIX Two Math / Noto Serif Math**
这类衬线数学面，其次是让测量和绘制用同一张表（现在已经是）。

---

## 2. 目标

以 TeXbook 附录 G 为验收口径，做到：

1. **间距**：8 类原子 + 8×8 间距表（thin 3mu / med 4mu / thick 5mu），
   Bin 归一化，`\nonscript`（S/SS 下没有 med/thick 空）。
2. **纵向**：一切净空按**字形墨迹**计算；字形墨迹、斜体修正、定界符变体
   从字体读，缺失时回退到 TeX 常数。
3. **style**：D/T/S/SS 四级 + cramped；进入 S 之后不再按比例无限缩小。
4. **几何**：分数、根式、上下标、大算符、定界符、重音的公式取 TeX 的规则
   与字体 MATH 表常数。
5. **可复现**：全部整数；`metricsHash` 覆盖数学常数；native/wasm 同哈希；
   新增 Gate M（数学坐标与不变量）。
6. **可参数化**：字体常数进度量表，跳距进模板，断行惩罚进引擎参数；
   `soma-math` 里不再出现魔数默认值。

**验收基线**：同一组 fixture 用 `lualatex + unicode-math + 同一数学字体`
排一遍，令牌级 x/基线差 ≤ 0.05em 的比率 ≥ 95%，其余 ≤ 0.1em；
外加人工目视 A/B 页（算子、分数、根式、上下标、括号各一页）。

---

## 3. 设计

### 3.1 度量缝：`soma.metrics/5` 增加 `math:` 段

现状：`.smt` 已到 v4，只有字形**水平**墨迹（`ink_left/right`，1 字节量化），
`MathMetrics` 拿不到字形纵向墨迹与斜体修正。

新增（可选段，旧表照常读）：

```text
u32 math_face_index                       (0xFFFFFFFF = 无数学面)
MathConstants: i32 数组（MATH v1.0 56 项，v1.1 57 项）
u32 glyph_count
per glyph: varint(codepoint delta) | i16 top | i16 bottom | i16 italic_correction
u32 variant_count
per variant: varint(base) | u8 axis (0=x,1=y) | u16 count
             per size: varint(codepoint) | i32 advance | i16 height | i16 depth
```

- `MathMetrics` 增加 `ink(text, size) -> (above, below)` 与
  `italic_correction(text, size)`，默认回退到字面 `ascent/descent` 与 0。
- `metrics-gen` 从 OTF/TTF 的 `MATH` 表读常数、`MathGlyphInfo`
  （斜体修正、顶部重音锚点）与 `MathVariants`（定界符的大号变体）；从
  `glyf`/CFF 的 bbox 读每个字形的纵向墨迹。没有 `MATH` 表时写
  `tex_fallback()` 的一组常数（Latin Modern 的近似值），golden 锁死。
- 每字形 `i16`（1/4096 em）比水平墨迹的 1 字节更精确：分数净空、根式横线
  都是 1/10 em 量级，1 字节（1/256 em）不够。

### 3.2 原子类与间距表

`Class` 扩到 8 类：`Ord Op Bin Rel Open Close Punct Inner`。间距表按
TeXbook 第 18 章（1 = 3mu，2 = 4mu，3 = 5mu，`*` = 不可能，按 Ord 处理）：

```text
        Ord Op  Bin Rel Open Close Punct Inner
Ord      0   1   2   3   0    0     0     1
Op       1   1   *   3   0    0     0     1
Bin      2   *   *   *   2    *     *     *
Rel      3   3   *   0   3    0     0     3
Open     0   0   *   0   0    0     0     0
Close    0   1   2   3   0    0     0     1
Punct    1   1   *   1   1    1     1     1
Inner    1   1   2   3   1    0     1     1
```

- mu 用千分比存：thin 167、med 222、thick 278（与现有 `Space(167/278)` 一致）。
- **Bin 归一化**：列表首尾、以及 Bin/Op/Rel/Open/Punct 之后的 Bin 视为 Ord
  （TeX 规则 5/6）。`x = -1`、`(+a)` 由此得到正确间距。
- **`\nonscript`**：S/SS 下 med/thick 归零。
- 类归属：`\sin \log \lim \max` 等 = `Op`；`\frac`、`\binom`、`\left…\right`
  = `Inner`；`\sqrt`、重音、`\text` = `Ord`；`\mathop` 等 8 个命令可显式改类。
- 公式内部的 `Space` 改为有符号（`\:` = 222、`\!` = −167），修掉 1.1 的
  空操作。

### 3.3 style 与 cramped

```rust
enum MathStyleLevel { Display, Text, Script, ScriptScript }
struct MathCtx { level, cramped: bool, size }
```

- 尺寸：D/T = 1.0，S = script_scale（0.7），SS = scriptscript_scale（0.5）；
  进入脚本后最多降到 SS（T→S→SS，SS 再加也不缩小），修 1.4 的无限缩小。
- `\displaystyle/\textstyle/\scriptstyle/\scriptscriptstyle` 直接切级。
- 上标后的下标、分母进 cramped；cramped 使上标的 `sup_drop` 规则改变。
- 上下标（TeX 规则 18）：取 `sup_shift`、`sub_shift` 为下界，再用
  `superscriptBaselineDropMax`/`subscriptBaselineDropMin` 与上下标墨迹决定
  实际位置；同时存在时保证 `sub_top ≤ sup_bottom − subSuperscriptGapMin`。
- **斜体修正**：`x^2` 的上标从 `x` 的 advance 右侧再退 `italic_correction`
  的位置起排（MATH 表；没有时 0）。这也是"LaTeX 味"最明显的一处。

### 3.4 盒子几何

- **分数**：分子用 num1（D）/ num2（T）/ num3（S）抬升，分母用 denom1/denom2
  下沉，净空以**墨迹** depth/height 计算，并保证 `rule_thickness + num3` 的
  最小净空；分母用 cramped。分数是 Inner。
- **根式**：vinculum 贴在 radicand 的**墨迹顶**再加一个线宽；根号字形先按
  `MathVariants` 取最小的够高变体，没有变体才连续缩放；指数用
  `radicalDegreeBottomRaisePercent` 与 `radicalKernBefore/AfterDegree`。
- **大算符**：显示/文本各取字体的 size variant；上下限用
  `upperLimitGapMin`/`upperLimitBaselineRiseMin`/`lowerLimitGapMin`/
  `lowerLimitBaselineDropMin`；`\int` 除非写 `\limits`，显示样式也把上下限
  放右侧（现行为正确，保留）。
- **定界符**：按内容墨迹高度选 `MathVariants` 的变体；超高时才用装配件
  （extender + top/bottom hook），与 `\overbrace` 的装配思路一致；没有变体
  数据时回退到连续缩放。
- **重音**：`\hat/\bar/\vec/\tilde/\widehat` 用 `MathTopAccentAttachment`
  定水平位置（宽体用装配横线）；`\overline/\underline` 用 rule。
- **根式/括号的最小高度**：TeX 的 `\delimiterfactor`/`\delimitershortfall`
  语义写入参数，默认 901/5pt 等价。

### 3.5 显示公式与多行

- 显示公式仍然不自动断（与 `\[...\]` 一致）；`multline`/`split`/`align` 的
  行距改用 `\jot`（3pt）+ 每行的 ascent/descent，不再借用 `frac_gap`。
- 公式前后的跳距进模板：`mathBlock` 的 `space-before/after` 加上
  `-plus/-minus`（模板已有这三个属性，D58），默认值取 10pt：
  above/below = 10pt plus 2pt minus 5pt；**short skip** 由布局按「前一行是否
  短」选用（0pt plus 3pt / 7pt plus 3pt minus 4pt）。
- 编号：单行公式的编号按 TeX `\eqno` 的垂直位置放（现在一律在基线）；
  多行按各自行基线（现行为正确）。用参照页锁定，不猜。

### 3.6 行内公式断行（可选阶段）

行内公式目前是一个 object atom。TeX 允许在 Rel/Bin 处断开并记
`\relpenalty=500`/`\binoppenalty=700`。做法：在**布局层**把长公式编译成
若干 object atom，段间边界带惩罚，间距作为 glue 计入；内核 DP 不变
（仍然只认 atom 与 boundary）。公式很短时（默认 ≤ 8 个原子）保持一个 atom，
避免让普通段落多出断点。此阶段会改 `soma.pages` schema（`inline_math`
从 1 个 atom 变成一串），单独排期。

---

## 4. 参数化清单

| 层 | 内容 | 存放 | 进哪个哈希 |
|---|---|---|---|
| 字体常数 | axis height、rule thickness、script %、num/denom、sup/sub、limit gap、radical、accent | `.smt` v5 的 `math:` 段 | `metricsHash` |
| 字形数据 | 纵向墨迹、斜体修正、变体字 | `.smt` v5 | `metricsHash` |
| 间距 | thin/med/thick（固定 + stretch/shrink，默认不开） | `soma-core` 的 `MathParams`（preset） | `paramHash` |
| 跳距 | above/below display、short skip、`\jot` | 模板 `mathBlock`/`mathRow` 样式 | `templateHash` |
| 断行 | `\relpenalty`、`\binoppenalty`、行内公式断行阈值 | `soma.engine/1` 新段 `math:`（带默认值，默认行为不变） | `engineHash` |
| 冻结 | DP 递推、坐标约定、`MathItem`/`MathRule` 形状 | 代码 | `modelVersion` |

原则：**`soma-math` 只吃一个 `MathParams` 值，不读字体、不读模板、不含默认
常数**。`MathParams::tex_fallback()` 只是给测试和旧表用的完整一组值。

---

## 5. 语法补齐（按使用频率）

1. `\binom{n}{k}`、`\genfrac`、`\cfrac`（连分数）；
2. `\overline`、`\underline`、`\hat`、`\bar`、`\vec`、`\tilde`、`\widehat`；
3. `\operatorname{…}`、`\operatorname*{…}`（后者显示样式堆上下限）；
4. `\displaystyle`、`\textstyle`、`\scriptstyle`、`\scriptscriptstyle`；
5. `\limits`、`\nolimits`；
6. `\mathop` 等 8 个类命令；
7. `\stackrel`、`\substack`（已有）、`\middle`；
8. `\:`（med space）、修 `\!`、`\negthinspace`、`\thinspace`。

---

## 6. 验证与门禁（新增 Gate M）

1. **单元 golden**：间距表 8×8、mu 换算、Bin 归一化、`\nonscript`、
   style 收敛、分数/上下标的净空不等式。每个常数一个断言，失败时打印
   `(实际, 期望, 来源)`。
2. **参照页**：`scripts/math-ab.sh` 用 `lualatex + unicode-math` 排
   `examples/math/fixture.tex`（同一数学字体），用 `pdftotext -bbox` 取
   令牌坐标；与我们的 canonical JSON 对比。报告逐项 Δx/Δy，门禁为
   「≥95% 的令牌 |Δx| ≤ 0.05em」；首版先只报告，稳定后进门。
3. **不变量**（不看参照页也能抓丑）：
   - 同一行相邻 item 的墨迹盒不重叠（重音、斜体修正除外，白名单）；
   - 分数：分子底 ≥ 线 + 最小净空，分母顶 ≥ 线 + 最小净空；
   - 根式：vinculum 在 radicand 墨迹顶上方一个线宽内；
   - 定界符墨迹覆盖 body 墨迹；上下标不相交。
4. **确定性**：`scripts/wasm-hash.sh` 覆盖一个含数学的 fixture；
   `scripts/gates.sh` 增加 Gate M。
5. **视觉 A/B**：`examples/book` 第 4 页前后对比；`docs/proposal-math-typography.md`
   附一张对照图（分数、根式、`\sin x`、`\sum`、`\left(\right)` 五组）。

---

## 7. 阶段计划

| 阶段 | 内容 | 门禁 | 影响面 |
|---|---|---|---|
| **M1** | `.smt` v5 `math:` 段；`MathMetrics::ink/italic_correction`；metrics-gen 读 MATH 表与字形 bbox；旧表回退 | metrics 往返、回退等价、`metricsHash` 变化 | 度量表、`soma-metrics`、`soma-layout` 适配层 |
| **M2** | 8 类原子 + 间距表 + Bin 归一化 + `\nonscript` + `\sin` 归 Op + `\:`/`\!` | 间距表 golden、样例 `sin x`/`a,b`/`x=-1` | `soma-math` 解析与布局；页面坐标变化 |
| **M3** | D/T/S/SS + cramped + 上下标规则 + 斜体修正 + style 命令 | style 收敛/间隙 golden、`x^{y^{z}}` | 所有含上下标的公式 |
| **M4** | 分数/根式/大算符/定界符/重音的墨迹几何与字体常数；`\binom`/重音/`\overline` | 净空不等式、A/B 页、变体选择 | 数学盒子高度/深度普遍变化，分页随之变 |
| **M5** | 多行行距（`\jot`）、显示跳距与 short skip、编号垂直位置 | 多行 fixture、`\eqno` 参照 | 模板新增 `mathRow` 样式；分页基线 |
| **M6**（可选） | 行内公式在 Rel/Bin 断行；`\relpenalty`/`\binoppenalty` | 断行 golden、`K-P ≤ Greedy-C` 不回归 | `soma.pages` schema +1 |
| **M7** | 数学字体策略：默认衬线数学面、`scripts/fonts.sh` 匹配正文轴、样例重排 | 视觉 A/B、字体文档 | 示例脚本与文档 |

前 4 阶段完成即可说「对标 LaTeX 的数学排版」；M5 补长文档质感，M6 是长行内
公式的健壮性，M7 是观感统一。

---

## 8. 影响面与迁移

- `.smt` 升 v5（新增可选段）；v1–v4 照常解析，没有 `math:` 段时用
  `tex_fallback()`。`metricsVersion` 升到 5，`metricsHash` 改变。
- 页面 canonical：数学盒子的 height/depth 从 em 盒改成墨迹盒，**所有含公式
  的页面**行高、分页、页码都会变；语料基线需要一次性重排并记录原因
  （与 ADR-0069 同样的做法：先说明，再更新数字）。
- `soma.pages/10` 在 M1–M5 不变；M6 才升 schema。
- 默认模板：`mathBlock` 的跳距按 3.5 调整；数学字体默认值在 M7 改
  `scripts/fonts.sh`，不改 CLI 的 `--font-math` 语义。
- 契约不变：`soma-math` 仍然不读字体、不读模板；`soma-render-*` 仍然只画
  `MathBox` 的 item/rule。

---

## 9. 非目标

- 不做宏展开（那是 `soma-latex` 的职责）；不做 `\halign`、`\mathchoice`
  的全分支展开；不做 `\intertext`/`alignat`（D71 仍开）。
- 不做行内公式的 TeX 级 `\mathsurround` 与 `\everymath`。
- 不引入第二套数学字体混排（M7 只选一套数学面；`\text` 仍用正文字体）。
- 不改 K-P 内核的 DP、代价模型与 tie-break。

---

## 10. 备选与风险

| 方案 | 取舍 |
|---|---|
| 用 `fontdue`/`rustybuzz` 之类现成 shaping | 违背零依赖与 clean-room；数学不是 shaping 问题，是 box 规则问题 |
| 只调常数、不接 MATH 表 | 实现快，但换数学字体就会「不像」；且无法拿字体变体 |
| 用 `f32` 中间量 | 破坏跨端哈希；全部用 i64 + 1/4096 网格，除法只在边界做一次 |
| 行内公式一直不可断 | 与 LaTeX 有差距，但中文正文里长行内公式少；放 M6，失败也不阻塞 |
| 直接照搬某开源实现的几何 | 不采用；只按 OpenType MATH 规范 + TeXbook 规则实现 |

**最大风险**是 M4 的「墨迹化」会让所有数学页重排，A/B 参照页也会暴露一批
既有 bug（可能包括行内公式的行高）。对策：M1 先把度量接好并用
`tex_fallback` 锁行为，M4 单独一个 commit，语料基线只在这一步动一次。

---

## 11. 需要拍板的三件事

1. **验收口径**：是否接受「lualatex + unicode-math 同字体、令牌 Δx ≤ 0.05em、
   ≥95%」作为 Gate M 的主门禁（首版只报告，M4 后转门禁）。
2. **度量表升级**：`.smt` v5 增加 `math:` 段（MATH 常数 + 字形纵向墨迹 +
   斜体修正 + 变体），由 `tools/metrics-gen` 生成；没有 MATH 表时回退
   `tex_fallback()`。
3. **阶段边界**：M1–M4 作为「数学 v2」一次做完；M5 紧随；M6（行内断行）
   要不要进这一轮，还是等长行内公式的真实语料出现再做。
