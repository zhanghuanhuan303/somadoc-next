# 提案：效果优先路线（E1–E6）

状态：**提案（待拍板）**。不改已冻结的 `docs/architecture-v2.md`，不动水平 K-P 内核（ADR-0007）；
ADR-0005 里「垂直流原子 = 块」这一条由新 ADR 替代。
若采纳，新增 ADR-0012…0015，并把 `soma.pages` 依次升到 `/2`、`/3`。

一句话：**先在真实字形上建模，再把段落拆到行级、把页面补齐成书页，最后用视觉门禁把「好看」变成可回归的字节。**
排序原则不是「先易后难」，而是**先做会让后面所有 golden 作废的那件事**。

---

## 1. 诊断：当前「效果」的七个硬缺口

按「对最终观感的影响 ÷ 返工风险」排序。证据全部来自本仓库实测，不是推测。

| # | 缺口 | 证据 | 归属 |
|---|---|---|---|
| 1 | **所有宽度都是虚构的** | `SyntheticMetrics` 用「CJK=1em、Latin=0.5em」之类的假值；断点、压缩量、`ascent/descent` 全建立在它上面 | D06 |
| 2 | **段落不能跨页** | 一个段落是一个 `FlowItem`，`height` 是整段高度；超过版心就直接 `overflow` | D49 |
| 3 | **页面上没有书页的东西** | 无页眉、页脚、页码、章节页版式 | D50 / D46 |
| 4 | **标点处理只做了单字裁剪** | `compress_left/right` 逐 atom 固定值，无叠标点成对挤压、无行尾悬挂；且 trim 用根 em，**不随 atom 字号缩放**（20pt 标题里的句号与 10.5pt 正文裁掉一样多） | D08 |
| 5 | **垂直代价的量纲错位** | `page_badness = scale × (free/page_height)³`：40 行的页少排 1 行，代价只有 **1.56**（整数后为 1），几乎免费；页面底部因此可以随意不齐 | D58 |
| 6 | **压缩越界成了常态** | `mixed` 4000 atoms：267 行里 **266 行** `overfull=true`。注意语义：这是「压缩超过声明的 shrink 下限」，不是出边距；根因是长 Latin run 不可断 | D09 |
| 7 | **「好看」无法验证** | 只有一个渲染器，且它画出的位置来自假度量；没有跨渲染器坐标比对，也没有视觉 golden | D28 / D02 |

第 1 条是**一次性代价**：真实度量一上，现有 252 个测试里所有硬编码行数/坐标/字节的断言都要重生一次。
所以它必须第一个做，而且只做一次。

---

## 2. 原则：效果 = 可测指标 + 可看样张 + 可复现字节

三者缺一，质量就只是声称。本提案把「效果」定义成下面这张记分卡，
每个里程碑都必须让其中至少一项**变好且被门禁锁住**。

### 2.1 效果记分卡（Gate Q′ 雏形）

输入：20 段真实中文语料（含标点密集段、中西混排段、长单字段）+ `zh-book-v1` preset。

| 指标 | 定义 | 目标 |
|---|---|---|
| `over_compress_lines` | `overfull=true` 的行数 | 0 |
| `ragged_lines` | 非末行、非单原子行里 `effective ≠ target` 的行数 | 0 |
| `page_free_p95` | 每页底部留白的 P95 | < 1.2 行 |
| `page_free_max` | 最大底部留白 | < 3 行 |
| `widow_violations` | 页首恰为某段最后 1 行 | 0 |
| `orphan_violations` | 页尾恰为某段第 1 行 | 0 |
| `ratio_jitter_p95` | 相邻行 `ratio_milli` 差值的 P95 | < 350 |
| `short_last_lines` | 末行短于阈值 1/4 段的个数 | 报告项，不设门禁 |
| `hyphen_breaks` | 英文断词数 | 0（没有词典就不许断） |
| `page_count` | 总页数 | ≤ 贪心分页的总页数 |
| `bdev_rel_error_p95` | `Line.boundary_deviation` 与精确值的相对误差 | < 0.20（超限告警） |

这些数字全部由 `soma-eval` 输出，**先有测试，再有实现**。

---

## 3. E1：真实字体度量（D06）

### 3.1 目标

把 `Metrics` 从「假度量」换成「字体真值表」，且**跨端字节可复现**：
不解析字体、不做 shaping、不在运行时读文件，只查预先算好的整数表。

### 3.2 设计

