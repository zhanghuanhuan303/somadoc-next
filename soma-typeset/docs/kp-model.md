# K-P 算法建模提案

**状态：** 提案，待拍板后进入 K1
**对应代码：** [`crates/soma-core/src/breaker.rs`](../crates/soma-core/src/breaker.rs)
**目标：** 先定模型，再优化性能。每个断行决策要可解释、可测试、可跨端复现。

---

## 0. 结论（先看 12 条）

1. **采用 K-P 的全局 DP 结构，但用加法加权代价，不用 TeX 的平方 demerits。** 这样才能满足 F6「每类惩罚有独立权重」，也更容易标定。
2. **输入是 `Atom` + `Boundary`；box/glue/penalty 是内部编译产物。** 中文规则在 IR 构建时编译进边界，breaker 保持脚本无关。
3. **`Forbidden` 是硬约束；`Forced` 单独建模。** 不要用负 penalty 模拟强制换行。
4. **标点挤压是行边裁剪，不是全局 glue。** 只在行超宽时裁剪行首/行尾；line cost 仍只依赖 `(i, j)`，DP 不受影响。
5. **中英边界间距偏差用「比例分配 + 前缀和」做 O(1) 聚合。** 不逐边界遍历。
6. **MVP 不把相邻行 fitness 放进 DP 状态。** 先测行间调整量方差；有证据再升级到 `(breakpoint, fitness)`。
7. **边裁剪会破坏行宽单调性，所以先不要上 TeX 式 active-node 剪枝。** 先 O(n²) + 前缀和，correctness first。
8. **所有量整数；舍入、tie-break、fallback 全部写进规范。** 这是 G4 的基础。
9. **强制换行按 segment 切分：** 段尾不拉伸、不触发短末行惩罚；下一段不重新首行缩进。
10. **验收用暴力枚举做参考最优解，不用 TeX/Typst 做参照。** F4 的「参考实现」就是我们自己的 brute force + 同一代价模型。
11. **样式和字号进入 `Metrics`，不进入断行算法。** 加粗/斜体/代码/大字号通过 `StyleId` 提供宽度与行高；样式切换不产生断点；真正的首字下沉不在 MVP。
12. **参数分层，默认值只是 preset。** 结构常量、规则比例、代价权重、样式度量、段落几何、评估协议各自独立；算法不写死 0.25em/2em/100/3000。详见 [`params.md`](params.md)。

---

## 1. 范围

### 1.1 本阶段目标

- 段落级全局优化（Paragraph window）。
- 中英混排、纯中文、纯英文共用一套模型。
- 禁断、强制换行、glue 伸缩、行边标点裁剪。
- 中英边界间距偏差进入代价模型。
- 样式（加粗/斜体/代码/字号）通过 `StyleId` 进入宽度与行高；行盒输出 ascent/descent/height。
- 整数、确定性、可序列化的 `LayoutResult`。
- 可选的 `KpTrace`，用于解释每行的代价构成和调参。

### 1.2 非目标

- 断词/连字符（MVP 明确不做）。
- **真正的首字下沉 / 多行 initial**（需要变行宽 + 浮动/绕排；阶段 3/4）。
- 页级/章级/整书级窗口。
- 相邻行 fitness / 行间节奏进入 DP 状态（MVP 不做）。
- `looseness`（指定行数）。
- 竖排、bidi、多栏、浮动体。
- 增量重排。

---

## 2. 形式化模型

### 2.1 符号

| 符号 | 含义 |
|---|---|
| `n` | atom 数量 |
| `a_i` | 第 `i` 个不可断单元（CJK 字、拉丁词、数字串、行内对象） |
| `w_i` | `a_i` 的宽度，来自 `Metrics::advance` |
| `b_k` | `a_k` 与 `a_{k+1}` 之间的边界 |
| `g_k` | `b_k` 的 glue 宽度；`Space` 边界由 `Metrics::space_width` 解析 |
| `s_k` / `h_k` | `b_k` 的 stretch / shrink |
| `p_t` | 断点；`0 = p_0 < p_1 < ... < p_m = n` |
| `[i, j)` | 一行，包含 atom `i..j-1`，`j > i` |
| `T(i)` | 行目标宽度 |
| `W(i,j)` | 行自然宽度 |
| `S(i,j)` / `H(i,j)` | 行内总 stretch / shrink |
| `r` | 伸缩比例（ratio） |
| `tol` | 允许的最大 `abs(r)` |

### 2.2 行宽与断点处的 glue

一行 `[i, j)` 的自然宽度：

```text
W(i, j) = Σ_{k=i}^{j-1} w_k  +  Σ_{k=i}^{j-2} g_k
S(i, j) = Σ_{k=i}^{j-2} s_k
H(i, j) = Σ_{k=i}^{j-2} h_k
```

**断点处的 glue 丢弃。** `b_{i-1}`（行首前）和 `b_{j-1}`（行尾后）不进入本行，这是 K-P/TeX 的标准行为，也自然解决「中英边界断行后不留下空格」。

用前缀和把 `W`、`S`、`H` 的查询做成 O(1)：

```text
P_w[k] = Σ_{t<k} w_t
P_g[k] = Σ_{t<k} g_t
W(i,j) = (P_w[j] - P_w[i]) + (P_g[j-1] - P_g[i])
```

### 2.3 目标宽度

```text
T(segment, local_i) = paragraph_width
                    - (segment.is_paragraph_start && local_i == 0 ? first_indent : 0)
```

- 首行缩进只作用于段落第一行；强制换行后的下一行不是新段落，**不重新缩进**。
- 段切分后不能用局部 `local_i == 0` 直接判断首行：必须同时满足 `is_paragraph_start`。这是 K5 的高发 bug 点。
- 页级/章级窗口以后才引入行号相关宽度；MVP 目标宽度只与段落首行有关。
- **枚举保留 `Justify/Left/Center/Right`；MVP 行为只实现 `Justify`。** 其他值返回 `Unsupported`；枚举先全，避免后续加变体成为破坏性 schema 变更。

### 2.4 可行性、ratio 与 tolerance

先算 `E = W(i,j) - T(i)`：

```text
E > 0   → 行超宽（overfull 候选）
E = 0   → 恰好
E < 0   → 行不足（underfull）
```

超宽时先做行边裁剪（见 2.5），得到 `E' = E - c`，其中 `c` 是裁剪量。

**ratio 的语义。** `r` 是相对**声明伸缩量**的有符号比例：`r > 0` 表示不足，`r < 0` 表示超宽；`|r| <= 1` 表示在名义 stretch/shrink 内，`|r| > 1` 表示超出名义量。`r` 不是行宽比例，也不是物理上限；实际调整量是 `r × S` 或 `r × H`。`tol` 只约束 `|r|`，不改变分配公式。

```text
若 E <= 0（不足或恰好）:
    c = 0
    D = -E
    若 D == 0:
        r = 0
    否则:
        S = S(i,j)
        若 S == 0:
            正常阶段：不可行（NoStretch）
            fallback：r = +BADNESS_RATIO_CLAMP（哨兵）；emergency = true
        否则:
            r = D / S

若 E > 0（超宽）:
    c = 行边裁剪量（见 2.5）
    E' = E - c        // 一定 >= 0
    若 E' == 0:
        r = 0
    否则:
        H = H(i,j)
        若 H == 0:
            正常阶段：不可行（NoShrink）
            fallback：r = -BADNESS_RATIO_CLAMP（哨兵）；overfull = true；emergency = true
        否则:
            r = -E' / H
            overfull = E' > H
            若 overfull:
                正常阶段：不可行
                fallback：cost += emergency；emergency = true
```

- **`S == 0 && D > 0` 不是完美行。** 它表示这一行没有任何可拉伸的边界，正常阶段必须进入 fallback；`BADNESS_RATIO_CLAMP` 是「无有限比例」的哨兵，不是物理比例。这样不会把「不足但无法拉伸」误判为 `r = 0`。
- **`H == 0 && E' > 0`** 同理；同时 `overfull = true`，因为即使耗尽名义 shrink 也放不下。
- **`overfull` 的精确含义**是裁剪后 `E' > H`：名义 shrink 全部用尽仍超宽。这就是 TeX 的 overfull box，badness 无穷，因此**正常阶段不可行**，只能经 fallback 到达（`cost += emergency`，`emergency = true`）。ADR-0041 之前它只是标记，结果是一行可以声明 `overfull: true` 却照样按 `-E'` 压缩、把词间空挤到粘连。`0 < E' <= H` 但 `|r| > tol` 是「超过容差但仍可压缩」，同样走 fallback，只是不记 `overfull`。
- **末行 / 强制段尾例外：** 它们不齐行，`adjustment = 0`，不进入上面的 S/H 分支；只允许 trim。裁剪后 `E' > 0` 时正常阶段不可行，fallback 记 `overfull + emergency`。
- **词间空的可压缩量是字体相关的：** 规则层给出 1/8 em 的上限，`soma-layout` 再把它夹到 `space_advance / 3`（TeX 的 space glue）。固定 em 在窄空字体上等于允许 40% 以上的压缩，宽空字体上又过于保守（ADR-0041）。
- `emergency_stretch > 0` 时，fallback 先把它加入 `S`；此时 `S > 0`，不需要哨兵。
- 可行性：`|r| <= tol`。`tol` 用千分比整数表示，例如 `tol = 1000` 表示 1.0。
- **`r` 与 TeX 的 tolerance 不是同一量。** TeX 的 `\tolerance=200` 是 badness 阈值；在 `p=3`、`scale=100` 下约对应 `|r| ≈ 1.26`。`default-v1` 的 `3000‰ = 3.0` 更宽松，它只是硬上限，不是首选比例；接近上限的行会被 badness 强烈惩罚。
- **联合约束：** 用于质量门禁的 preset 必须满足 `badness_scale >= line`，保证 `|r| = 1` 时 badness 至少等于一行固定代价；否则「多一行」可能比「把行拉到名义极限」更便宜。`default-v1` 为 `100 >= 10`。
- **正常阶段不使用紧急伸展。** 如果无可行路径，进入 4.5 的 fallback。

### 2.5 行边标点裁剪

#### 为什么不能建模成全局 glue

如果把「标点可挤压」做成每个标点边界的 shrink，那么行中间也会被压缩，导致整段标点都被挤扁。中文排版里的挤压主要发生在**行首/行尾**；行中间应该优先用字间距调整。

因此：

- `Atom` 带 `compress_left` / `compress_right`，表示该原子在行边最多能裁掉多少侧边距。
- 裁剪只在**行超宽**时发生。
- 裁剪量进入 line cost，但仍然是 `(i, j)` 的函数，所以 DP 不需要额外状态。

