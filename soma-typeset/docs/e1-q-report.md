# E1–Q′ 交付报告（效果优先路线）

状态：**完成**。`bash scripts/gates.sh` 全绿。
对应提案：`docs/proposal-effect-max.md`；决策：ADR-0012…0015。

## 1. 结果一览

| 项 | 结果 |
|---|---|
| 测试 | **299 passed / 0 failed** |
| 门禁 | fmt、clippy `-D warnings`、tests、wasm32 check、**native vs wasm32-wasip1 字节一致（9,174 bytes）**、demo smoke、依赖边界、Gate A、Gate C、Gate V/R、Gate Q′ |
| schema | `soma.metrics/1`、`soma.template/2`、`soma.styles/2`、`soma.pages/3`（`soma.doc/1`、`soma.layout/6` 未变） |
| 基准 | `scripts/kp-verify.sh` 两项 PASS；4,000 atoms 纯中文 ≈1.0 ms，标点密集 ≈1.1 ms |
| 记分卡 | 见 §6 |

> 后续 ADR-0018/0019 又把模板升到 `soma.template/3`、页面升到 `soma.pages/4`；
> 本表记录 E1–Q′ 交付时的状态。

## 2. 每个里程碑做了什么

### E1 真实字体度量

- 新 crate `soma-metrics`：零依赖、`no_std`，解析提交进仓库的 `soma.metrics/1` 二进制表；
  查表全整数、跨端一致。表 63,212 bytes，四面：Noto Serif/Sans CJK SC 的 Regular 与 Bold（OFL）。
- 新工具 `tools/metrics-gen`（std、`publish = false`）：读 OTF/TTC 的
  `head`/`hhea`/`OS/2`/`hmtx`/`maxp`/`cmap`，只提取数值，不提取轮廓。
- `soma-layout` 新增 `TableMetrics`（单面单号）与 `StyledMetrics`（`StyleId` → 字面 + 字号），
  标题按 2em 真实放大；`sans`/`mono`/`hei`/`gothic` 选无衬线面。
- `Metrics` trait 增加带默认值的 `version()` / `hash()`；`PagedLayout.metricsHash` 进入
  canonical，换表即换字节。

### E2 行级垂直排版

- 垂直流原子从「块」降为「行」；段落可跨页，`PlacedBlock.lineOffset` / `linesTotal`
  记录切片，`(node, lineOffset)` 唯一。
- 页高代价按**行高**归一化：少排一行 = 1000‰，不再是页高的 2.5‰。
- `clubPenalty` / `widowPenalty` 有限值；模板可用 `widow-lines` / `orphan-lines` 表达意图。
- 垂直胶水 `space-before/after-plus/minus`：既参与 badness，也**真实分配**到块坐标。
- 脚注按**标记行**计费，跨页段落的脚注落在正确页面。
- 高度可加且单调，因此「超页即停」是精确剪枝；长文档不退化。

### E3 标点与字距

- trim 上限按 atom 自身字号缩放（20pt 标题与 10.5pt 正文同比例）。
- 叠标点成对压缩：`trim.pair`（1/4em，`ParamOnly`，引 clreq §6.3.2.2）。
- 行尾悬挂标点：`hanging-punctuation` + `hanging-cap`；只调整行尾盒与 `adjustment`，
  **不改断点**。默认关闭，`false` 时与 E2 逐字节一致。

### E4 页面母版

- 模板 `page.header` / `page.footer` / `page.header-text` / `page.folio`。
- 页眉页脚住在页边距内，**不占版心**：开启母版不改变 `Page.blocks`。
- running title = 该页首项之前最近的标题；`{page}` / `{pages}` 在分页后回填，
  因不占版心所以不会引起二次分页。
- 页眉页脚是普通 `PlacedBlock`，渲染器用同一套绝对定位绘制。

### E5 基线网格