- 新 crate `soma-metrics-table`：`no_std`、零依赖、`include_bytes!` 内嵌表，
  实现现有 `Metrics` trait。不需要改内核。
- 表格式 `soma.metrics/1`（自有二进制，小端定长，字段顺序固定）：

```
magic "SMT1" | version u16 | face_count u16 | unit i32(=4096)
per face: family_id u16 | weight u8 | slant u8 | ascent i32 | descent i32 | leading i32
          | space_advance i32 | kern_count u32 | glyph_count u32
per glyph (sorted by codepoint, delta-uvarint):
          codepoint_delta | advance i32 | left_bearing i16 | right_bearing i16 | flags u8
kern pairs: (left_delta, right_delta, amount i16)
```

- 所有长度统一归一化到 **1/4096 em**，从字体 `unitsPerEm` 做整数四舍五入
  （`(v * 4096 + upem/2) / upem`），**不出现浮点**。
- 面（face）解析：`family` → face_id；`weight`/`slant` 走回退链。
  v1 只发行 Serif Regular + Sans Regular 两个面；CJK 的 bold 与 regular
  advance 相同（等宽字形），Latin bold 先用 regular advance，差异记为新债务 D61。
- 左右边距（`left/right_bearing`）是 E3 悬挂标点的数据来源。

### 3.3 生成与许可

- 工具 `tools/metrics-gen`（`publish = false`，std，只在本地跑，**不进任何 no_std crate**）：
  读一个 OTF/TTF，输出 `.smt` blob + 一份可读清单（面、覆盖字数、blob 的 `metricsHash`）+ 字体文件的 SHA-256。
- 字体：**Source Han Serif SC** 与 **Source Han Sans SC**（SIL OFL 1.1）。
  只提取数值度量，不提取轮廓；`PROVENANCE.md` 记录字体名、版本、下载 URL、SHA-256、OFL 全文位置。
- 覆盖范围：通用规范汉字表一二级 + ASCII + CJK 标点 + 常用符号，约 7,000 字/面。
  估算 40–60 KB/面，两合约 100 KB，只随 `soma-metrics-table` 进 native 构建。
- **wasm fixture 用子集表**：只含 fixture 用到的字符（约 1–2 KB），
  native 与 wasm 用**同一个子集 blob**，8,983 bytes 的一致性门禁语义不变。
- blur 与表都进仓库，构建不联网。

### 3.4 接口变化

- `PagedLayout.metrics_version: u16` 保留（表格式版本），**新增 `metrics_hash: u64`**。
- `soma.pages/1 → /2`。canonical JSON 只加一个 `metricsHash` 字段。

### 3.5 TDD 顺序

1. 表格式 round-trip：`blob → table → advance`（不依赖字体文件）。
2. 归一化：`unitsPerEm=1000` 时 advance=500 → `2048`；`upem=2048` 时 1024 → `2048`。
3. 回退：未知码位 → 覆盖字均宽 → 最后 `1em`；**必须确定性**。
4. `metricsHash` 稳定：同一 blob 两次加载同值；改一个字节必变。
5. 全管线：native 与 wasm 同字节（门禁已有）。
6. 视觉样张：demo 的 HTML 用真实字族名，本机装了字体就能直接看。

### 3.6 出口

- 252 → 约 265 测试，all green。
- 所有 golden 用新度量重生一次，并在 `docs/testing.md` 写明「这一版之后不再有假度量 golden」。

---

## 4. E2：行级垂直排版（D49 + D58）

### 4.1 为什么必须改结构

现在的垂直 DP 的「原子」是**块**：一个段落的 `height` 是整段高度。
所以段落只要超过版心就无解，只能 `overflow` 或整段推到下一页（留一大片空白）。
任何 widow/orphan 规则也无处附着。

### 4.2 设计：把垂直流的原子从「块」降到「行」

- `FlowItem` 改为 `VItem`（行级）：

```
VItem {
  block: u16,              // 属于哪个 PlacedBlock
  line: u16,               // 块内第几行（原子块用 0）
  height: Width,
  before: Glue { width, plus, minus },   // D58：可拉伸的段间胶水
  after:  Glue { width, plus, minus },
  boundary: BreakBoundary, // Allowed | Forbidden | Mandatory
  penalty: i64,            // 软惩罚：club / widow / 段间
  note: Option<u16>,       // 该行携带的脚注
}
```

- 段落的每一行产出一个 `VItem`；`before` 只挂在首行，`after` 只挂在末行。
- 标题/图/表/代码等不可拆块仍是**单个** `VItem`（`line=0`，整块高度）。
- 强制分页（`page_break_before`）与现有水平段的做法一致：**先切成垂直段**，
  段内跑 DP。避免在 DP 里处理「必须断」的语义。