#### 裁剪顺序

```text
c_end   = min(max(E, 0), trim_right(j-1))
c_start = min(max(E - c_end, 0), trim_left(i))
c       = c_end + c_start
E'      = E - c
```

- **先裁行尾，再裁行首。** 行尾标点是常见情况；行首裁剪留给开引号/开括号。
- **model v1 只实现 `TrimPriority::EndFirst`。** `StartFirst` / `Balanced` 在 `ParamSet::validate` 阶段报错，不静默退化成 EndFirst；新增顺序要升 `modelVersion`。
- `Line.ratio_milli` 是**裁剪后**的 stretch/shrink 比例，直接用于 badness；末行与 forced 段尾不齐行，固定为 0。
- K4 起规则层把 `open_trim_left=250‰` / `close_trim_right=500‰` / `middle_trim_left/right=500‰` 写入 `Atom.compress_*`。这些**比例**仍是 `ParamOnly`，不是 Gate Q 硬断言；规则本身（行首禁开标点、行尾禁关标点）已是 `Verified`。
- 两侧都裁时，`Line.trim_left` / `Line.trim_right` 分别记录，便于渲染和评估。
- 如果 `E <= 0`，`c = 0`，**不裁剪**；不足的行应该用 stretch，而不是先裁再拉。
- 单原子行没有内部 glue，但仍允许裁剪两侧；`adjustment = 0`，只影响行宽和渲染。

#### 渲染坐标约定

提案采用以下约定，避免「裁剪后行宽对不上」：

- 第一个 atom 的 `x = indent - trim_left`（glyph 向左挂出）。
- 最后一个 atom 的 `advance = w_last - trim_right`（右侧边距被吃掉）。
- 如果首尾是同一个 atom，两条都生效：`x = indent - trim_left`，`advance = w - trim_right`。
- 内部 atom 的 `advance` 不变。
- 内部 glue 的调整量仍按 4.2 的比例分配。
- **不要对 CjkLatin 边界做「优先压缩」**：`D` 的聚合公式假设严格按 stretch/shrink 比例分配；优先策略会让 eval 方差与 cost 的 `D` 对不上。悬挂标点以后另立规则。
- 因此：`x_first + Σ advance + Σ (glue + adjustment) = indent + T`。

**box 坐标填充伪码（K4 直接照此实现）：**

```text
x = indent - trim_left
for k in i..j:
    advance = w_k
    if k == j-1:
        advance -= trim_right
    emit AtomBox { atom: k, x, advance }
    if k+1 < j:
        x += advance + glue_k + adjustment_k
```

- `adjustment_k` 是内部边界的分配量，总和等于 `Line.adjustment`。
- 首尾同一个 atom 时：`x = indent - trim_left`，`advance = w - trim_right`，右边缘 = `indent + w - trim_left - trim_right`。
- 这个伪码必须配 `single_atom_double_trim` golden，否则 K4 容易只测行宽不测逐 box 坐标。

`Line.natural_width` 仍是未裁剪的自然宽度；`Line.trim_left` / `Line.trim_right` 单独记录。渲染器或评估器用 `natural - trim_left - trim_right` 得到有效自然宽度。

#### 与规则层的关系

- 规则层决定每个标点的 `compress_*` 上限，以及「连续标点」是否可以叠加。
- breaker 只做确定性裁剪，不认识「中文逗号」。
- 如果未来要支持悬挂标点（hanging punctuation），可以把它建模成「允许负的 trim 超出内容盒」，不需要改 DP。

### 2.6 代价函数

#### 原则

- **加法加权代价**：每个物理项有独立权重，满足 F6。
- **不使用 TeX 的平方 demerits**：`(line_penalty + badness)^2 + penalty^2` 会让权重互相耦合，不利于标定。
- 所有项非负；总代价非负。
- 每个 `(i, j)` 的代价 O(1)。

#### 概念公式

```text
C(i, j) = line
        + badness_scale × abs(r)^badness_exponent
        + break_penalty × penalty(j) / PENALTY_REF
        + compress_amount × c / EM
        + compress_count × [c > 0]
        + boundary_deviation × D(i, j) / ideal_gap
        + (last ? short_score(i, j) : 0)
        + height_cost(i, j)
        + (emergency ? emergency : 0)
        + (forbidden ? forbidden : 0)
```

| 项 | 归一化量 | 权重含义 |
|---|---|---|
| `line` | 1 行 | cost / line |
| `badness_scale × abs(r)^p` | `abs(r)^p` | `|r|=1` 时的 cost |
| `break_penalty × penalty / 100` | 规则层 penalty（0..100） | penalty=100 时的 cost |
| `compress_amount × c / EM` | `c` 占 1 em 的比例 | 每 1em trim 的 cost |
| `compress_count` | 一次 trim 事件 | cost / event |
| `boundary_deviation × D / ideal_gap` | `D` 占 ideal gap 的比例 | 每 1 个 ideal gap 偏差的 cost |
| `short_score` | `short / short_last_threshold` | 末行为 0 宽时的 cost |
| `height_cost` | `excess / body_height` | 每 1 个 body height 超出的 cost |
| `emergency` / `forbidden` | 一次事件 | cost / event |

- `EM = 4096` base units；`PENALTY_REF = 100` 是规则层 penalty 的归一化上界。
- `ideal_gap` 来自 RuleSet；如果为 0，用 `EM` 代替。
- `body_height` 来自当前段落的最小正文行高。
- **所有权重都是「每个归一化单位的 cost」**，不再有「每 1/4096 em」这种与 `line` 不可比的量纲。
- `line` 对末行/段尾也计数（见 2.6.3）。

> `w_*` 是 `CostParams` 字段的简写；完整变量名、范围和默认 preset 见 [`params.md`](params.md)。

#### 2.6.1 badness 的整数化

概念值：

```text
B = badness_scale × |r|^badness_exponent
```

`badness_scale` 和 `badness_exponent` 是 Tier A 参数；`default-v1` 取 `scale=100`、`exponent=3`，但这只是 preset，不是常量。

整数实现：

```text
r_milli    = (adjustment * 1000) / capacity   // 超宽为负，不足为正；截断到零
r_clamped  = min(abs(r_milli), BADNESS_RATIO_CLAMP)
B_int      = (badness_scale * r_clamped^p) / 1000^p
```

- `capacity` 是 `S`（不足）或 `H`（超宽）。
- 归一化分母 `1000^p` **只随 exponent 变，不随 scale 变**；`p=3` 时是 `1_000_000_000`。
- `BADNESS_RATIO_CLAMP = 100_000` 是结构常量；正常路径 `abs(r_milli) <= tolerance <= 10_000`，只有 emergency 才会用到 clamp。
- 中间量用 `u128` 计算，最后 clamp 到 `i64::MAX`；`badness_scale` 建议范围 `0..=1_000_000`，此时 `p<=5` 也不会溢出。
- wasm32 上通用 `u128` 乘除比 `u64` 慢；`p` 只允许 1..=5，K6 可在 `p ∈ {2,3,4}` 时用受限整数幂表/分支乘法特化，但 MVP 先用 `u128` 保证正确。
- K1 已统一到这个口径；旧实现曾比公式大 1000×，旧结果不应再作为 baseline。
- 当 `r_milli` 很小（例如 100），`B_int` 可能截断为 0；这是可接受的，因为此时偏离很小。
- 如果性能敏感，K6 可把 `p=3` 且 `scale/ratio` 有界的路径特化为 i64；MVP 先用 u128 保证正确。
- 所有权重非负；配置加载时校验。

**与 `tolerance` 的耦合：** `tolerance` 和 `badness_scale` 不能独立标定。`default-v1` 的 `tolerance=3000` 只是起点；K1/K6 必须做 `(tolerance, badness_scale)` 二维敏感度扫描。ratio 接近 tolerance 时 badness 会远大于 `line`，这是设计后果，不是 bug。

#### 2.6.2 中英边界间距偏差

比例分配假设：行内调整量 `A`（不足时 `A = D`，超宽时 `A = E'`）按每个边界的 stretch/shrink 比例分配：

```text
boundary_adjust_k = A × w_k / G_total
```

其中 `w_k` 是 `s_k`（不足）或 `h_k`（超宽），`G_total = Σ w_k`。

对 role 为 `CjkLatin` 的边界，间距偏差的绝对值合计是：

```text
A × (Σ_{CjkLatin} w_k) / G_total
```

按 CJK-Latin 边界数量 `N_cl` 归一化，得到平均绝对偏差：

```text
D = A × sum_cjk / (G_total × N_cl)
```

- `sum_cjk`、`G_total`、`N_cl` 都用前缀和 O(1) 得到。
- 如果 `G_total == 0` 或 `N_cl == 0`，`D = 0`。
- `Line.boundary_deviation` 是 O(1) 近似标量；`Line.cjk_latin_adjustments` 输出每个 CjkLatin 边界的实际分配量（`boundary` + `adjustment`），eval/渲染器可据此精确校验比例分配假设。
- 评估层仍然计算「方差」；代价模型用平均绝对偏差，避免平方带来的溢出和权重放大。
- 这个公式依赖「比例分配」这个渲染约定；4.2 的坐标分配必须与它一致。

**可选的平方版本：** 如果后续评估发现平均绝对偏差不够，可以换成平方和，用 i128 或归一化比例计算；MVP 不用。

#### 2.6.3 末行与段尾

- 段落最后一行：不齐行，`adjustment = 0`；不做 badness；只计 `short_score`。
- `short_score`：

```text
effective_width = natural - trim_left - trim_right   // 末行 adjustment 不参与
short = max(0, short_last_threshold - effective_width)
short_score = short × short_last_scale / short_last_threshold
```

- `short_last_threshold` 默认 2em，`short_last_scale` 默认 1_000_000；两者都是 Tier A 参数。
- `effective_width` 使用**裁剪后**的自然宽度；末行 `adjustment` 不参与。
- 如果阈值设为 0，则不计算短末行项。
- **`w_line` 对末行和 forced 段尾也计数。** 理由是「行」是物理成本；`short_score` 是对末行过短的额外惩罚。这一点由 golden 锁死，避免实现者自行改语义。

- 最后一行裁剪后仍超宽：正常阶段不可行；fallback 允许，标记 `overfull + emergency`。末行不拉伸，也不按 shrink 比例分配。
- 强制换行的段尾：不拉伸、不触发 `short_score`；它是「有意短行」。
- 段落最后一行如果是 forced segment 的段尾，按段尾处理，不按段落末行处理。
- **跨段落节奏不在本模型内。** `short_last`/孤字是段落级指标；评估不期待跨段效果，跨段节奏不进入 Gate Q。

#### 2.6.4 断点 penalty