- 模板 `baseline-grid: <length>`；行高与块间距向上取整到网格整数倍，所有正文行
  `y % grid == 0`。默认关闭。

### E6 混排兜底与视觉门禁

- Latin run 在 `-` 后切开；`/` 前禁止断行、后允许断行；`·` 与 U+200B 视为断点。
- 新 crate `soma-render-svg`：每页一个 `<svg>`，逐 atom 输出 `<text>`，
  带 `data-page` / `data-node` / `data-atom`。
- Gate V：SVG 字节 golden（`GOLDEN_SVG_HASH`）。
- Gate R：SVG 坐标必须等于布局坐标；HTML 与 SVG 必须覆盖同一组 atom。

### Q′ 效果记分卡

`soma-eval score --paragraphs N` 输出 CSV；`scripts/gate-q.sh` 运行结构断言并打印报告。

## 3. 关键数值（本次实测）

```
paragraphs,40
pages,18
greedy_pages,18
cost_kp,790336
cost_greedy,800192
lines,494
over_compress_lines,0
emergency_lines,0
continuation_blocks,16
widow_violations,0
orphan_violations,0
page_free_p95,1048
page_free_max,1048
ratio_jitter_p95,496
bdev_rel_error_p95,0
```

读法：

- 40 段、494 行、18 页；K-P 与贪心页数相同，但代价更低（`790336 < 800192`）。
- 段落跨页 16 次，widow/orphan 违规 **0**。
- 压缩越过 shrink 下限的行 **0**； emergency 行 **0**。
- 非末页底部留白 P95 = 1048‰（约 1.05 行），最大相同。
- `boundaryDeviation` 的近似值与精确值相对误差 P95 = 0（本语料上比例分配假设成立）。
- `ratio_jitter_p95 = 496‰` 仍高于提案里的 350 目标，属标定窗口（D05），不是结构缺陷。

## 4. 变更清单

新增：

- `crates/soma-metrics/`（`Cargo.toml`、`src/lib.rs`、`data/noto-cjk.smt`）
- `crates/soma-render-svg/`（`src/lib.rs`、`tests/gate_r.rs`）
- `crates/soma-layout/src/metrics.rs`、`src/pagination.rs`
- `crates/soma-eval/src/score.rs`、`tests/effect.rs`
- `tools/metrics-gen/`
- `scripts/gate-r.sh`、`gate-q.sh`、`effect-score.sh`
- ADR-0012…0015、本报告

主要修改：

- `soma-core`：`Metrics` 增加 `version`/`hash`。
- `soma-rules`：`-` 断点、`/` 规则、`PAIR_TRIM`、`trim.pair` 规则行。
- `soma-template`：9 个新属性、页面母版字段、`soma.template/2`、`soma.styles/2`。
- `soma-layout`：行级分页、母版、悬挂标点、网格、`soma.pages/3`、`metricsHash`、`verticalHash`。
- `soma-render-html`：绘制页眉页脚；span 带 `data-node`/`data-atom`。
- `soma-demo`、`soma-wasm`：改用真实度量。
- `docs/debt.md`、`roadmap-v2.md`、`template.md`、`README.md`、`PROVENANCE.md`。

## 5. 与提案的偏差（都已记入债务）

| 提案项 | 实际 | 债务 |
|---|---|---|
| E6 字间 tracking 兼底 | 只做断点，不做 letter-spacing | D60 |
| E1 bold 面 | 只发行 Regular，bold 用其宽度 | D61 |
| E4 首页/封面母版 | 只有一套母版 | D62 |
| E1 全字集 | Latin/标点/假名/CJK 基本区与扩展 A | D63 |
| E4 页眉页脚多行 | 单行居中 | D64 |

## 6. 复现

```sh
bash scripts/gates.sh          # 全部门禁
bash scripts/gate-q.sh         # 记分卡
bash scripts/gate-r.sh         # 视觉锁 + 跨渲染器
bash scripts/kp-verify.sh      # 内核基准与长度策略
cargo run -p soma-eval --release -- score --paragraphs 40
```