- `keep_with_next` 变成标题末行与下一块首行之间的 `BreakBoundary::Forbidden`。

### 4.3 新代价：垂直 badness 按「行」归一化

```
free       = page_height - used - notes
if free < 0: overflow（只在无可避免时允许，收 overflow_penalty）
if free == 0: 0
if stretch > 0 && free <= stretch: r = free/stretch, B = scale_v × r³
else: lines_free_milli = free × 1000 / body_line_height
      B = scale_v × lines_free_milli³ / 1e9
```

这样「少排一行」的代价从 1 变成 100（`scale_v=100`），页面底部才真的会去齐。
`body_line_height` 取该页首块的行高，保证确定性。

### 4.4 widow / orphan

- `club_penalty`：段落第 1 行之后可断处（避免页尾只剩一行）。
- `widow_penalty`：段落最后 1 行之前可断处（避免页首只有一行）。
- 默认两者都取一个「明显贵于一行 badness、但仍有上限」的值（如 `scale_v × 1000`），
  **不是无穷大**——真的排不下时仍要能断。若某段只有 1–2 行，不施加。
- 这些是**模板可继承属性**：`widow-lines: 2`、`orphan-lines: 2`。

### 4.5 脚注按行计费

`Mark` 增加 `line: u16`；页内脚注高度 = 该页所覆盖行区间内 marker 的脚注高度之和。
用前缀和做 O(1)。这比现在「脚注跟着块走」更准：
段落跨页时，脚注落在**标记所在的那一页**。

### 4.6 性能：垂直方向可以精确剪枝

水平方向的 trim 破坏了单调性，所以不能剪；**垂直方向高度是固定的、可加的**，
`used(i..j)` 对 `j` 严格递增。于是「j 一超版心就 break」是**精确剪枝**，不是启发式：

```
while j < n:
    if used + notes > page_height && j > i: break
```

复杂度 O(线数 × 每页最大行数)，与页数线性相关。10,000 行的文档也只有几十万次评估。

### 4.7 输出变化

- `PlacedBlock` 新增 `line_offset: u32` 与 `lines_total: u32`：
  `line_offset > 0` 表示承接上一页；`line_offset + lines.len() < lines_total` 表示续到下一页。
- 同一个 `NodeId` 可以在两页的 `blocks` 里各出现一次，`(node, line_offset)` 唯一。
- `soma.pages/2`（与 E1 同一个 schema 版本，一次升版）。

### 4.8 不变量（先写测试）

1. **行守恒**：所有页面里同一块的 `line_offset` 区间恰好覆盖 `0..lines_total`，无重叠无丢失。
2. **顺序守恒**：按页序拼回去，行的 `y` 单调递增。
3. **单页等价**：把版心设得足够高时，行级 DP 的输出与当前块级 DP 逐字节一致。
4. **widow/orphan**：fixtures 里违规数为 0。
5. **强制分页**：`page_break_before` 之前的页不出现该块任何一行。

### 4.9 出口

- 记分卡 `page_free_p95`、`widow_violations`、`orphan_violations` 首次可测。
- 新增 ADR-0012「行级垂直排版」。

---

## 5. E3：标点与字距的精致化（D08）

中文排版的「像中文」几乎全在标点上。现在只做了「行首行尾单字裁剪」。

### 5.1 三项改动

1. **trim 随 atom 字号缩放**：`compress_left/right` 从「根 em 的绝对值」改为
   「以 atom 自身 `font_size` 为基准的比例」，在 rules 层解析成绝对值。
   20pt 标题里的句号裁剪量应为正文的 20/10.5 倍。
2. **叠标点成对挤压**：新增 `pair_trim_table`，形如 `（（`、`」」`、`。，`、`：；`
   这类连续标点的组合，第二个标点额外压缩。数据来自 clreq 与 GB/T 15834，
   每条带 `ruleId` 与 `RuleStatus`；未核实的标 `Tbd`，只产生软规则。
3. **行尾悬挂标点（opt-in）**：允许行末的 `Close`/`Middle` 类标点
   **悬挂进右边距**，上限 = 度量的 `right_bearing`（E1 提供）。
   实现上就是允许**负 trim**，并且渲染时该 atom 的 x 可超出内容盒、不裁切。

### 5.2 参数化