- `break_penalty(j)` 是边界 `b_{j-1}` 的 penalty；段落末尾为 0。
- **penalty 非负**（`u32` 或校验过的 `i32`）。不要用负 penalty 模拟强制换行；用 `Breakability::Forced`。
- 规则层可以用 penalty 表达「标点堆叠」「行末逗号」「连续关闭标点」等软约束。
- 硬禁断仍用 `Forbidden`，不进 penalty。

### 2.7 总代价与 DP

总代价：

```text
total = Σ_t C(p_t, p_{t+1})
```

DP：

```text
best[0] = 0
for j in 1..=n:
    if j < n and b_{j-1} is Forbidden: continue
    for i in 0..j:
        if best[i] is None: continue
        // 段内不应出现 Forced；由 Segment 预切分保证（K5）。
        c = line_cost(i, j)
        if c is None: continue
        cand = best[i] + c
        if cand is better than best[j]:
            best[j] = cand
            prev[j] = i
```

说明：

- `line_cost(i, j)` 包含**行尾断点** `b_{j-1}` 的 penalty；上一行的代价已经在它结束时计过。
- 段落末尾 `j = n` 没有断点 penalty。
- 强制换行建议**预切分 segment**，而不是在 DP 里检查「内部是否有 Forced」；这样每个 segment 是一个独立 DP，逻辑更简单。
- `best[j]` 相同代价时用 tie-break（2.8）。
- 重建：从 `n` 沿 `prev` 回溯，再反转。

### 2.8 确定性

- 所有宽度、cost、ratio 用整数（`i32` / `i64`）；核心不出现 `f32`。
- 除法统一截断到零；分配余数时按边界顺序从前往后分配。
- DP 迭代顺序固定：`j` 升序、`i` 升序；只有严格更优才替换。
- tie-break：
  1. 总代价最小；
  2. 行数最少；
  3. 在相同 `(cost, line_count)` 下，优先选择更小的行起点（即更长的一行）。
  - 这是**每个状态的局部确定性 tie-break**，最终表现为末行起点更小；不是全局字典序。够用且实现简单。
  - **跨实现字节一致的前提是迭代顺序固定**（`j` 升序、`i` 升序，只有严格更优才替换）。K1 必须用暴力枚举 + tie fixture 锁死；如果以后要实现全局字典序，需要为每个状态存 path rank，成本是 O(行数) 比较，n=2000 时最坏约 400 万次比较，可接受但不在 MVP。
- `LayoutResult` 带 `schema` / `modelVersion` / `preset` / `rulesVersion` / `metricsVersion` / `paramHash`。
- **兼容策略：** 只有 `schema` 相同才比较 canonical 字节；reader 必须忽略未知字段；新增字段/语义变化都升 `schema`。旧 reader 可以宽松读取新 schema，但不能宣称字节一致。
- 任何改变 canonical 字节的模型变化都要升 schema 版本。

### 2.9 样式、字号与行高

加粗、斜体、代码、大字号、上下标都不应该改变断行算法的结构。它们统一通过 `StyleId` 进入 `Metrics`。`StyleId` 是宿主样式表的索引，同一 `metricsVersion` 下必须稳定；它可以编码字体族、粗细、斜体、字号、基线偏移等。**字号也是 `StyleId` 的一部分**：核心不解析 pt/em，只从度量表拿宽度和行高；换字号 = 换 style + 度量表，不需要改 IR 或算法。

- **宽度**：`Metrics::advance(text, style)` 按样式返回。粗体通常更宽，斜体可能更窄/有 overhang，代码字体是另一套 advance。
- **行高/基线**：新增一个按样式/文本查询的 `GlyphMetrics`，供行盒计算使用。
- **断点**：样式切换本身**不产生断点**；规则层在样式边界切 atom，但必须保持词/短语的不可断关系。

#### 2.9.1 `GlyphMetrics`

```rust
pub struct GlyphMetrics {
    pub advance: Width,
    pub ascent: Width,
    pub descent: Width,
    pub leading: Width,
    /// 正数表示基线上移（上标），负数表示基线下移（下标）。
    pub baseline_shift: Width,
    /// 视觉溢出，不参与断行宽度；供渲染器使用。
    pub overhang_left: Width,
    pub overhang_right: Width,
}
```

`Metrics` 增加带默认实现的方法，避免破坏已有实现：

```rust
fn glyph_metrics(&self, text: &str, style: StyleId) -> GlyphMetrics {
    let line = self.line_metrics(style);
    GlyphMetrics {
        advance: self.advance(text, style),
        ascent: line.ascent,
        descent: line.descent,
        leading: line.leading,
        baseline_shift: Width::ZERO,
        overhang_left: Width::ZERO,
        overhang_right: Width::ZERO,
    }
}
```

- `advance` 仍是断行热路径；`glyph_metrics` 主要用于行盒、基线、分页。
- 真实字体表可以对 Latin run 取该 run 内 glyph 的最大 ascent/descent；测试假表可以按样式返回固定值。
- **`overhang_*` 与 `trim_*` 是两件事。** `overhang` 是视觉溢出（斜体/装饰 glyph），不进入 advance、不参与断行、不参与 trim；`trim_left/right` 是 RuleSet 给出的**布局压缩**，会改变行宽和渲染坐标。两者不能互相替代。
- 悬挂标点以后用「负 trim 超出内容盒」建模，不用 overhang。
- 跨样式边界默认不做 kerning/连字；如果需要，Metrics 必须提供稳定的 pair 结果，但 MVP 不做。
- 粗体/斜体应来自真实字体的度量或宿主明确提供的合成度量；核心不猜测伪粗体宽度。

#### 2.9.2 行盒

一行 `[i, j)` 的行盒由该行所有 atom 的 `GlyphMetrics` 决定：

```text
ascent  = max_k (ascent_k + baseline_shift_k)
descent = max_k (descent_k - baseline_shift_k)
leading = max_k leading_k
content_height = ascent + descent + leading
height  = max(content_height, style.line_height_min)
baseline = top + ascent
```

- `descent` 夹到 `>= 0`。
- `height_scale > 0` 时，range-max 表存 **clamp 前**的 `descent_k - baseline_shift_k`，查询后再 clamp。因为 `max_k max(0, d_k) = max(0, max_k d_k)`，两者等价；K3 不要写成「先逐 atom clamp 再取 max」的不同口径（也可行，但表更大）。
- n=4,000 时，ascent/descent/leading 三张 sparse table 约 1.1MB；可接受，只在 `height_scale > 0` 时构建。
- 所有 atom 共享一条基线；上下标通过 `baseline_shift` 调整。
- `leading` 的上下分配由渲染器决定（half-leading 等）；核心只输出总量和 `height`。
- `ParagraphStyle.line_height` 变成**最小行高/正文行距**：
  `actual_height = max(content_height, style.line_height_min)`。
- `Line` 输出 `ascent` / `descent` / `leading` / `height`，供渲染和未来的分页使用。
- `LayoutResult` 不重复存 `StyleId` / `baseline_shift`；渲染适配器持有 `Paragraph` IR，按 atom index 查样式。这样 canonical 结果保持精简。
- 行首大字（同一基线、更大字号）只是让 `ascent/descent/height` 变大，不需要额外 DP 状态。
- 如果宿主使用基线网格（baseline grid），核心只输出内容行高；网格吸附由渲染/分页适配器处理（可选，MVP 不做）。

#### 2.9.3 行高是否进入代价

- **MVP 默认不进入代价**：`height_scale = 0`，只输出行高，评估层统计行高方差。
- 如果需要抑制「某行突然变得很高」，可启用：

  ```text
  height_cost = height_scale × max(0, height - height_threshold) / body_height
  ```

  - `body_height` 取当前段落的最小正文行盒；归一化后 `height_scale` 的含义是「每超出 1 个正文行高的 cost」。
  - `height_threshold` 默认取正文字号的标准行盒（body height）；它是 Tier A 参数。
  - 一旦启用，`eval_line(i, j)` 需要 O(1) 的行内最大 ascent/descent/leading。推荐 sparse table（O(n log n) 内存，O(1) 查询）；只在 `height_scale > 0` 时构建。
  - 相邻行高度跳变（`|height_prev - height_cur|`）需要 DP 状态或后处理；MVP 不做，但要作为评估指标：`line_height_jump_count`，阈值默认 `0.5 × body_height`。默认是 **target**；启用 `height_scale > 0` 时升为 **guard**，因为此时模型声称控制行高。
- 大字号本身不是问题，**行高方差**才是问题；默认不惩罚大字，避免把作者有意的强调压平。

#### 2.9.4 样式与断点的关系

- 加粗/斜体/链接/普通强调：宽度由 `Metrics` 决定；没有额外代价；样式切换不产生断点。
- 行内代码：建议作为一个 `InlineObject` 或不可断 atom；宽度包含 padding/border；MVP 不在代码内部断行。
- 上标/下标：用 `StyleId + baseline_shift` 表示；与基字的边界设为 `Forbidden`，避免 `x` 和 `²` 被拆到两行。
- 行内数学：`InlineObject`；宽度和行高由宿主提供；不可断。
- 图片/注脚引用：`InlineObject`；不可断；参与行高。
- Emoji：普通 atom；宽度/行高由宿主提供；按普通断点规则处理。
- 如果作者希望「整个加粗短语/链接内部不换行」，那是规则层的短语约束：在短语边界加 `Forbidden` 或 penalty，而不是给样式加隐式规则。

### 2.10 参数化：变量，不是魔法数字

模型里的 0.25em、1/32em、2em、100、3000 都只是 `default-v1` 的取值，不是算法常量。完整参数系统见 [`params.md`](params.md)。

分层：

| 层 | 内容 | 谁解析 |
|---|---|---|
| C 结构常量 | 1/4096 em、`BADNESS_RATIO_CLAMP`、`PENALTY_REF`、DP、舍入、tie-break | 代码/模型版本 |
| B 规则参数 | gap、trim、penalty、禁断/强制表 | 规则层 → `ParaIR` |
| A 代价/布局 | tolerance、权重、阈值、fallback、flags | breaker 读 `BreakParams` |
| S 样式/度量 | advance、ascent/descent、space、line-height | `Metrics` |
| P 段落 | width、indent、align、line_height_min | `ParagraphStyle` |
| E 评估 | 样本数、评审人数、胜率阈值、CI/κ | 评估协议，独立于布局 |

原则：

- **算法只读 `BreakParams`**；中文规则认 `RuleSet`；字体认 `Metrics`；段落认 `ParagraphStyle`。
- 默认值是 `default-v1` preset；算法里不写 `if param == 100` 这类业务分支。
- 代价权重按请求/文档，不按段落；规则比例有 `RULES_VERSION`；样式度量有 `METRICS_VERSION`。
- 只标定约 8 个 Tier A 参数；Tier B 用规则预设；Tier C 不动。
- 结果携带 `modelVersion` / `preset` / `rulesVersion` / `metricsVersion` / `paramHash`。