## 7. 仍未做

Gate Q 的真实盲测（D01）、Tier A 标定（D04/D05）、`soma.engine/1`（D29/D59）、
HTML/LaTeX 前端、PDF、增量重排。见 `docs/debt.md`。

---

## 8. 复测（2026-11-20）

E1–Q′ 之后又落了 ADR-0016…0043（模板、分页、PDF、SVG、度量表 v4、混排、CFF、
空格与 overfull、PDF 流压缩）。上面的数字是交付时的快照，这一节是**同一台机器、
同一份语料、当前构建**的复测，不覆盖历史读数。

### 构建

| 项 | 交付时 | 现在 |
|---|---|---|
| 测试 | 299 | **572** |
| schema | `soma.metrics/1`、`soma.template/2`、`soma.styles/2`、`soma.pages/3` | `soma.metrics/4`、`soma.template/9`、`soma.styles/7`、`soma.pages/9`、`soma.doc/2`、`soma.engine/1` |
| 模型版本 | `modelVersion 1` | `modelVersion 2`（ADR-0041） |

### 记分卡（40 段合成语料）

```text
pages,18                  greedy_pages,18
cost_kp,790336            cost_greedy,800192
lines,494                 over_compress_lines,0
emergency_lines,0         continuation_blocks,16
widow_violations,0        orphan_violations,0
page_free_p95,1048        page_free_max,1048
ratio_jitter_p95,445      bdev_rel_error_p95,0
```

与交付时唯一不同的读数是 `ratio_jitter_p95`：**496 → 445**。ADR-0041 让 overfull 的
齐行行不再当作正常断点，挤压行变少，行内调整量随之收敛。

### 真实语料（`corpus/zh`）

```text
document,pages,paragraphs,lines,over_compress,emergency,continuation,widows,orphans,fitness_jumps,page_free_p95,page_free_max,jitter_p95,bdev_error_p95
abstract,1,6,24,0,0,0,0,0,0,0,0,577,0
classics,2,15,42,0,0,0,0,0,3,0,0,942,0
editorial,2,7,30,0,0,1,0,0,0,423,423,905,0
footnotes,2,6,22,0,0,1,0,0,0,1580,1580,455,9
interview,2,15,38,0,0,0,0,0,0,0,0,608,0
mixed,1,6,24,0,0,0,0,0,0,0,0,640,0
notice,1,12,26,0,0,0,0,0,0,0,0,702,0
science,2,8,35,0,0,1,0,0,0,0,0,328,0
technical,2,8,32,0,0,0,0,0,0,0,0,591,0
table-page,2,123,125,0,0,0,0,0,0,278,278,0,0
math-page,1,3,5,0,0,0,0,0,0,0,0,452,0
book,30,390,810,0,0,0,0,0,0,486,486,337,0
ALL,48,599,1213,0,0,3,0,0,3,486,1580,368,9
```

- `over_compress` 0（ADR-0041 之前是 3）。
- `fitness_jumps` 3，全部在 `classics`，来自两行非常松的行；ADR-0042 记录它们为何是
  代价最优、而不是无解，以及为什么因此不引入 fitness DP 状态。
- `bdev_rel_error_p95 = 9‰`：比例分配假设在 599 段的真实语料上误差小于 1%。

### 产物大小（同一输入，ADR-0043 前后）

| 样张 | 未压缩 | 现在 |
|---|---|---|
| `anthology.pdf`（49 页，MiSans + 霞鹜） | 1,686,314 B | **527,273 B** |
| Noto Serif CJK 单字体（8 页，CFF） | 1,919,666 B | **1,627,922 B** |

### 仍缺

盲测与 Tier A 标定（D01/D04/D05）、图片嵌入与 tagged PDF（D82）、索引排序（D80）。
`docs/debt.md` 是唯一权威列表。