| 参数 | 层 | 默认 | 作用 |
|---|---|---|---|
| `trim-scale` | Table | 1.0 | 全局 trim 比例 |
| `pair-trim` | RuleSet | 0.5 | 叠标点额外压缩占单字 trim 的比例 |
| `hanging-punctuation` | Template | `false`（`zh-book-v1` 为 `true`） | 行尾悬挂 |
| `hanging-cap` | Template | `0.5em` | 悬挂上限 |
| `tolerance` | Engine | 3000（`zh-book-v1` 建议 1500） | 正常阶段允许的 `r` |

`tolerance` 与 `badness_scale` 耦合，E3 用一个二维 sweep 出表，
把「压缩越界」压到接近 0，**不靠手调**。

### 5.3 测试

- `（（`、`」」` 的组合宽度 < 两个单字宽度。
- 20pt 标题与 10.5pt 正文的 trim 比 = 字号比。
- 悬挂：行末标点的 `x + advance > target`，但不超出版心 + `hanging-cap`；
  且 `adjustment` 仍精确等于 `target - natural`。
- 关掉 `hanging-punctuation` 时输出与 E2 逐字节一致（opt-in 不能污染默认路径）。

### 5.4 出口

- 新增 `zh-book-v1` preset（模板级），集中所有「书版」默认值。
- `over_compress_lines` 在语料上降到个位数。

---

## 6. E4：页面母版、页眉页脚、页码（D50 + D46）

### 6.1 模板

```yaml
page-master:
  size: a5          # 已有
  margins: 20mm 18mm 22mm 18mm
  header:           # 新增
    height: 14mm
    align: center
    style: header
    content: running-title
  footer:
    height: 16mm
    style: footer
    content: folio
  folio:
    format: "第 {page} 页 / 共 {pages} 页"
    start: 1
  first-page: cover  # 首页用不同母版
```

- `running-title`：取该页之前最近的标题文本；没有则空。
- `folio`：页码在**分页完成后**回填，需要第二遍只填文本、不改断点。
  `{pages}` 的总页数也来自第一遍结果；因为页眉页脚不参与版心高度，回填不会改变分页
  （测试锁死这条不变式）。

### 6.2 布局与渲染

- `Page` 新增 `header: Vec<PlacedBlock>`、`footer: Vec<PlacedBlock>`、`folio_text`。
- 渲染器只画，不计算：页眉页脚也是绝对定位的块。
- `soma.pages/2 → /3`。

### 6.3 测试

- 首页无页眉页脚（`first-page: cover`）。
- 页码连续、`{pages}` 与 `page_count()` 一致。
- 回填前后 `Page.blocks` 坐标逐字节一致（不变式）。
- 关闭母版时回到 `/2` 的等价输出（字段为空）。

### 6.4 出口

- 新增 ADR-0013「页面母版」。

---

## 7. E5：基线网格（opt-in，新债务 D60）

书版中文的「齐」不只是行末齐，还有**基线对齐**：相邻页的正文行落在同一条网格上。

- 模板属性：`baseline-grid: 12pt`（或 `auto` = 正文字高）。
- 实现：在生成 `VItem` 时把行高与段间距**量化到网格整数倍**：
  `h' = ceil(h / grid) × grid`。这样所有基线天然落在 `n × grid` 上，
  DP 仍按整数工作，不需要事后平移。
- 版心高度向下取整到网格整数倍。
- 大字号行（行内 2em）按 `ceil` 占两格或三格，不重叠。
- 默认关闭；`zh-book-v1` 里按模板决定。

测试：所有正文行 `y % grid == 0`；开启后页数变化 ≤ 1；关闭时与 E4 逐字节一致。

---

## 8. E6：混排兜底 + 视觉门禁（D09 + D28）

### 8.1 不引入词典的断行兜底

- 允许在 `-`、`/`、`·`、`\u{200B}` 处断（规则层不再对这些位置发 Forbidden）。
- 长 run 仍不可断时，允许**字间微调（tracking）**：在 emergency 阶段
  对 Latin run 施加 ±`tracking-limit`（默认 1/32 em）的字符间距，
  优先于「压缩越过 shrink 下限」。
- 仍然**不做**断词：`hyphen_breaks` 门禁保持 0。

### 8.2 视觉门禁（Gate V / Gate R）

- 新 crate `soma-render-svg`：把 `PagedLayout` 画成 SVG（`<rect>` 页面 + `<text>` 行 + 每 atom 的 x/y）。
  标注为**调试/视觉样张**，不承诺打印保真（N2 仍然不做 PDF）。