---

## 3. 中文与混排的具体建模

### 3.1 atom 化

| 源文本 | atom | style | 说明 |
|---|---|---|---|
| 汉字 | 单个 CJK atom | `StyleId` | 可逐字断行；粗体/斜体只改宽度 |
| 中文标点 | 单个 CJK atom + `PunctClass` | `StyleId` | 参与禁断/裁剪 |
| 拉丁词 | 一个 `LatinRun` | `StyleId` | 内部不拆；MVP 不做断词 |
| 数字/单位 | 一个 `DigitRun` | `StyleId` | `3.14`、`50%` 保持一个原子 |
| 行内代码/对象 | 一个 `InlineObject` | `StyleId` | 不拆；宽度含 padding/border |
| 上下标 | 独立 atom | `StyleId` + `baseline_shift` | 与基字之间禁断 |
| 空白 | 不产生 atom | — | 产生 `BoundaryKind::Space` |

### 3.2 边界类型

| 边界 | `kind` | glue | 断行 |
|---|---|---|---|
| 两个 CJK 原子 | `Normal` | 0 + 小 stretch/shrink | 允许 |
| CJK ↔ 拉丁/数字，无空格 | `Normal` | 0.25em + stretch/shrink | 允许 |
| 拉丁词之间的空格 | `Space` | 宽度来自 `Metrics::space_width` | 允许 |
| 关闭/点号之前 | `Normal` | 0 | **禁断** |
| 空格 + 关闭/点号 | `Space` | 空格宽度 | **禁断**（空格仍保留在行内） |
| 开标点之后 | `Normal` | 0 | **禁断** |
| 两个拉丁 run 紧邻 | `Normal` | 0 | **禁断**（不拆词） |
| 强制换行 | `Breakability::Forced` | 断点 glue 丢弃 | **必须断** |

> 建议给 `Boundary` 增加 `role`（`Plain` / `CjkLatin` / ...）和 `Breakability::Forced`；`role` 用于边界偏差代价，`Forced` 用于段切分。不要把 CJK-Latin 语义塞进 `kind`。
>
> 表中的 0.25em、stretch/shrink 只是 `default-v1` 的 RuleSet 取值；实际值来自 [`params.md`](params.md) 的 Tier B，不是 breaker 常量。

- **长英文词/URL 不拆。** 如果单个 LatinRun 超过行宽，正常阶段不可行；fallback 允许 `Overfull` 并标 `overfull`。它不计入「英文拆断数」，评估单列 `overfull_lines`。

### 3.3 标点类别与禁断

第一版规则：

- `Open`：`（【《〈「『“‘([{`
- `Close`：`）】》〉」』”’)]}`
- `Middle`：`，。、；：！？…,.;:!?`
- `None`：其他

禁断：

- `previous.class == Open` → 禁断（开标点后不换行）。
- `next.class == Close || next.class == Middle` → 禁断（关闭/点号前不换行）。
- 两个 word-like atom 紧邻 → 禁断（不拆英文词/数字串）。

这些是硬约束（前提是规则状态为 `Verified`）；正常阶段不可违反，fallback 才允许并标记 violation。

> **规则状态：** 每条规则带 `status: Verified | Tbd | ParamOnly`。`Tbd` 规则只能产生 `Allowed` + penalty/trim，不能产生 `Forbidden` / `Forced`；`ParamOnly` 只提供参数，不产生断点规则。否则未核实的规则会污染 golden，并被误当成硬约束。
>
> K0 交付一张 `ruleId × source × status × golden` 盘点表；Gate Q 只对 `Verified` 规则做 guard 断言，`Tbd` 只报告。K6 已提供 `soma_rules::RULES` 元数据；当前盘点为 8 `Verified` / 0 `Tbd` / 6 `ParamOnly`：标点硬规则已引用 GB/T 15834-2011 via W3C clreq §6.1.1（basic），trim 比例仍是 `ParamOnly` 待标定。

### 3.4 挤压优先级

- 行尾标点先裁（`compress_right`）。
- 行首标点后裁（`compress_left`）。
- 两侧上限来自规则层；连续标点的叠加规则也在规则层。
- 挤压上限可以随样式/字号调整（大字号标点的侧边距更大）；规则层写入 `Atom`，breaker 不关心。
- trim 比例是 Tier B 规则参数（见 [`params.md`](params.md)），不是 `CostParams` 的权重。
- 裁剪只发生在超宽行；不足行不裁。
- 裁剪成本和次数进入 line cost。

### 3.5 中英边界

- 有空格：`Space` 边界，宽度来自 `Metrics::space_width`；不再额外加 0.25em。
- 无空格：`CjkLatin` role，ideal gap = 0.25em（待 clreq 核实），带 stretch/shrink。
- 断在该边界时 glue 丢弃，下一行不留下空隙。
- 边界间距偏差按 2.6.2 聚合；只在 `role == CjkLatin` 的边界上计算。

### 3.6 末行与孤字

- 末行不齐行。
- 末行有效宽度 < 2em 时计 `short_score`。
- 「孤字成行」由评估层从 `LayoutResult` 计数；代价模型只负责降低它的概率。
- 段尾/强制换行段尾不触发短末行惩罚。

### 3.7 强制换行 / verse

- `Breakability::Forced` 表示必须在此断行。
- 建议预切分：`[start, end)` 每个 segment 独立 DP。
- 段尾：不拉伸、不触发短末行惩罚；cell 可标记 `forcedEnd`。
- 下一段第一行不重新首行缩进。
- Markdown hard break（行尾两个空格、反斜杠、`<br>`）由前端转成 `Forced`。

**段切分的 index 约定（K5 必锁）：**

```rust
struct Segment {
    start: usize,              // 全局 atom index，含
    end: usize,                // 全局 atom index，不含
    is_paragraph_start: bool,
    is_paragraph_end: bool,
}
```

- 段内 DP 用局部 index `0..segment.len()`，但 `T` 必须判断 `segment.is_paragraph_start && local_i == 0`，不能只看局部 `i == 0`。
- 段尾 `is_paragraph_end == false` 时，末行不拉伸、不触发 `short_score`；`Line.forced_end = true`。
- 段首 `is_paragraph_start == false` 时，不重新首行缩进。
- golden 必须覆盖「forced 后段首不缩进」和「段尾不短末行」两个 case。

**K5 实现口径：**

- breaker 在进入 DP 前用 `split_segments` 扫描 `Forced`，每个 segment 独立跑 DP；segment 内部只允许 `Allowed` 断点。
- 段尾行使用 `LineEnd::ForcedEnd`：先 trim，裁剪后仍超宽才走 fallback（`overfull + emergency`）；不拉伸、不计 `short_score`，`Line.forced_end = true`。
- 全局 `max_lines` 在 segment 合并后统计；`features.forced_breaks == false` 且 IR 含 `Forced` 时返回 `BreakError::ForcedBreaksDisabled`，不静默当成 `Allowed`。
- 因为 DP 使用全局 atom index，段首缩进仍可安全地用 `start == 0` 判断；不要改成只看局部 `i == 0`。

### 3.8 一个手算例子

文本：`中文 English，混排`；宽度 7em；`SyntheticMetrics`：

| atom | 宽度 |
|---|---|
| 中 | 1em |
| 文 | 1em |
| English | 3.5em |
| ， | 1em |
| 混 | 1em |
| 排 | 1em |

自然宽度：`1+1+3.5+1+1+1 = 8.5em`，加上空格 `0.25em` = `8.75em`。
宽度 7em 时：

- 行 `[0,4)` = `中文 English，`：自然宽度 `1+1+3.5+1 + 0.25 = 6.75em`；不足 `0.25em`；stretch = CJK gap（1/32em）×2 + space（1/8em）= `0.0625 + 0.125 = 0.1875em`；ratio = `0.25/0.1875 ≈ 1.333`；badness = `100 × 1.333^3 ≈ 237`。
- 行 `[4,6)` = `混排`：2em，末行；`short_score = 0`（恰好 2em）。
- 总代价约为 `w_line×2 + 237`。
- 禁断边界 `English → ，` 不能被选为断点，所以不会出现「行首逗号」。

这个例子用来写 golden test；数值以实际整数实现为准。

### 3.9 样式 run 与断点

样式是 **IR 属性**，不是 breaker 的隐式规则。规则层在构建 `ParaIR` 时负责把样式 run 切成 atom，并决定哪些边界不可断。

| 特性 | atom/边界 | 宽度 | 断行 | 行高 | MVP |
|---|---|---|---|---|---|
| 加粗 / 斜体 | atom + `StyleId` | `Metrics` | 普通规则 | 样式行高 | 是 |
| 行内代码 | `InlineObject` | 含 padding/border | 不可断 | 样式行高 | 是 |
| 链接 | atom + `StyleId` | `Metrics` | 普通规则 | 样式行高 | 是 |
| 上标 / 下标 | atom + `baseline_shift` | `Metrics` | 与基字禁断 | 行盒计算 | 是 |
| 行内数学 | `InlineObject` | 宿主提供 | 不可断 | 宿主提供 | 是（作为对象） |
| 注脚引用 | `InlineObject` | 宿主提供 | 不可断 | 宿主提供 | 是 |
| 图片 | `InlineObject` | 宿主提供 | 不可断 | 宿主提供 | 是（作为对象） |
| Emoji | atom | 宿主提供 | 普通规则 | 宿主提供 | 是 |
| 2em 行内大字 | atom + `StyleId` | `Metrics` | 普通规则 | 行盒变大 | 是 |
| 首字下沉 / 多行 initial | 特殊 initial | 特殊 | 变行宽 | 多行 | **否**（见 3.10） |
| 标题 / 引文 | 独立段落/块 | 自己的样式 | 自己的 K-P | 自己的行高 | 是（独立处理） |

规则：

- **一个 atom 只能有一个 `StyleId`。** 规则层必须在样式边界切分 atom；`StyleId` 不进入 `Metrics` 之外的核心逻辑。
- **样式切换不产生断点。** 一个拉丁词被加粗拆成 `LatinRun` 两段时，边界必须是 `Forbidden`；否则会把词拆开。当前规则层的「两个 word-like atom 紧邻 → 禁断」已经覆盖这种情况，但后续要加测试锁住。
- **CJK 样式 run**：每个汉字仍是独立 atom，样式变化不阻止逐字断行（除非标点规则）。
- **短语级 no-break**：如果作者希望「整个加粗短语/链接内部不换行」，由规则层在短语边界加 `Forbidden` 或 penalty；不要给样式加隐式规则。
- **上下标**：`x²` 必须是一个基字 + 一个上标 atom，边界 `Forbidden`；行盒用 `baseline_shift` 计算。
- **注脚引用 / 行内数学 / 行内图片**：对象本身不可断；与基字/相邻词的边界由规则层决定，默认 `Forbidden`，避免对象被孤立到下一行。
- **行内代码**：MVP 不拆；如果未来要允许代码内换行，用特殊的 `CodeSpace` 边界，而不是普通空格。
- **大字号**：不改变断点规则，只改变宽度和行高；`height_scale` 默认 0。

两个具体例子：

- `un**believ**able` → `un` / `believ` / `able` 三个 `LatinRun` atom，两个边界 `Forbidden`。加粗只改 `advance`，不产生断点，词不会被拆开。
- `**大**字` → 一个 2em 的 CJK atom + 一个普通 CJK atom。行盒的 `ascent/descent/height` 变大；断行规则不变；如果该行因此超宽，走正常的 trim/shrink/emergency，而不是特殊分支。

**未纳入 MVP 的样式特性（先留接口，不实现）：**

- **Ruby / 注音**：需要上下两层行盒和额外的宽度/行高；阶段 3+。
- **着重号 / emphasis dots**：文字上方或下方的注记；影响行高和渲染，不影响断行宽度；阶段 3+。
- **下划线 / 删除线**：视觉属性，不改变 advance；渲染器处理，breaker 不关心。
- **真正的首字下沉**：见 3.10。
- **竖排 / bidi**：非目标。

### 3.10 行首大字 / 首字下沉

“行首大字”有三种，必须分开：

1. **行内大字号字符**（同一基线，字号更大）
   - 普通 atom；宽度、ascent/descent 来自 `Metrics`。
   - 行盒变高；断行规则不变。
   - 如果业务上希望它和后一个字不拆开，由规则层把边界设为 `Forbidden`；这不是字号本身的属性。
   - **MVP 支持。**

2. **大号开标点 / 装饰性 glyph**
   - 普通 atom + `compress_left/right` / overhang；
   - 行首裁剪和禁断规则照常。
   - **MVP 支持。**

3. **真正的首字下沉 / 多行 initial**（大字占 2–3 行，正文绕排）
   - 需要浮动体/绕排布局，以及前 N 行的**变行宽**；
   - K-P 的 `T(i)` 不再只依赖 `i == 0`，还要依赖行号/可用宽度；
   - 需要额外的碰撞/绕排 pass，超出 PRD 的 MVP 非目标（图文混排、浮动体）。
   - **MVP 不支持**，作为阶段 3/4 的扩展。

如果只是想要“第一个字看起来大一点”，用第 1 种即可，不需要首字下沉。

**扩展到真正的 initial 时，需要两件事：**

- `ParagraphStyle` 增加 `line_width_schedule`（前 N 行可用宽度）；`T` 变成 `T(i, line_index)`，DP 状态需要行号，或先做变行宽预布局。
- `AtomKind::Initial { span_lines }` + 渲染器浮动定位；breaker 仍然只负责断行，不负责绕排几何。

这两项都不属于 MVP；不在 K1–K8 内。

---

## 4. 算法与实现

### 4.1 当前参考实现

`breaker.rs` 已有：

- `Shaped`：atom 宽度 + glue/stretch/shrink 前缀和。
- `eval_line`：O(1) 行评估。
- `optimal`：O(n²) DP。
- `greedy`：Greedy-F / Greedy-C。
- `LineEval`、`LayoutResult`、canonical JSON。
- 首行缩进、末行短行惩罚、emergency fallback。

本提案的 K1–K5 是在这个骨架上补齐模型，不重写算法。

### 4.2 目标数据结构

```rust
struct Shaped<'a> {
    paragraph: &'a Paragraph,
    atom_width: Vec<i64>,
    glue: Vec<i64>,
    stretch: Vec<i64>,
    shrink: Vec<i64>,

    // 当前 segment 的归属；target() 用它判断首行缩进，不能只看局部 i == 0。
    is_paragraph_start: bool,
    is_paragraph_end: bool,

    // 行宽/伸缩 O(1) 查询
    atom_prefix: Vec<i64>,
    glue_prefix: Vec<i64>,
    stretch_prefix: Vec<i64>,
    shrink_prefix: Vec<i64>,

    // 中英边界偏差 O(1) 查询
    cjk_latin_stretch_prefix: Vec<i64>,
    cjk_latin_shrink_prefix: Vec<i64>,
    cjk_latin_count_prefix: Vec<u32>,

    // 行高：默认不参与 cost，只在 build_result 里对已选行扫一遍；
    // 只有 height_scale > 0 时才构建 range-max。
    // 预先算好 ascent + baseline_shift / descent - baseline_shift
    ascent_range_max: Option<RangeMax>,
    descent_range_max: Option<RangeMax>,
    leading_range_max: Option<RangeMax>,
}

struct LineEval {
    cost: Cost,
    target: i64,
    natural: i64,
    effective: i64,
    adjustment: i64,
    ratio_milli: i32,
    trim_left: i32,
    trim_right: i32,
    boundary_deviation: i64,
    break_penalty: i64,
    badness: i64,
    ascent: i64,
    descent: i64,
    leading: i64,
    height: i64,
    overfull: bool,
    emergency: bool,
}
```

- `adjustment` 是裁剪后的 signed 调整量，用于分配内部 glue。
- `ratio_milli` 进入 trace 和评估；不直接改变 DP。
- `trim_left` / `trim_right` 进入 `Line`。
- `boundary_deviation` 进入 `Line`；eval 另算逐边界的真实方差，并报告两者差异。这是监控「比例分配假设」是否失效的指标。
- `cjk_latin_adjustments` 只在 `build_result` 对已选行计算，不进入 DP 热路径；每个元素是 `{ boundary, adjustment }`，总 O(n)。
- `break_penalty` 来自 `b_{j-1}`；`j == n` 时显式取 0，禁止越界读 `b_{n-1}`。
- `ascent` / `descent` / `leading` / `height` 默认在 `build_result` 里对已选行扫描得到（总 O(n)）；只有 `height_scale > 0` 时才需要在 `eval_line` 里 O(1) 查询。

### 4.3 迭代顺序与分配

- DP：`j` 升序、`i` 升序。
- 内部 glue 调整：不足时按 `stretch` 比例分配；超宽时按 `shrink` 比例分配。
- 余数按边界顺序从前往后分配。
- 如果总权重为 0：平均分配（仅 emergency/路径退化时出现）。
- 单原子行：`adjustment = 0`，不做分配。
- 中英边界偏差公式假设的正是「按 stretch/shrink 比例分配」；2.6.2 和 4.3 必须保持一致。

### 4.4 性能、长度策略与剪枝

- 参考实现 O(n²)，每行 O(1)。
- `n = 2,000` → 约 200 万次行评估；**早期估算** native 5–20ms、wasm 20–80ms。整数除法是主要常数因子，<5ms 的早期估算偏乐观；K6 实测见下。
- `n = 4,000` → 约 800 万次；**早期估算** 20–80ms native、wasm 更高。K1 用真实骨架测过 n=2,000 / n=4,000，K6 又加了宽度窗口。
- **确定性长度策略（不是超时）：** `BreakParams.max_kp_atoms: Option<u32>`。
  - `None`：不限制，全部走 K-P。
  - `Some(k)` 且 `n > k`：该段落改用 **Greedy-C**（同一套 `line_cost`），结果标 `breaker_used = GreedyC`、`length_limited = true`。
  - **禁止用 wall-clock 时间决定算法**；否则同一输入在不同机器上会得到不同断点，破坏 G4。
  - K6 实测 4,000 atoms 在 native 预算内，因此 `default-v1` 保留 `max_kp_atoms = None`；改这个默认值要换 `PRESET_ID`。
  - **K1 必须报告 `length_limited` 占比**（总量、按 atom 桶、关键质量样本）；关键样本 >20% 时，质量结论只覆盖 `Optimal` 子集。
- `max_lines` 作为调用方保护，不是核心模型的一部分。
- **MVP 实测结论：** 4,000 atoms 已在预算内，所以默认不启用长度兜底；策略仍保留为确定性开关（`max_kp_atoms` + Greedy-C + 标记），WASM 实测超预算时再启用。
- **K6 实测结果（native release，带规则 glue）：** 纯中文 2,000 atoms P95 约 0.27ms、4,000 约 0.53ms；混排 4,000 约 0.58ms；标点密集 4,000 约 1.23ms。早期 31ms/138ms 的数字来自未加宽度窗口的全量扫描。4,000 atoms 已在预算内，因此 `default-v1.max_kp_atoms` **保留 `None`**；长度策略仍实现并用测试锁定，WASM 实测超预算时再设 `Some(2000)`。
- **K6 采用的保守剪枝：** 对每个段落计算可能的 atom 数窗口 `[k_min, k_max]`：
  - 不足方向：`k` 个 atom 的最大自然宽度 + 最大 stretch（含 emergency）仍达不到最小 target → 不可能满足；
  - 超宽方向：`k` 个 atom 的最小自然宽度 - 最大 trim 仍超过最大 target + 最大 shrink → 不可能满足。
  - 窗口只用全局上下界，可能偏宽，但**不会丢弃可行行**；`allow_emergency == true` 时禁用（emergency 行可任意短/宽），末行/forced 段尾不受窗口约束。
  - 差分测试：`brute_force_matches_dp*`、`pruning_matches_brute_force_on_varied_widths`、`pruning_reduces_the_candidate_count`。
- **没有实现 active-node deactivation。** 宽度窗口已把实测预算压到远低于上限，继续上 deactivation 的收益不足以抵消 trim 破坏单调性的风险。

### 4.5 Fallback ladder

```text
1. normal:
   tolerance = BreakParams.tolerance
   emergency_stretch = BreakParams.emergency_stretch（通常 0）
2. emergency:
   tolerance = FallbackParams.emergency_tolerance
   emergency_stretch = FallbackParams.emergency_stretch
   每行 emergency flag + CostParams.emergency
3. forbidden rescue（只在需要时）:
   允许 Forbidden 断点，记 violation + CostParams.forbidden
4. overfull:
   允许行超过目标；overfull flag
```