- Gate V：3 个 fixture 的 SVG 字节 golden。
- Gate R：同一 `PagedLayout` 经 HTML 与 SVG 两个渲染器，**逐 atom 坐标相同**（容差 0）。
  这正好补上 D28 缺的「第二个渲染器」。
- demo 页面同时输出 HTML 与 SVG，人眼可看。

### 8.3 出口

- 效果第一次可以「看」也可以「回归」。

---

## 9. schema / ADR / 门禁变更

| 里程碑 | schema | 新 ADR | 门禁 |
|---|---|---|---|
| E1 | `soma.pages/2`（+`metricsHash`） | 0012 度量表 | Gate A 增 `soma-metrics-table` 的 no_std/零依赖检查；表 hash 稳定 |
| E2 | `soma.pages/2`（+`line_offset`/`lines_total`；胶水） | 0013 行级垂直排版（替代 ADR-0005 的垂直部分） | 行守恒 / 单页等价 / widow·orphan |
| E3 | 不变 | 0014 标点与悬挂 | 叠标点、字号缩放、opt-in 不污染默认 |
| E4 | `soma.pages/3`（+页眉页脚/folio） | 0015 页面母版 | 回填不改分页、首页例外 |
| E5 | 不变（字段在 `/3` 内） | — | 网格对齐 |
| E6 | 不变 | — | Gate V（SVG golden）+ Gate R（跨渲染器坐标） |

**一次升版原则**：E1 与 E2 合并到 `/2`，避免两轮 golden 重生。
E4 才升 `/3`。

---

## 10. 顺序、可停点、工作量

| 里程碑 | 内容 | 依赖 | 可停点 | 估时 |
|---|---|---|---|---|
| E1 | 真实度量 + `/2` | 无 | ✅ 全绿可停 | 3 d |
| E2 | 行级垂直 DP + 胶水 + widow/orphan | E1 | ✅ | 4 d |
| E3 | 标点精致化 + `zh-book-v1` | E1、E2 | ✅ | 3 d |
| E4 | 母版/页眉页脚/页码 | E2 | ✅ | 3 d |
| E5 | 基线网格 | E4 | ✅ | 1.5 d |
| E6 | 混排兜底 + SVG/Gate V/R | E3 | ✅ | 2.5 d |
| Q′ | 记分卡 + 语料 + 对照报告 | E1–E6 | — | 1.5 d |

合计 **约 18–19 个工作日**。每个里程碑独立全绿，可随时停。

---

## 11. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| 真实度量让 252 个测试大面积失效 | 一天内无法判断「改坏了」还是「改对了」 | E1 第一件事是**把断言分成「不变量」与「快照」两类**；快照重生，不变量保留 |
| 字体许可 | 合规风险 | OFL + 只提取数值 + PROVENANCE 记录 URL/SHA-256/许可原文 |
| 表太大 | wasm 体积 | native 全表、wasm 子集表；两者 hash 各自记录 |
| 行级 DP 变慢 | 长文档墙钟 | 垂直方向是**精确**窗口剪枝（§4.6），不是启发式；超过阈值回退块级 |
| 悬挂标点改变默认输出 | golden 漂移 | opt-in，默认关；关掉必须逐字节等于 E2 |
| 母版回填引起二次分页 | 不可复现 | 页眉页脚不占版心；回填前后 `blocks` 字节不变写进测试 |
| 基线网格与图/表冲突 | 重叠 | 大块按 `ceil` 占整格；默认关；只在 `zh-book-v1` 开 |
| 长 Latin run 仍不可断 | overfull 残留 | tracking 兜底 + 记分卡诚实报告，不谎称 0 |

---

## 12. 明确不做

- 不做断词/连字词典（`hyphen_breaks` 门禁保持 0）。
- 不做 PDF、不做字体嵌入、不做多栏、不做竖排、不做 bidi。
- 不做真首字下沉、不做浮动体绕排、不做整书级优化。
- 不做增量重排（N4），不改水平内核的代价模型（ADR-0007 仍冻结；
  `tolerance` 属参数层，不是模型层）。
- 不引入浮点；所有新量仍是整数。

---

## 13. 一句话总结

**E1 把尺子换成真的，E2 让段落能在页间断开，E3 让标点像中文，E4 让它像一页书，
E5 让两面书页对得齐，E6 让「好看」可以被回归。**
其中 E1 是不可推迟的一次性代价；E2 是当前唯一的**硬缺陷**；其余是质量上限。