- 上面的 `3em` 只是 `default-v1` 的起点；实际值来自 `FallbackParams`。
- 每一级都用同一套 `line_cost`，只增加允许条件和 emergency 项。
- 结果永远完整；不 panic。
- 评估层把 emergency/overfull/violation 计入报告，不把它们藏起来。
- `LayoutResult` 显式输出 `fallback_stage`（`Normal | Emergency | ForbiddenRescue | Overfull`）、`emergency_lines`、`forbidden_violations`、`breaker_used`、`length_limited`。
- **`Line.overfull` 与 `FallbackStage::Overfull` 不是同一概念。** 前者表示裁剪后 `E' > H`（超过名义 shrink），可以在 `Normal` 阶段出现；后者表示允许最终行宽超过 target。评估时必须分开统计。
- **`total_cost` 只在同一 `(modelVersion, preset, rulesVersion, metricsVersion, fallback_stage)` 下可比。** 跨 fallback 阶段比较总代价没有意义；评估层必须按阶段分组。
- `forbidden_violations > 0` 只在 `ForbiddenRescue` 阶段允许；`Normal` 阶段必须为 0。
- **K6 实现状态：** 已实现 1/2/3（Normal → relaxed 无 emergency 标记 → emergency 标记 → ForbiddenRescue）；`fallback.emergency_tolerance` 与 `fallback.emergency_stretch` 生效，`allow_overfull` 控制紧急阶段的超宽行。4（`FallbackStage::Overfull`）仍保留为 schema 值，当前由 emergency 覆盖；`forbidden_violations` 只在 rescue 阶段非零。

### 4.6 Greedy 基线

| 模式 | 规则 | 用途 |
|---|---|---|
| Greedy-F | 取能放下的最远断点 | 工业基线；盲测的「对手」 |
| Greedy-C | 取当前行代价最小的断点，无前瞻 | 同代价模型消融 |

- `K-P ≤ Greedy-C` 是硬不变量（同一代价模型、K-P 搜索更全；`length_limited` 时不运行 K-P）。
- `K-P ≤ Greedy-F` 不保证；F 不是代价最优，只是行为基线。
- 两种贪心都必须走同一个 `line_cost`，否则对比不公平。

### 4.7 Trace

新增独立 API（不进入 canonical JSON）：

```rust
struct LineTrace {
    start: u32,
    end: u32,
    target: i64,
    natural: i64,
    effective: i64,
    stretch: i64,
    shrink: i64,
    ratio_milli: i32,
    badness: i64,
    ascent: i64,
    descent: i64,
    leading: i64,
    height: i64,
    trim_left: i32,
    trim_right: i32,
    compression_cost: i64,
    boundary_deviation: i64,
    /// 只在 trace 模式计算：逐边界真实平均绝对偏差，O(行长)。
    boundary_deviation_exact: i64,
    break_penalty: i64,
    line_penalty: i64,
    short_score: i64,
    emergency: bool,
    overfull: bool,
    cost: i64,
}

struct KpTrace {
    lines: Vec<LineTrace>,
    evaluated_candidates: u64,
    fallback_used: bool,
    fallback_stage: FallbackStage,
    breaker_used: BreakerKind,
    tolerance: i32,
}
```

用途：

- demo 里展示每行代价构成；
- 调参时看哪一项主导；
- 回归时比较两个权重配置；
- `boundary_deviation_exact` 与 cost 用的近似值对比，监控「比例分配假设」是否失效；该字段只在 trace 模式计算，不进入 canonical。
- 不序列化进 `LayoutResult`，避免 canonical 结果膨胀。

---

## 5. 验证

### 5.1 暴力枚举参考解

- 对 `n ≤ 16` 的 fixture，枚举所有合法断点组合（遵守 Forbidden/Forced）。
- 用**同一个** `line_cost` 计算总代价，取最小值。
- 断言 DP 的 `total_cost` 等于暴力最优。
- 只比较总代价，不强制比较断点序列（tie 时允许多个最优解）。
- 这是 F4 的「参考实现」，不依赖 TeX/Typst。

### 5.2 不变量

- `K-P ≤ Greedy-C` 只在 `breaker_used == Optimal` 时断言；`length_limited` 时结果本身就是 Greedy-C。
- 除 fallback 外，没有 Forbidden 断点。
- `fallback_stage == Normal` 时：`emergency_lines == 0`、`forbidden_violations == 0`。
- 行覆盖完整：`line[0].start == 0`，`line[k].end == line[k+1].start`，`last.end == n`。
- 非末行、无裁剪时：`natural + adjustment == target`（或 `natural - trim + adjustment == target`）。
- 末行 `adjustment == 0`；强制段尾 `adjustment == 0`，且不触发短末行惩罚。
- `FallbackStage::Normal` 下，非末行的最终行宽等于 target；`Line.overfull` 只表示超过名义 shrink，不等于超过 target。
- `overfull == true` 当且仅当裁剪后 `E' > H`（末行是 `E' > 0`）；`0 < E' <= H` 但 `|r| > tol` 只记 `emergency`，不记 `overfull`。
- `S == 0 && D > 0` 不得产生 `r = 0` 的「完美行」；正常阶段必须不可行，fallback 必须有 `emergency`。
- `max_kp_atoms = Some(k)` 且 `n > k` 时：`breaker_used == GreedyC`、`length_limited == true`；算法选择只依赖输入和参数，不依赖时钟。
- `Line.cjk_latin_adjustments` 与 `Line.adjustment` / `Line.boundary_deviation` 自洽；eval 可用它重算精确 D。
- 基准必须报告 `length_limited` 占比；质量统计按 `breaker_used=Optimal` 子集单列。
- 长英文词超宽走 `Emergency` 阶段，并带 `Line.overfull = true`；`FallbackStage::Overfull` 仍保留未启用。不计入英文拆断数。
- 同输入、同 `ParamSet` hash、同度量 → 同 canonical 字节。
- 改一个 Tier A 参数 → `paramHash` 改变；非法参数在布局前报错。
- native 与 `wasm32-wasip1` → 同哈希。
- `overfull == false` 且 `|r| <= tol` 时，非末行的最终行宽 `effective_width + adjustment == target`；`effective_width` 本身可以大于 target，由 shrink 吸收。
- 行高方差 / 最大行高：从 `Line.height` 统计；MVP 只报告，不作为通过条件。
- `line_height_jump_count`：默认 target；`height_scale > 0` 时升为 guard。
- `boundary_deviation` 的 cost 近似 vs eval 真实值：P95 相对误差 >20% 警告，>50% 失败（仅在两者都非零时计）。
- `Align != Justify` 返回 `Unsupported`，不得静默降级。
- `RuleStatus::Tbd` 的规则不得产生 `Forbidden` / `Forced`；硬约束必须有 `Verified` 状态。

### 5.3 Golden / fixtures

| fixture | 覆盖 |
|---|---|
| `pure-cjk` | 纯中文逐字断行 |
| `cjk-punct` | 行首/行末标点禁断 |
| `mixed-space` | 中英空格边界 |
| `mixed-nospace` | 中英自动间距 |
| `long-latin` | 超长不可断词：允许 `Overfull`，不计英文拆断 |
| `cjk-latin-adjustments` | 逐边界实际分配与 `boundary_deviation` 近似值自洽 |
| `length-limited-policy` | `n > max_kp_atoms` → GreedyC + `length_limited` |
| `opening-closing` | 开/关标点禁断 |
| `styled-mixed` | 加粗/斜体 CJK+Latin；词内样式切换不拆词 |
| `large-inline` | 2em 行首大字；行盒变高 |
| `super-sub` | 上下标 baseline shift；与基字禁断 |
| `inline-code` | 行内代码不可断 |
| `edge-trim` | 行尾标点裁剪 |
| `underfull-no-stretch` | `S == 0 && D > 0`：正常不可行，fallback 不把不足当完美 |
| `overfull-no-shrink` | `H == 0 && E' > 0`：fallback 标 `overfull + emergency` |
| `overfull-within-tolerance` | `E' > H` 但 `|r| <= tol`：只标 `overfull`，不误标 `emergency` |
| `boundary-deviation-mismatch` | space 主导 vs CjkLatin 主导：cost D 与 eval 真实值差异可解释 |
| `height-jump` | 相邻行高度跳变计数；`height_scale > 0` 时升为 guard |
| `tbd-rule-soft` | `Tbd` 规则只产生 penalty/trim，不产生硬禁断 |
| `align-left-unsupported` | 非 Justify 返回 `Unsupported` |
| `schema-unknown-fields` | reader 忽略未知字段；不同 schema 不比较字节 |
| `forced-break` | 强制换行段切分 |
| `tie` | 相同代价的 tie-break |
| `narrow` | 极窄行宽/退化路径 |

每个 fixture 带手算或半手算的期望行数、断点、cost 范围；不把易变的浮点值写进 golden。

### 5.4 Fuzz / property

- 随机 atom 宽度、glue、stretch/shrink、Breakability、PunctuationClass。
- 断言不 panic、行覆盖完整、确定性、K-P ≤ Greedy-C。
- 随机生成长度 0/1/2 的退化段落。
- 随机宽度极窄/极宽，触发 fallback。
- 建议用 `proptest` 作为 dev-dependency；也可以先用固定种子的手写生成器。

### 5.5 跨目标

- `cargo test` native。
- `cargo check -p soma-core --target wasm32-unknown-unknown`。
- 同一 fixture 在 native 和 `wasm32-wasip1` 下输出同一 canonical 哈希。
- 记录 Rust / 规则 / 度量表版本。

### 5.6 基准

- 场景：100 / 500 / 1,000 / 2,000 / 4,000 atoms；纯中文、混排、标点密集。
- 记录：行评估次数、耗时中位数/P95、内存峰值、`fallback_stage` 分布、`emergency_lines`、`overfull_lines`、`breaker_used`、`length_limited`。
- **报告 `length_limited` 占比：** 总量、按 atom 桶、关键质量样本分别报告；关键样本 >20% 时，质量结论只能覆盖 `Optimal` 子集。
- **K1 就用真实骨架测 n=2,000 / n=4,000**，不用 Typst 或估算代理。
- 暂定预算：native P95 `n=2,000 <= 20ms`、`n=4,000 <= 80ms`；wasm P95 `n=2,000 <= 80ms`、`n=4,000 <= 300ms`。这是工程预算，不是模型正确性条件。
- 如果 n=4,000 超预算：按 4.4 的确定性策略把 `default-v1.max_kp_atoms` 设为 2,000，超过的段落走 Greedy-C 并标 `length_limited`；不引入时间型 fallback，也不把未验证的剪枝写进排期。
- K6 再做常数优化/剪枝，之后回写预算。

### 5.7 盲测统计（技术验收与质量宣称分开）

- **预实验：** 5–8 名评审 × 8–10 段，先测 Fleiss' κ；κ<0.2 时先统一判定示例和评审口径，再进入正式盲测。预实验样本不计入 30 段正式集。
- **样本：** 30 段冻结混排（调参集 10 段单独隔离）；分层覆盖纯中文、混排、标点密集、长英文、窄栏、样式、强制换行。
- **评审：** 5 名（最低 5；3 名只能标为探索性）；每段 A/B 随机、左右位置平衡、隐去算法来源。
- **通过口径：** 每段多数票（5 人 ≥3）为胜；报告胜率 + Wilson 95% 置信区间 + 评审间一致性（Fleiss' κ）。
- **Gate Q：** 点估计 ≥60%，且 95% CI 下界 >50%，且 κ ≥0.2，且无「全体一致选贪心」。若 CI 跨 50%，预注册扩展到 60 段再判定；仍跨 50% 记 **Inconclusive**，不记失败，也不宣称通过。
- **Gate Q 只对 `Verified` 规则做 guard 断言；`Tbd` 只报告。** `length_limited` 段落单列统计，不能用 Greedy-C 兜底结果支撑 K-P 质量。
- **统计诚实：** 30 段只能可靠发现较大偏好（约 ≥70% 胜率）；它不足以「证明 60%」。60% 是产品阈值，不是统计证明；需要证明 60% 时，Phase 2 扩到 100+ 段或改成 Bradley-Terry/Thurstone 模型并报告置信区间。
- 「无全体一致选贪心」是必要条件，不是充分条件；不能替代 CI 和 κ。
- 盲测段落不参与调参；评审理由可以回写权重，但不能针对具体段落加规则。
- **Inconclusive 不阻塞技术交付（Gate T），只影响质量宣称。**

---

## 6. 里程碑

| 阶段 | 交付物 | 退出条件 |
|---|---|---|
| K0 | 本提案 + 参数分层 + **规则状态盘点表**；决策拍板 | 模型无未决歧义；`Verified/Tbd/ParamOnly` 数量与 golden 覆盖已记录 |
| K1 | `params` 模块 + `default-v1` preset + 校验/hash；暴力枚举 + fixtures；修正 badness 口径与 tie-break；n=2,000/4,000 真实骨架基准 + 长度策略拍板 | 参数可加载；DP 在 n≤16 上与暴力最优一致；性能数字、`length_limited` 占比和 `max_kp_atoms` 决定已记录 |
| K2 | 代价模型 v1：`line`、`break_penalty`、`boundary_deviation`、`short_score`、`cjk_latin_adjustments`、trace | 每行可解释；K-P ≤ Greedy-C；fixtures 通过 |
| K3 | 样式与行高：`GlyphMetrics`、`Line.ascent/descent/height`、2em 行内大字、词内样式禁断 | styled/large/super-sub 案例通过 |
| K4 | 行边裁剪 + 渲染坐标约定 + `Line.trim_*` | 裁剪案例通过；行宽对得上 |
| K5 | 强制换行/segment 切分 | Markdown hard break 案例通过 |
| K6 | 性能优化；Tier A 初标定 + 敏感度报告；可选保守 active-node 剪枝 + 差分测试 | n=2,000 在预算内；参数报告完成 |
| K7 | 可选 fitness/rhythm DP 状态 | 仅在评估证明需要时启动；不在基线 |
| K8 | 接入 rules/eval/demo，更新 canonical schema；盲测统计 | 回归、盲测、文档完成 |

**冻结基线（2026-11-20）：** K0–K5 约 15 天；K6 5 天；K8 3 天；盲测预实验/正式盲测/评估/文档 10 天；缓冲 7 天；合计 40 个工作日（8 周全职）。兼职 18–20 周。K7 不在基线内。执行中不重新估算；范围变化写变更记录，按 K7 → fitness → 剪枝 → demo 打磨的顺序砍，Gate T 不砍。

**Gate T（技术验收）与 Gate Q（质量宣称）分开：** 盲测 Inconclusive 不阻塞技术交付；只有 Gate T 失败才阻塞 K1–K8。

---

## 7. 需要拍板的决策

| 决策 | 建议 | 理由 |
|---|---|---|
| 代价结构 | **加法加权**，不用 TeX 平方 demerits | F6 独立权重、可标定、可解释 |
| 挤压是否进 MVP | **是**，K4 | PRD F6/F19 要求；不做会低估中文质量 |
| 强制换行 | **是**，K5 | Markdown hard break 常见；用 Forced 而非负 penalty |
| 相邻行 fitness | **MVP 不做**，K7 可选 | 状态 ×4、调参复杂；先看评估数据 |
| 加粗 / 斜体 / 代码 | 通过 `StyleId` + `Metrics`；样式切换不产生断点 | 断行只看宽度；样式是度量问题 |
| 词内样式切换 | 边界 `Forbidden` | 不能因为加粗把英文词拆开 |
| 行内大字号 | **支持**；行盒变高，`height_scale` 默认 0 | 只影响宽度/行高，不需要额外 DP 状态 |
| 上标 / 下标 | `baseline_shift` + 与基字禁断 | 保持 `x²` 附着；行盒正确 |
| 真正的首字下沉 | **MVP 不做**，阶段 3/4 | 需要变行宽 + 浮动/绕排；PRD 非目标 |
| 行高进入代价 | **默认不进入**，先做指标 | 大字号可能是作者意图；先用方差评估 |
| penalty 符号 | **非负** | 强制换行用 Forced；避免负代价和剪枝歧义 |
| 每行固定代价 | **是**，`w_line` | 抑制无意义的多行；让 exact line 也有成本 |
| badness 口径 | `badness_scale × abs(r)^badness_exponent`；default-v1 为 100/3 | 变量化；K2 统一到该口径 |
| 边界偏差 | **平均绝对偏差** + `Line.cjk_latin_adjustments` 逐边界实际量 | O(1) 前缀和 + O(n) 校验；避免平方溢出 |
| tie-break | 代价 → 行数 → 末行起点更小 | 局部确定、实现简单；全局字典序以后再说 |
| active-node 剪枝 | **K6 之前不做** | 边裁剪破坏单调性；先保证正确 |
| 强制段尾 | 不拉伸、不触发短末行 | 有意短行；下一段不重新缩进 |
| canonical schema | K2=`/2`（`cjkLatinAdjustments`）、K3=`/3`（行盒）、K4=`/4`（trim）、K5=`/5`（forcedEnd）、K6=`/6`（结果 flags） | 新增字段改变字节；同 schema 才比较字节 |
| 默认 tolerance | default-v1 取 3000；它是硬上限不是首选比例；K1/K6 做 `(tol, scale)` 扫描 | 中文 glue 较小；太低会频繁 fallback；接近 tol 会被 badness 惩罚 |
| `r` 的语义 | 相对声明 stretch/shrink 的比例；`|r|>1` 是超出名义量，不是行宽比例 | 避免把 TeX 的 badness 阈值和 ratio 混为一谈 |
| `S=0` / `H=0` | 正常阶段不可行；fallback 用 `BADNESS_RATIO_CLAMP` 哨兵，不把不足当完美 | 防止「无边界可伸缩」被 `r=0` 免费放过 |
| `overfull` | 裁剪后 `E' > H`；`E' <= H` 但超 tolerance 只记 emergency | 与 Greedy-F 的标记语义一致，避免误报 |
| 末行/强制段尾 | 不拉伸、`adjustment=0`；裁剪后仍超宽才 fallback | 有意短行不应被 justify 惩罚 |
| 参数分层 | 结构/规则/代价/样式/段落/评估六层 | 避免参数爆炸和职责混乱 |
| 默认值 | **preset `default-v1`**，不是代码常量 | 可整体替换、可标定、可复现 |
| 代价权重作用域 | **按请求/文档**，不按段落 | 段落只改几何；保证段落可比 |
| 规则参数作用域 | `RuleSet`，编译进 `ParaIR` | breaker 不认识中文规则 |
| 标定范围 | 约 8 个 Tier A；Tier B preset；Tier C 不动 | 控制自由度，防过拟合 |
| 版本与哈希 | `modelVersion` / `preset` / `rulesVersion` / `metricsVersion` / `paramHash` | 可复现 |
| 评估参数 | 独立 `EvalParams` | 改版式不能改成功标准 |
| 盲测统计 | 预实验 5–8 人测 κ；30 段 × 5 评审；Wilson CI + Fleiss' κ；CI 跨 50% 扩到 60；仍跨记 Inconclusive | 10 段/60% 不可证伪；不把产品阈值当统计证明 |
| 性能降级 | `max_kp_atoms` + Greedy-C + `breaker_used`；禁止时间型 fallback | 同一输入同结果；不承诺未验证的剪枝 |
| 最大段落规模 | K1 实测 n=2,000/4,000；超预算则 `max_kp_atoms=2,000`；报告 `length_limited` 占比 | 先有数字再承诺；4,000 不是硬保证；质量按 Optimal 子集单列 |
| `LayoutResult` 阶段 | `fallback_stage` / `emergency_lines` / `forbidden_violations` / `breaker_used` / `length_limited` | 跨阶段总代价不可比；评估需分组 |
| 非 Justify | 枚举保留完整；MVP 只实现 Justify，其余 Unsupported | 避免后续加 Left/Center 的破坏性 schema 变更 |
| 规则状态 | `Verified / Tbd / ParamOnly`；Tbd 只能是软规则 | 未核实规则不得污染硬约束和 golden |
| 规则盘点 | K0 交付 `ruleId × source × status × golden` 表 | Gate Q 只对 Verified 断言，Tbd 只报告 |
| 边界分配 | `Line.cjk_latin_adjustments` 输出逐边界实际分配 | 渲染器/eval 可校验比例分配假设 |
| 门禁分层 | **Gate T 技术验收 / Gate Q 质量宣称 / Gate D 交付** | Inconclusive 不阻塞技术交付 |
| 排期 | **冻结 8 周全职；兼职 18–20 周** | 不执行中重估；变更需记录 |
| 长英文词 | 不可断；允许 `Overfull`，单列 `overfull_lines` | 避免英文拆断 guard 与 fallback 冲突 |
| 跨段节奏 | `short_last` 是段落级；不期待跨段效果 | 不把范围外效果写进 Gate Q |
| canonical 兼容 | 同 schema 才比较字节；reader 忽略未知字段 | 允许增量演进，不假装跨 schema 字节一致 |
| 行高跳变 | 默认 target；`height_scale>0` 时 guard；`height_scale=0` 不计入 ≥3 项 | 默认不压扁作者大字，但启用行高代价后要负责 |
| 量纲归一化 | 每项除以 `EM` / `ideal_gap` / `PENALTY_REF` / `body_height` | 权重可解释；避免标定互相吸收 |

---

## 8. 与 PRD 的映射

| PRD | 本模型 |
|---|---|
| F1 CommonMark AST + 标点语义 | `soma-md` + rules；样式 run 转 `StyleId` |
| F2 IR 与语法解耦 | `DocIR` / `ParaIR`；`Atom.style` + `Boundary.role` |
| F4 K-P 全局断行 | 2.7 的 DP + 5.1 暴力参考 |
| F5 贪心基线 | 4.6，共用 `line_cost` |
| F6 中文代价模型 | 2.6 + 3.3–3.5，各权重独立 |
| F17 断行单元 | 3.1，CJK atom / Latin run / Digit run |
| F18 中英边界 | 3.5 + 2.6.2 |
| F19 混排标点优先级 | 3.3 + 3.4，规则层给 class/trim/penalty |
| 样式 / 字号 / 行高 | 2.9 + 3.9；`GlyphMetrics` + `Line.ascent/descent/height` |
| 行首大字 | 3.10；行内大字号支持，真正首字下沉非 MVP |
| 参数化 / 可复现 | 2.10 + [`params.md`](params.md)；preset / hash / 版本 |
| F8 确定性序列化 | 2.8 + canonical schema |
| F9 质量评估 | 5.3/5.4，从 `LayoutResult` 计算 |
| F12 跨运行时 | 5.5，native vs wasm32-wasip1 哈希 |

---

## 附录 A：核心 API 草案

> `ParagraphStyle.line_height` 在 K3 之后解释为**最小正文行高**；实际行高由 `Line.height` 输出。
> `GlyphMetrics` / `Metrics::glyph_metrics` 见 2.9.1。
> `BreakParams` / `CostParams` / `RuleSet` 的完整定义见 [`params.md`](params.md)。

```rust
pub struct BreakConfig {
    params: ParamSet,   // Tier A + feature flags + version tags
    window: Window,
    param_hash: ParamHash,
}

impl BreakConfig {
    pub fn new(params: ParamSet, window: Window) -> Self;
    pub fn from_params(params: ParamSet) -> Self;
    pub fn params(&self) -> &ParamSet;
    pub fn window(&self) -> Window;
    pub fn param_hash(&self) -> ParamHash;
    pub fn rules_version(&self) -> u16;
    pub fn metrics_version(&self) -> u16;
}
```

pub struct Line {
    pub start: u32,
    pub end: u32,
    pub indent: Width,
    pub target_width: Width,
    pub natural_width: Width,
    pub trim_left: Width,
    pub trim_right: Width,
    pub adjustment: i32,
    pub ratio_milli: i32,
    pub ascent: Width,
    pub descent: Width,
    pub leading: Width,
    pub height: Width,
    /// 按比例分配假设聚合的平均绝对偏差；eval 用 cjk_latin_adjustments 精确校验。
    pub boundary_deviation: i64,
    /// 仅 CjkLatin 边界的实际分配量；总 O(n)，不进 DP 热路径。
    pub cjk_latin_adjustments: Vec<CjkLatinAdjust>,
    pub cost: Cost,
    /// 裁剪后 `E' > H`；末行是裁剪后 `E' > 0`。
    pub overfull: bool,
    pub emergency: bool,
    pub forced_end: bool,
    pub boxes: Vec<AtomBox>,
}

pub struct CjkLatinAdjust {
    pub boundary: u32,
    pub adjustment: i32,
}

pub enum FallbackStage {
    Normal,
    Emergency,
    ForbiddenRescue,
    Overfull,
}

pub enum BreakerKind {
    Optimal,
    GreedyC,
    GreedyF,
}

pub struct LayoutResult {
    pub lines: Vec<Line>,
    /// 只在同一 `(modelVersion, preset, rulesVersion, metricsVersion, fallback_stage)` 下可比。
    pub total_cost: Cost,
    pub breaker_used: BreakerKind,
    pub fallback_stage: FallbackStage,
    pub length_limited: bool,
    pub emergency_lines: u32,
    pub forbidden_violations: u32,
    pub rules_version: u16,
    pub metrics_version: u16,
}

pub struct Boundary {
    pub breakability: Breakability,  // Allowed | Forbidden | Forced
    pub kind: BoundaryKind,          // Normal | Space
    pub role: BoundaryRole,          // Plain | CjkLatin
    pub glue: Glue,
    pub penalty: u32,                // 非负
}
```

## 附录 B：`default-v1` preset（起点，非结论）

这些是 `default-v1` 的取值；完整参数表见 [`params.md`](params.md) §10。

| 参数 | 起点 | 说明 |
|---|---|---|
| `line` | 10 | 每行固定 |
| `badness_scale` | 100 | 配合 `abs(r)^badness_exponent`；`|r|=1` 时 cost=100 |
| `badness_exponent` | 3 | 默认 3；高级参数 |
| `break_penalty` | 100 | `penalty=100` 时 cost=100（10 行） |
| `compress_amount` | 20 | 每 1 em trim；0.25em = 5 |
| `compress_count` | 5 | 每次裁剪；0.25em 事件合计约 10（1 行） |
| `boundary_deviation` | 50 | 每 1 个 ideal gap 的平均偏差；满 gap 偏差 = 5 行 |
| `short_last_scale` | 1_000_000 | `short_score` 0..1e6 |
| `short_last_threshold` | 2em | 低于此值计费 |
| `height_scale` | 0 | 行高代价；默认关闭 |
| `height_threshold` | none → body height | 默认取正文行盒 |
| `max_kp_atoms` | None | K6 实测 4,000 atoms 在预算内，保留 `None`；WASM 实测超预算再设 2,000 |
| `emergency` | 10_000_000 | 每行 |
| `forbidden` | 100_000_000 | rescue 断点（可选） |
| `fitness` | 0 | K7 才启用 |

这些值按**归一化公式**重新推导，不与旧版「每 1/4096 em」的数值等比换算。设计基准：一行 = 10；一次 0.25em trim + 事件约 1 行；一个 ideal gap 的平均偏差 = 5 行；`|r|=1` 的 badness = 10 行。这些只是让 K1–K5 有可跑的比较基线；最终权重在调参集上标定，盲测集不参与。改默认值要换 preset id，不要改算法常量。

## 附录 C：测试矩阵

| 测试 | 类型 | 断言 |
|---|---|---|
| `brute_force_matches_dp` | 单元 | 总代价等于暴力最优 |
| `kp_not_worse_than_greedy_c` | 单元 | `total_cost <=` |
| `forbidden_break_not_chosen` | 单元 | 断点集合不含禁断 |
| `forced_break_split` | 单元 | 段尾/段首正确 |
| `forced_segment_no_indent` | 单元 | forced 后段首不重新缩进 |
| `final_line_penalty_zero` | 单元 | j=n 不越界读 b_{n-1} |
| `line_trim_matches_target` | 单元 | 裁剪后右边缘 = target |
| `single_atom_double_trim` | 单元 | 单原子双侧 trim 坐标/右边缘正确 |
| `double_trim_still_overfull_fallback` | 单元 | 双侧 trim 后仍超宽 → fallback |
| `breaker_ignores_punctuation_class` | 单元 | 只读 trim 数值，不认标点类别 |
| `pruning_matches_brute_force` | 单元 | 宽度窗口不改变最优代价（含变宽/变 glue） |
| `pruning_reduces_candidate_count` | 单元 | 窗口实际减少评估数 |
| `forbidden_rescue_is_last_resort` | 单元 | Normal/Emergency 不可行时才用 ForbiddenRescue，并计 violation |
| `boundary_deviation_cost_vs_actual` | 单元 | cost 近似 vs eval 真实方差差异可解释 |
| `underfull_no_stretch_not_free` | 单元 | `S==0 && D>0` 正常不可行，fallback 有 emergency |
| `overfull_no_shrink_flagged` | 单元 | `H==0 && E'>0` 标 overfull + emergency |
| `overfull_within_tolerance_not_emergency` | 单元 | `E'>H` 但 `|r|<=tol` 只标 overfull |
| `final_line_trim_then_fallback` | 单元 | 末行先 trim，裁剪后仍超宽才 fallback |
| `fallback_stage_fields_consistent` | 单元 | stage/emergency_lines/violations 与行 flag 一致 |
| `length_limited_uses_greedy_c` | 单元 | 超 `max_kp_atoms` 用 Greedy-C 且标 length_limited |
| `boundary_deviation_p95_guard` | 单元 | 相对误差 >20% 警告、>50% 失败 |
| `cjk_latin_adjustments_match_deviation` | 单元 | 逐边界实际分配与近似 D 自洽 |
| `long_latin_overfull_not_split` | 单元 | 长英文词走 Emergency + `overfull` 标记，拆断数仍为 0 |
| `length_limited_share_reported` | 单元 | 基准报告 length_limited 占比 |
| `pilot_kappa_measured` | 评估 | 预实验 κ 进入报告；κ<0.2 触发口径校准 |
| `rule_inventory_complete` | 评估 | K0 盘点表覆盖所有规则 |
| `unknown_fields_ignored` | 单元 | reader 忽略未知字段；跨 schema 不比字节 |
| `height_jump_metric` | 单元 | 相邻行高度跳变计数；height_scale>0 时 guard |
| `tbd_rule_not_hard` | 单元 | Tbd 规则不产生 Forbidden/Forced |
| `align_non_justify_unsupported` | 单元 | Left/Center/Right 返回 Unsupported |
| `badness_scale_ge_line_preset` | 单元 | default-v1 满足 `badness_scale >= line` |
| `last_line_counts_line_cost` | 单元 | 末行也计 w_line |
| `tie_break_brute_force` | 单元 | tie 场景与暴力参考一致 |
| `single_atom_line_adjustment_zero` | 单元 | 单原子行不分配 |
| `style_split_word_no_break` | 单元 | 词内加粗切换不产生断点 |
| `large_inline_line_height` | 单元 | 2em 行首字导致行盒变高 |
| `super_sub_baseline` | 单元 | 上下标 shift 进入行盒 |
| `inline_code_unbreakable` | 单元 | 代码对象不被拆开 |
| `glyph_metrics_line_box` | 单元 | ascent/descent/height 与度量一致 |
| `param_validation_rejects_bad_values` | 单元 | 非法参数报错 |
| `preset_hash_stable` | 单元 | 同 preset 同 hash |
| `override_changes_only_expected_field` | 单元 | 覆盖只影响目标字段 |
| `tie_break_is_stable` | 单元 | 重复运行同结果 |
| `canonical_bytes_stable` | 单元 | 同输入同字节 |
| `fuzz_no_panic` | 属性 | 不 panic、行覆盖完整 |
| `wasm_hash_matches_native` | 集成 | 同哈希 |
| `bench_2k_atoms` | 基准 | 记录中位数/P95 |
| `bench_4k_atoms` | 基准 | 记录中位数/P95、length_limited 占比 |
