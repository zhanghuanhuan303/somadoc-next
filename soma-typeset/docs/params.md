# 参数化模型（ParamSet）提案

**状态：** 提案，配合 [`kp-model.md`](kp-model.md) 使用
**目标：** 把 K-P 模型里的「魔法数字」变成有名字、有单位、有范围、有作用域、可版本化的变量。默认值只是 `preset` 数据，不是算法里的常量。

---

## 0. 原则

1. **不是所有东西都该可调。** 结构常量、规则参数、代价权重、样式度量分开；否则参数爆炸，无法标定，也无法复现。
2. **默认值是 preset，不是硬编码。** 算法只读 `BreakParams`；`default-v1` 是一组数据，可以整体替换。
3. **单位统一。** 宽度 = 1/4096 base em；比例 = permille（千分比）；代价 = `i64`；核心不出现浮点。
4. **作用域明确。** 模型/规则/样式/段落/原子各自只拥有该层的参数。
5. **可校验、可版本化、可哈希。** 非法参数在布局前报错；结果携带 preset / rules / metrics / param hash。
6. **标定要克制。** 只标定少量 Tier A 参数；Tier B 用规则预设；Tier C 固定。
7. **评估参数独立。** 盲测样本数、胜率阈值属于评估协议，不属于排版模型；改版式不能顺便改成功标准。

---

## 1. 五层参数

| 层 | 名称 | 配置者 | 是否进入 breaker | 版本 |
|---|---|---|---|---|
| C | 结构常量 | 代码/模型版本 | 是（隐式） | `MODEL_VERSION` |
| B | 规则参数 | 规则表 / 排版规范 | 否；编译进 `Atom`/`Boundary` | `RULES_VERSION` |
| A | 代价/布局参数 | 文档/请求/preset | 是 | `PARAM_HASH` |
| S | 样式/度量参数 | 字体表 / 宿主 | 通过 `Metrics` | `METRICS_VERSION` |
| P | 段落参数 | 调用方 | 通过 `ParagraphStyle` | 随请求 |
| E | 评估参数 | 评估协议 | 否 | `EVAL_VERSION` |

### 1.1 Tier C：结构常量（不可配置）

这些不是「调参」，而是模型定义的一部分；改变它们就是新模型版本：

- `UNITS_PER_EM = 4096`。
- `BADNESS_RATIO_CLAMP = 100_000`；badness 归一化分母 `1000^p`（只随 exponent 变）。
- `PENALTY_REF = 100`；规则层 penalty 的归一化上界。
- `ideal_gap == 0` 时，`boundary_deviation` 归一化退化为 `EM`。
- DP 递推、断点定义、断点处 glue 丢弃。
- 整数除法截断、余数分配顺序。
- canonical schema、tie-break 的层数。
- `Line` / `LayoutResult` 的字段语义。

### 1.2 Tier B：规则参数（RuleSet）

由规则表/排版规范决定，在构建 `ParaIR` 时解析成具体宽度和标志：

- CJK 间距、CJK-Latin 间距、空格 stretch/shrink。
- 标点类别、禁断/强制表、断点 penalty。
- 每类标点的 trim 上限、连续标点叠加规则。
- 规则来源（`clreq` / GB / 出版社实践）、`ruleId` 和 `status: Verified | Tbd | ParamOnly`。

**breaker 不认识这些参数**，只看到最终的 `Atom`/`Boundary`。这条边界很重要：中文规则变化不需要改 DP。

### 1.3 Tier A：代价/布局参数（BreakParams）

每次布局请求传入；可以由 preset + 覆盖得到：

- `tolerance`、`emergency_stretch`、`max_lines`。
- `max_kp_atoms`：确定性长度策略；超过后改用 Greedy-C，结果标 `breaker_used/length_limited`。不是超时。
- 代价权重、阈值、feature flags、fallback 策略、trim 优先级。

这是唯一需要标定的层；控制在约 10–15 个字段，其中约 8 个实际标定。

### 1.4 Tier S：样式/度量参数（Metrics）

由字体表或宿主提供：

- 每个 `StyleId` 的 advance 表。
- ascent / descent / leading / baseline_shift / overhang（overhang 只用于视觉，不进入断行）。
- space width、line-height 最小值。
- 粗体/斜体/代码/字号都映射到不同 `StyleId`。

这些来自字体和设计，不是 K-P 标定参数；用 `METRICS_VERSION` 锁定。

### 1.5 Tier P：段落参数（ParagraphStyle）

每个段落自己的几何：

- `width`、`first_indent`、`align`。
- `line_height_min`（最小正文行高）。
- 段落级 feature override（如 `allow_edge_trim`）只在明确需要时开放。
- **MVP 行为只支持 `align = Justify`；枚举保留 `Justify/Left/Center/Right`。** 其余值返回 `Unsupported`；不要把非 justify 混进 `T` 的语义。

### 1.6 Tier E：评估参数（EvalParams）

- 盲测样本数、评审人数、胜率阈值、κ 下限、是否禁止「一致选贪心」。
- 指标定义、样本分层、回归阈值、`boundary_deviation` 告警/失败阈值。

**不放进 `ParamSet`**，避免用调参改成功标准。

---

## 2. 单位与 base em

- `base_em = 4096 units` 是结构常量。
- 所有 `Width` 都是 **base em** 的单位，不是 glyph 自己的 em。
- 一个 2em 的大字返回 `8192`；一个 0.5em 的空格返回 `2048`。
- 物理字号（pt）由宿主决定；核心只认 base em。
- 比例参数用 permille：`ratio_to_width(r) = r * 4096 / 1000`。
- 这样同一套比例可以跨字号复用；大字不需要特殊分支。

```rust
pub struct Width(pub i32);       // 1/4096 base em
pub struct Permille(pub u16);    // 0..=10000，表示 0.000..=10.000
pub struct Weight(pub u32);      // 非负代价权重
```

---

## 3. 参数树

```rust
pub struct ParamSet {
    pub preset: PresetId,          // "default-v1" 等
    pub model_version: u16,        // Tier C
    pub rules_version: u16,        // Tier B
    pub metrics_version: u16,      // Tier S
    pub break_params: BreakParams, // Tier A
    pub features: FeatureFlags,    // Tier A
}

pub struct BreakParams {
    pub tolerance: Permille,
    pub emergency_stretch: Width,
    pub emergency_tolerance: Permille,
    pub max_lines: Option<u32>,
    pub max_kp_atoms: Option<u32>,
    pub trim_priority: TrimPriority,
    pub tie_break: TieBreak,
    pub costs: CostParams,
    pub fallback: FallbackParams,
}

pub struct CostParams {
    pub line: Weight,
    pub badness_scale: Weight,
    pub badness_exponent: u8,          // 默认 3
    pub break_penalty: Weight,
    pub compress_amount: Weight,
    pub compress_count: Weight,
    pub boundary_deviation: Weight,
    pub short_last_scale: Weight,
    pub short_last_threshold: Width,
    pub height_scale: Weight,
    /// None 表示使用当前段落的最小正文行高。
    pub height_threshold: Option<Width>,
    pub emergency: Weight,
    pub forbidden: Weight,
    pub fitness: Weight,               // 可选，默认 0
}

pub struct FallbackParams {
    pub allow_emergency: bool,
    pub allow_forbidden: bool,
    pub allow_overfull: bool,
    pub emergency_stretch: Width,
}

pub struct FeatureFlags {
    pub edge_trim: bool,
    pub boundary_deviation: bool,
    pub height_cost: bool,
    pub fitness: bool,
    pub forced_breaks: bool,
}

pub enum TrimPriority { EndFirst, StartFirst, Balanced }
pub enum TieBreak { CostLineStart }
```

`ParamSet` 不内联规则表和字体表；它只带版本号和 `BreakParams`。规则表是 `RuleSet`，字体表是 `Metrics`。这样 WASM 不必携带全部数据，也避免一份配置复制多份。

K-P 文档里的 `w_*` 简写与 `CostParams` 的对应关系：

| K-P 简写 | `CostParams` |
|---|---|
| `w_line` | `costs.line` |
| `w_badness` | `costs.badness_scale`（指数是 `badness_exponent`） |
| `w_break` | `costs.break_penalty` |
| `w_compress` | `costs.compress_amount` |
| `w_compress_count` | `costs.compress_count` |
| `w_boundary` | `costs.boundary_deviation` |
| `w_short` | `costs.short_last_scale` |
| `w_height` | `costs.height_scale` |
| `w_emergency` | `costs.emergency` |
| `w_forbidden` | `costs.forbidden` |
| `w_fitness` | `costs.fitness` |

规则参数不进入 `BreakParams`；它们在 `RuleSet` 里：

```rust
pub struct RuleSet {
    pub version: u16,
    pub cjk_gap: GapRatio,
    pub cjk_latin_gap: GapRatio,
    pub space_stretch: Permille,
    pub space_shrink: Permille,
    pub punct: PunctRuleTable,
    pub penalties: PenaltyTable,
}

/// 规则可信度。硬禁断/强制只能来自 Verified。
pub enum RuleStatus {
    Verified,
    Tbd,
    ParamOnly,
}

pub struct PunctRule {
    pub class: PunctClass,
    pub trim_left: Permille,
    pub trim_right: Permille,
    pub rule_id: u32,
    pub status: RuleStatus,
}

pub struct PenaltyRule {
    pub rule_id: u32,
    pub penalty: u32,
    pub status: RuleStatus,
}

pub struct GapRatio {
    pub width: Permille,
    pub stretch: Permille,
    pub shrink: Permille,
}
```

规则层把 `GapRatio` 解析成 `Boundary.glue`，把 `PunctRuleTable` 解析成 `Atom.compress_*` 和 `Boundary.penalty`。

---

## 4. 解析顺序与优先级

从最具体到最通用：

1. **Atom/Boundary 显式值**（规则层已解析；breaker 直接使用）。
2. **ParagraphStyle**（宽度、缩进、对齐、最小行高）。
3. **StyleId 的 Metrics**（advance、ascent/descent、space、trim scale）。
4. **RuleSet**（gap、trim、penalty、禁断/强制表）。
5. **BreakParams**（tolerance、权重、fallback、feature flags）。
6. **Preset 默认值**。

职责划分：

- **规则层**解析 1–4，输出 `ParaIR`。
- **breaker**只读 1 的最终值和 5。
- **Metrics** 由宿主实现；breaker 不解析字体。
- **Preset** 是数据；算法里没有 `if param == ...` 的业务分支（feature flags 除外）。

**不要**让 cost 权重按段落变化。代价权重属于一次布局请求/文档；段落只改几何。否则同一份文档的不同段落不可比较，调参也会过拟合。

---

## 5. 校验

所有参数在布局前校验；错误带参数路径：

```rust
pub struct ParamError {
    pub path: &'static str,   // "break_params.costs.badness_exponent"
    pub reason: &'static str,
}
```

建议范围（`default-v1` 只是取值，不是范围上限）：

| 参数 | 范围 |
|---|---|
| `tolerance` / `emergency_tolerance` | 0..=10000 permille |
| `badness_exponent` | 1..=5 |
| 所有权重 | 0..=1_000_000_000 |
| `emergency_stretch` | 0..=10em |
| `short_last_threshold` | 0..=8em |
| `height_threshold` | 0..=5em |
| `first_indent` | 0..=10em |
| `line_height_min` | 0.5em..=5em |
| `cjk_gap.*` / `cjk_latin_gap.*` | 0..=1000 permille |
| `space_stretch` / `space_shrink` | 0..=1000 permille |
| trim 比例 | 0..=1000 permille |
| `font_scale` | 0.25..=4.0（250..=4000 permille） |
| `max_lines` | 1..=10000 |
| `max_kp_atoms` | `None` 或 1..=10000 |

校验规则：

- 非负；整数；无 NaN。
- 权重求和与 `badness` 计算不溢出：`B_int` 用 `u128` 中间量，归一化分母是 `1000^p`，最后 clamp 到 `i64::MAX`。
- `r_milli` 用 `unsigned_abs` 或等价写法，避免 `i64::MIN.abs()` 溢出；再 clamp 到 `BADNESS_RATIO_CLAMP`。
- `BADNESS_RATIO_CLAMP = 100_000` 是结构常量；正常路径 `abs(r_milli) <= tolerance <= 10_000`，只有 emergency 才用到 clamp。
- `badness_scale` 建议范围 `0..=1_000_000`；`badness_exponent` 范围 `1..=5`。
- **preset 级检查：** 用于质量门禁的 preset 应满足 `badness_scale >= line`，保证 `|r|=1` 时 badness 至少等于一行。`default-v1` 为 `100 >= 10`；实验 preset 可以违反，但报告里必须标注。
- `trim_priority` / `tie_break` 是枚举，非法值报错。model v1 只实现 `TrimPriority::EndFirst`；当 `features.edge_trim == true` 时，`StartFirst` / `Balanced` 在校验时报错，不静默退化为 EndFirst（`edge_trim == false` 时忽略该字段）。
- `max_lines = 0` 报错；`max_kp_atoms = Some(0)` 报错。
- `line_height_min <= 0` 报错。
- `ParamSet` 通过校验后才计算 `PARAM_HASH`。

### 5.1 跨字段约束

- `emergency_tolerance >= tolerance`；否则 fallback 比正常还紧。
- `max_kp_atoms = Some(k)` 时 `k > 0`；`n > k` 走 Greedy-C，结果必须标 `breaker_used = GreedyC` / `length_limited = true`（K6 已实现）。
- `short_last_threshold == 0` 表示关闭短行项（末行与孤字行）；否则必须 `> 0`。
- `height_scale == 0` 时忽略 `height_threshold`；`height_scale > 0` 时阈值必须 `> 0` 或 `None`（取正文行高）。
- `features.edge_trim == false` 时忽略 `trim_priority` 和所有 trim 成本。
- `features.boundary_deviation == false` 时 `costs.boundary_deviation` 必须为 0 或不生效。
- `features.fitness == false` 时 `costs.fitness` 必须为 0 或不生效。
- 段落 `first_indent < width`；否则报 `InvalidParagraph`。
- 段落 `align != Justify` 报 `Unsupported`（不是 `ParamError`）；MVP 不实现 ragged/center/right。
- `fallback.allow_forbidden == true` 时必须同时 `fallback.allow_emergency == true`，保证 fallback ladder 有定义。K6 已实现 `Normal → Emergency → ForbiddenRescue`；`allow_overfull` 控制紧急阶段是否接受超宽行。
- `features.forced_breaks == false` 时 IR 中出现 `Forced` 边界应报错，而不是静默忽略（K5 返回 `BreakError::ForcedBreaksDisabled`）。
- **规则状态约束：** `RuleStatus::Tbd` 的规则不得产生 `Forbidden` / `Forced`；硬约束必须 `Verified`。该不变量由 `soma-rules` 的执行测试锁定（`hard_break_rules_are_verified`、`tbd_rules_are_never_hard_break_rules`）；`RuleStatus` 不进 `ParamSet`，所以不在 `ParamSet::validate` 里重复检查。

---

## 6. 版本与哈希

| 版本 | 变化时 |
|---|---|
| `MODEL_VERSION` | DP 结构、单位、舍入、tie-break 语义变化 |
| `RULES_VERSION` | 标点表、gap 比例、trim/penalty 规则变化 |
| `METRICS_VERSION` | 字体/样式度量表变化 |
| `PARAM_HASH` | `modelVersion` / `rulesVersion`、`BreakParams` 或 feature flags 的任何变化 |
| `PRESET_ID` | 默认值集合变化（`default-v1` → `default-v2`） |
| `EVAL_VERSION` | 盲测/指标协议变化 |

`LayoutResult` 携带：

```json
{
  "modelVersion": 6,
  "preset": "default-v1",
  "metricsVersion": 2,
  "rulesVersion": 3,
  "paramHash": "9f2c..."
}
```

- 同输入 + 同版本 + 同 hash → 同 canonical 字节。
- 只改权重也要换 `PARAM_HASH`；只改默认 preset 要换 `PRESET_ID`。
- 结构性变更不要偷偷塞进 `PARAM_HASH`；升 `MODEL_VERSION`。
- **兼容策略：** 只有 `schema` 相同才比较 canonical 字节；reader 忽略未知字段；新增字段/语义变化升 `schema`。旧 reader 可以宽松读取新 schema，但不能宣称字节一致。

---

## 7. Preset

Preset 是数据，不是算法分支：

- `default-v1`：冻结的 K-P 参考（`tolerance = 3000`）。
- `publish-v1`：**`soma.engine/1` 的默认 preset**（ADR-0023）：`default-v1` + `tolerance = 1500`，其余权重与回退完全相同；母语出版优先。
- `book-v1`：`publish-v1` + `short_last_threshold = 3em`（ADR-0106）。中文书籍里两字末行按孤尾计费；自 ADR-0112 起段中孤字行也按同一项计费。样书 `examples/book/book.engine` 使用它。
- `proposal-v1`：`book-v1` + `short_last_threshold = 8em`、`short_last_scale = 2_000_000`（ADR-0115）。提案的末行 5–8 个字也按孤尾计费，代价是 `book-v1` 的两倍；Studio 的内置默认（`examples/templates/proposal.engine`）使用它。
- `latex-v1`：LaTeX article 的断行口径（ADR-0069，**CLI 内置默认**）：`default-v1` + `tolerance = 1260`（`\tolerance=200` 换算）+ `short_last_scale = 0`（短行项关闭：末行与孤字行都不罚）；其余权重与回退不变。
- ~~更保守的 emergency~~：实测没有必要，回退阶梯保持 `default-v1`。
- `mobile-v1`：更低 `max_lines`、更少 feature（延迟优先）。
- `debug`：启用 trace、保留所有候选统计（不用于发布）。

加载方式：

```text
preset default-v1
set break_params.costs.badness_scale = 120
set break_params.tolerance = 2500
set features.height_cost = true
```

```rust
let mut params = preset::default_v1();
params.break_params.costs.badness_scale = Weight(120);
params.break_params.tolerance = Permille(2500);
params.features.height_cost = true;
```

实现建议：

- `soma-core` 内嵌 `preset::default_v1() -> ParamSet`，WASM 不需要文件系统。
- 也可以从 JSON/TOML 加载；解析后做同样校验。
- Demo 只暴露 4–6 个 Tier A 滑杆，不暴露规则表内部参数。

**实现状态：** `CostWeights` 已移除；`BreakConfig` 持有 `ParamSet`，默认值来自 `preset::default_v1()`，算法里不再保留数字默认值。

---

## 8. 标定

### 8.1 只标定 Tier A

- `default-v1` 给起点。
- 在**调参集**上标定约 8 个参数：`line`、`badness_scale`、`badness_exponent`（通常固定 3）、`break_penalty`、`compress_*`、`boundary_deviation`、`short_last_*`。
- `tolerance` 先按工程约束给候选，并参与 K1 的 `(tolerance, badness_scale)` 二维扫描；`emergency_*` 不标定。
- Tier B 的 gap/trim 比例先按 `clreq`/出版实践定；只有明确证据才按 style 微调。
- Tier C 不动。

### 8.2 影响层级（先标定高影响项）

| 层级 | 参数 | 说明 |
|---|---|---|
| 高 | `tolerance`、`badness_scale`、`badness_exponent`、`line` | 直接决定断点选择 |
| 高 | `compress_amount`、`compress_count`、`boundary_deviation` | 中文/混排质量的核心权衡 |
| 中 | `short_last_scale`、`short_last_threshold` | 孤字/末行 |
| 中 | `cjk_latin_gap.*`、`space_stretch/shrink` | 间距与齐行 |
| 低/实验 | `height_scale`、`fitness` | 默认关闭 |
| 固定 | `tie_break`、`allow_*`、fallback 阈值 | 工程/可复现 |

**参数交互（不能独立标）：**

- `tolerance` × `badness_scale`：ratio 接近 tolerance 时 badness 可能远大于 `line`；K1/K6 做二维扫描。
- `compress_amount` × `compress_count`：前者按量、后者按次数；两者会互相补偿。
- `boundary_deviation` × `cjk_latin_gap.*`：前者惩罚偏差，后者决定理想 gap 与可伸缩量。
- `line` × `short_last_*`：前者影响行数，后者影响末行长度。

### 8.3 方法

- 整数网格/坐标下降；不用连续浮点优化。
- 目标是 guard 指标 + 综合评分；盲测不参与调参。
- **固定起点：** `badness_exponent = 3`、`line = 10`；不在这两个维度上搜索。
- **第一步（粗网格）：** `tolerance ∈ {1500, 2000, 3000}` × `badness_scale ∈ {50, 100, 200}`；先过滤 guard 失败或 fallback 率过高的组合。
- **第二步（坐标下降，2 轮）：** 按顺序扫 `compress_amount ∈ {5, 10, 20, 40}`、`compress_count ∈ {0, 5, 10, 20}`、`boundary_deviation ∈ {0, 25, 50, 100}`、`short_last_scale ∈ {100_000, 300_000, 1_000_000}`、`short_last_threshold ∈ {1em, 1.5em, 2em}`、`break_penalty ∈ {0, 50, 100, 200}`。每个参数 5 档；每轮固定其他参数，选 guard 不退化且 target 综合分最好的档。
- **第三步（细化）：** 在最优档邻域缩小步长再扫一轮；仍用整数。
- **报告：** 每个参数的敏感度、几乎无影响的参数、所选 `ParamSet` + hash、调参集与冻结集结果；每项典型贡献量级。
- 限制自由度：不要给每个样本开 override，不要给每段单独调权重。

### 8.4 过拟合防线

- 10 段混排调参 + **30 段冻结盲测**（5 名评审；不是 10 段）。
- **预实验：** 5–8 名评审 × 8–10 段测 Fleiss' κ；κ<0.2 先统一判定示例和评审口径，再正式盲测。
- 盲测用 Wilson 95% CI + Fleiss' κ；点估计 ≥60% 且 CI 下界 >50% 且 κ ≥0.2 才算通过。CI 跨 50% 时预注册扩展到 60 段；仍跨 50% 记 **Inconclusive**。
- **Inconclusive 只影响 Gate Q 的质量宣称，不阻塞 Gate T 的技术交付。**
- 规则表来源可追溯；不能「看到样本就加一条规则」。
- 每次改 `ParamSet` 都生成新的 hash，并重跑基线。
- 盲测通过线不随参数调整而移动；60% 是产品阈值，不是统计证明。

---

## 9. 评估参数（独立）

```rust
pub struct EvalParams {
    pub blind_samples: u32,        // 默认 30
    pub blind_samples_max: u32,    // 默认 60；CI 跨 50% 时预注册扩展
    pub pilot_samples: u32,        // 默认 10；预实验，不计入正式 30
    pub reviewers_min: u32,        // 默认 5
    pub reviewers_max: u32,        // 默认 7
    pub pilot_reviewers_min: u32,  // 默认 5
    pub pilot_reviewers_max: u32,  // 默认 8
    pub win_threshold: Permille,   // 默认 600 = 60%
    pub min_kappa: Permille,       // 默认 200 = 0.2
    pub forbid_unanimous_greedy: bool,
    pub boundary_dev_warn: Permille, // 默认 200 = 20%
    pub boundary_dev_fail: Permille, // 默认 500 = 50%
    pub height_jump_threshold: Width, // 默认 0.5em
    pub length_limited_warn: Permille, // 默认 200 = 20%
}
```

- 与 `ParamSet` 分开文件/模块。
- 改版式参数不能改 `win_threshold`；反之亦然。
- 评估协议版本 `EVAL_VERSION` 写进报告。
- `reviewers_min < 5` 只能标为探索性，不能作为门禁。
- **Gate Q 可判 Inconclusive；它不阻塞 Gate T。** 预实验 κ 与正式盲测结论都要进报告。

---

## 10. 参数清单（default-v1 起点）

> 下表是 `default-v1` 的起点，不是硬编码常量。宽度单位是 1/4096 base em；比例是 permille。

### 10.1 Tier A：布局/fallback

| 参数 | 默认 | 范围 | 标定 | 说明 |
|---|---|---|---|---|
| `tolerance` | 3000 | 0..=10000 | 工程 | 正常阶段最大 `abs(r)`；3.0 是硬上限，不是首选比例 |
| `emergency_stretch` | 0 | 0..=10em | 工程 | 正常阶段额外 stretch；通常 0 |
| `emergency_tolerance` | 10000 | 0..=10000 | 工程 | fallback 最大 ratio |
| `max_lines` | none | 1..=10000 | 工程 | 调用方保护 |
| `max_kp_atoms` | none | `None` 或 1..=10000 | 工程 | 超过后改用 Greedy-C；K6 实测 4,000 atoms 已在 native 预算内，默认保留 `None` |
| `trim_priority` | EndFirst | enum | 规则 | 行尾先裁；行尾标点的侧空在两端对齐行上恒裁（ADR-0036），行首开标点在两端对齐与左对齐行上贴墨裁（ADR-0082/0105），其余行首侧空只在行超宽时裁 |
| `tie_break` | CostLineStart | enum | 固定 | 代价→行数→起点更小 |
| `features.edge_trim` | true | bool | 规则 | K4 启用 |
| `features.boundary_deviation` | true | bool | 规则 | K2 启用 |
| `features.height_cost` | false | bool | 实验 | K3 默认关闭 |
| `features.fitness` | false | bool | 实验 | K7 才启用；`rhythm-v1` 打开（ADR-0064） |
| `features.forced_breaks` | true | bool | 规则 | K5 启用 |
| `fallback.allow_emergency` | true | bool | 固定 | fallback 开关 |
| `fallback.allow_forbidden` | true | bool | 固定 | 最后一搏允许 Forbidden 断点；结果标 `ForbiddenRescue` |
| `fallback.allow_overfull` | true | bool | 固定 | 紧急阶段是否接受终端/零 shrink 的超宽行 |
| `fallback.emergency_stretch` | 3em | 0..=10em | 工程 | 复制到 `break_params.emergency_stretch` 后使用 |

### 10.2 Tier A：代价权重

| 参数 | 默认 | 范围 | 标定 | 说明 |
|---|---|---|---|---|
| `line` | 10 | 0..=1e9 | 是 | 每行固定 |
| `badness_scale` | 100 | 0..=1_000_000 | 是 | `B = scale × abs(r)^p`；`|r|=1` 时 cost=100 |
| `badness_exponent` | 3 | 1..=5 | 否（高级） | 默认 3 |
| `break_penalty` | 100 | 0..=1e9 | 是 | `penalty=100` 时 cost=100（10 行） |
| `compress_amount` | 20 | 0..=1e9 | 是 | 每 1 em trim；0.25em = 5 |

> `publish-v1` 只改 `tolerance`（1500）；`badness_scale=100`、权重、fallback 与
> 短行阈值都与 `default-v1` 相同，便于对照。`book-v1`（ADR-0106）在
> `publish-v1` 上只把 `short_last_threshold` 提到 3em，用于中文书籍样张；
> 自 ADR-0112 起同一个短行项也计非末行的孤字行。
> `rules_version = 3` 对应 ADR-0124（相邻标点 pair gap、trim 按边计数、移除
> `PAIR_TRIM`）；2 对应 ADR-0111/0112（代码分段、分隔符 penalty 与短行口径）；
> 初值是 ADR-0023 的开标点 trim。
| `compress_count` | 5 | 0..=1e9 | 是 | 每条被裁的边；一行两侧都裁计两次 |
| `boundary_deviation` | 50 | 0..=1e9 | 是 | 每 1 个 ideal gap 的平均偏差；满 gap = 5 行 |
| `short_last_scale` | 1_000_000 | 0..=1e9 | 是 | 短行（末行与孤字行）；0 宽行的 cost |
| `short_last_threshold` | 2em | 0..=8em | 是 | 低于此值计费 |
| `height_scale` | 0 | 0..=1e9 | 实验 | 行高代价；默认关闭 |
| `height_threshold` | none → body height | `None` 或 0..=5em | 实验 | 超出部分计费 |
| `emergency` | 10_000_000 | 0..=1e9 | 固定 | 每行 emergency |
| `forbidden` | 100_000_000 | 0..=1e9 | 固定 | rescue 禁断 |
| `fitness` | 0 | 0..=1e9 | 实验 | 每跨一个松紧等级记一次；`rhythm-v1` 为 10（ADR-0064） |

> 这些数字只是起点。真正标定后写入新的 preset；不要把它们散落在代码里。
>
> **这些值按归一化公式重新推导，不是旧值的等比换算。** 设计基准：一行 = 10；一次 0.25em trim + 事件约 1 行；一个 ideal gap 的平均偏差 = 5 行；`|r|=1` 的 badness = 10 行。旧版的 `compress_amount=1 / 1/4096em`、`compress_count=1000` 与 `line=10` 不可比，已废弃。

### 10.2b 量纲（避免权重互相吸收）

| 参数 | 量纲 | 归一化 | 说明 |
|---|---|---|---|
| `line` | cost / line | 1 | 每行固定 |
| `badness_scale` | cost / badness-unit | `abs(r)^p` | `|r|=1` 时的 cost |
| `break_penalty` | cost / rule-penalty-unit | `penalty / 100` | penalty=100 时的 cost |
| `compress_amount` | cost / em-trimmed | `c / EM` | 每 1 em trim |
| `compress_count` | cost / trimmed edge | `[trim_left > 0] + [trim_right > 0]` | 每条发生 trim 的边 |
| `boundary_deviation` | cost / ideal-gap-deviation | `D / ideal_gap` | 每 1 个 ideal gap 的平均偏差 |
| `short_last_scale` | cost / short-score-unit | `short / threshold` | 0 宽短行的 cost（末行或孤字行） |
| `height_scale` | cost / body-height-excess | `excess / body_height` | 每超出 1 个正文行高 |
| `emergency` / `forbidden` | cost / event | 1 | 每行/每次 rescue |

标定时先固定量纲，再调数值；不要把 `boundary_deviation` 调成「等价于压缩权重缩放」。K6 报告里要给出每一项的典型贡献量级。

### 10.3 Tier B：规则比例（RuleSet）

| 参数 | 默认 | 范围 | 说明 |
|---|---|---|---|
| `cjk_gap.width` | 0 | 0..=1000‰ | CJK-CJK 间距 |
| `cjk_gap.stretch` | 32‰ | 0..=1000‰ | 1/32 em |
| `cjk_gap.shrink` | 32‰ | 0..=1000‰ | 1/32 em |
| `cjk_latin_gap.width` | 250‰ | 0..=1000‰ | 0.25em，待 clreq 核实 |
| `cjk_latin_gap.stretch` | 62‰ | 0..=1000‰ | 约 1/16 em |
| `cjk_latin_gap.shrink` | 62‰ | 0..=1000‰ | 约 1/16 em |
| `space_stretch` | 125‰ | 0..=1000‰ | 1/8 em |
| `space_shrink` | 125‰ | 0..=1000‰ | 1/8 em |
| `open_trim_left` | 250‰ | 0..=1000‰ | 开标点左侧；待规则表 |
| `close_trim_right` | 500‰ | 0..=1000‰ | 关闭/点号右侧；待规则表 |
| `middle_trim_left` | 500‰ | 0..=1000‰ | 点号左侧；待规则表 |
| `middle_trim_right` | 500‰ | 0..=1000‰ | 点号右侧；待规则表 |
| `punct_penalty_table` | 0 | table id | 标点堆叠等 |
| `pair_gap` | 250‰ / 125‰ | 0..=1000‰ | 相邻标点的目标墨距（全角/半角） |
| `pair_cap` | 500‰ / 125‰ | 0..=1000‰ | 单个 pair 的最大收紧量 |

> 规则比例属于 `RuleSet`，有来源和版本；不要从 `CostParams` 里改。
> 这些比例都是 **base em**。如果业务希望按字号缩放（例如大字号标点的 trim 更大），规则层用 `font_scale` 乘一下再写入 IR；breaker 只看最终宽度。
> **K4/v2 状态：** `soma-rules` 已把 `open_trim_left` / `close_trim_right` / `middle_trim_left` / `middle_trim_right` 写进 `Atom.compress_*`。行尾收尾标点的右侧侧空在**两端对齐行上恒裁**，与宽窄无关（ADR-0036）；行首开标点在两端对齐与左对齐/ragged 行上都贴墨裁（ADR-0082/0105），其余行首侧空只在行超宽时裁。半角标点的侧空为 `125‰`（`LATIN_TRIM`，代码常量，不在 Tier B 表内）。这两组数值是**回退值**：`soma.metrics/2` 表若带标点轮廓（`ink_right`），`scale_trim` 用真实墨迹覆盖它们（ADR-0037）。
> **相邻标点（v2）：** `PAIR_TRIM` 已删除。`soma-layout::apply_pair_gap` 用 `ink_right + ink_left` 得到真实墨距，按 `punct.pair.gap` 的目标收紧，写入 `Boundary.pair_squeeze`；规则只在行内边界生效，断点边界被排除。`pair_gap` / `pair_cap` 仍是 `ParamOnly`（未标定），不参与 Gate Q 的硬断言。
> **K6 状态：** `soma_rules::RULES` 提供 `id / status / source` 盘点表；`RuleStatus` 为 `Verified | Tbd | ParamOnly`。当前盘点为 **8 `Verified` / 0 `Tbd` / 7 `ParamOnly`**：标点硬规则已引用 GB/T 15834-2011 via W3C clreq §6.1.1（basic）并升为 `Verified`；trim/gap 比例仍为 `ParamOnly`，Gate Q 不得把比例数值当作已核实来源。
> **每条规则带 `rule_id` / `source` / `status: Verified | Tbd | ParamOnly`。** `Tbd` 只能产生 `Allowed` + penalty/trim，不能产生 `Forbidden`/`Forced`；`ParamOnly` 只提供参数，不产生断点规则。K0 交付 `ruleId × source × status × golden` 盘点表；Gate Q 只对 `Verified` 断言。

### 10.3b Tier P：段落参数（ParagraphStyle）

| 参数 | 默认 | 范围 | 说明 |
|---|---|---|---|
| `width` | 由文档决定 | >0 | 内容宽度，base em |
| `first_indent` | 2em | 0..=10em | 首行缩进 |
| `align` | Justify | enum | 枚举保留 Justify/Left/Center/Right；MVP 只实现 Justify |
| `line_height_min_override` | none | 0.5..=5em | 可选，覆盖样式最小行高 |

### 10.4 Tier S：样式/度量

| 参数 | 说明 |
|---|---|
| `font_scale` | 相对 base em 的缩放（250..=4000‰） |
| `advance` | 按 style + text 查询；CJK/Han 可区间压缩 |
| `ascent` / `descent` / `leading` | 行盒 |
| `baseline_shift` | 上下标 |
| `overhang_left/right` | 视觉溢出；不进入 advance、不参与断行/trim |
| `space_width` | 空格宽度 |
| `line_height_min` | 该样式最小行高；默认 1.5em |

> `overhang` 与 `trim` 分开：`overhang` 是 `GlyphMetrics` 的视觉信息；`trim_left/right` 是 `RuleSet` 的布局压缩，会改变行宽和坐标。不要把二者混用。

### 10.5 Tier E：评估（独立）

| 参数 | 默认 |
|---|---|
| `blind_samples` | 30 |
| `blind_samples_max` | 60 |
| `pilot_samples` | 10 |
| `reviewers_min/max` | 5 / 7 |
| `pilot_reviewers_min/max` | 5 / 8 |
| `win_threshold` | 600‰（60%） |
| `min_kappa` | 200‰（0.2） |
| `boundary_dev_warn/fail` | 200‰ / 500‰ |
| `height_jump_threshold` | 0.5em |
| `length_limited_warn` | 200‰（20%） |
| `forbid_unanimous_greedy` | true |

---

## 11. 与代码的映射

| 模块 | 职责 |
|---|---|
| `soma-core::params` | `ParamSet`、`BreakParams`、`CostParams`、`FallbackParams`、`FeatureFlags`、校验、hash |
| `soma-core::preset` | `default_v1()` 等内嵌 preset；算法不含数字默认值 |
| `soma-core::breaker` | 只读 `BreakParams` + `ParaIR`；不解析规则/样式 |
| `soma-rules` | `RuleSet` + `ParagraphStyle` + `Metrics` → `ParaIR` |
| `soma-eval` | `EvalParams` + 指标；不进入 `ParamSet` |
| `soma-wasm` | 接收 preset/overrides；不访问文件系统 |

`BreakConfig` 现在持有 `ParamSet`；`ParamHash` 在构造时捕获：

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

`CostWeights` 已移除；算法不再保留数字默认值。

---

## 12. 需要拍板的参数化问题

| 问题 | 建议 |
|---|---|
| 哪些参数暴露给用户 | Tier A 的 4–6 个：行宽、tolerance、badness 权重、中英 gap、压缩权重 |
| 哪些参数只给规则表 | gap/trim/penalty 全部 Tier B；用户不直接改 |
| 代价权重能否按段落覆盖 | **不能**；按请求/文档。段落只改几何 |
| `badness_exponent` 是否可调 | 默认 3；作为高级参数，不进入 demo |
| preset 存储 | 内嵌 Rust 函数 + 可选 JSON；WASM 不依赖文件系统 |
| `first_indent` / `line_height_min` 归属 | 前者 Tier P，后者 Tier S |
| 规则参数是否按 style 覆盖 | 允许规则层按 style 缩放 trim；breaker 不感知 |
| 评估参数是否进 ParamSet | **不进**；单独 `EvalParams` |
| 参数 hash 的粒度 | `BreakParams` + flags；规则/度量用各自 version |
| 默认值版本 | `default-v1`；改默认值换 preset id |
| 长度策略 | `max_kp_atoms` + Greedy-C + `breaker_used`；禁止时间型 fallback |
| 非 Justify | 枚举保留完整；MVP 只实现 Justify，其余 `Unsupported` |
| 规则状态 | `Verified / Tbd / ParamOnly`；Tbd 不得产生硬禁断 |
| 规则盘点 | K0 交付 `ruleId × source × status × golden` 表 |
| 量纲 | 每项先按 `EM/ideal_gap/PENALTY_REF/body_height` 归一化，再乘权重 |
| 盲测 | 预实验 5–8 人测 κ；30 段 × 5 评审 + CI + κ；10 段不再作为门禁 |
| 门禁分层 | Gate T 技术验收 / Gate Q 质量宣称 / Gate D 交付 |
| length_limited | 质量按 `Optimal` 子集单列；关键样本 >20% 只做部分宣称 |
| canonical 兼容 | 同 schema 才比较字节；reader 忽略未知字段 |

---

## 13. 测试

### 已实现的运行时测试

| 断言 | 测试 | 位置 |
|---|---|---|
| `default-v1` 与冻结表一致 | `default_v1_matches_the_frozen_table` | `tests/k1_contract.rs` |
| preset 与 `ParamSet` 单一来源 | `preset_module_and_paramset_are_one_source` | 同上 |
| 非法值被拒绝（weight/exponent/max_lines/max_kp_atoms/emergency/threshold） | `validation_rejects_bad_values` | 同上 |
| 单字段覆盖只改一个字段，且 `paramHash` 变化 | `override_changes_only_the_expected_field` | 同上 |
| `paramHash` 稳定、权重敏感、不含 rules/metrics 版本 | `param_hash_is_stable_and_weight_sensitive` | 同上 |
| 非 Justify 全部 `Unsupported` | `every_non_justify_align_variant_is_unsupported` | `tests/k1_breaker.rs` |
| `max_kp_atoms` 改变 `breaker_used` | `max_kp_atoms_uses_greedy_c` | `tests/k6_fallback.rs` |
| 同参数同 canonical 字节 | `result_is_byte_identical_across_runs` | `../soma-rules/tests/vertical_slice.rs` |
| 规则盘点完整且唯一 | `rule_inventory_is_complete_and_unique` | `soma-rules` 单测 |
| `Verified` 规则不再带临时来源 | `verified_rules_do_not_carry_provisional_sources` | 同上 |
| 标点硬规则不得退回 `Tbd` | `hard_break_rules_are_verified`、`tbd_rules_are_never_hard_break_rules` | 同上 |
| `length_limited` 占比可报告 | `length_limited_share_is_reported_per_corpus` | `soma-eval` 单测 |
| 盲测统计（Wilson / Fleiss κ） | `wilson_interval_*`、`fleiss_kappa` 相关 | `soma-eval` 单测 |

### 由类型/模块边界强制（不是运行时测试）

- `rule_params_do_not_enter_breaker`：`BreakParams` 结构上没有 gap/trim 字段，写进 breaker 无法编译。
- `style_metrics_are_not_cost_weights`：`CostParams` 结构上没有 advance/ascent，写入无法编译。
- `overhang_is_not_trim`：`GlyphMetrics.overhang` 与 `Atom.compress_*` 是不同来源，trim 只读后者。
- `eval_params_do_not_change_layout`：`EvalParams` 不进 `ParamSet`，也不进 `paramHash`。

这些约束由 crate 分层和字段归位保证；加一条运行测试只能重复编译器已经做的检查。

### 尚未实现（需要外部输入）

- `pilot_kappa_measured`：需要 5–8 人预实验，属 Gate Q。
- `unknown_fields_ignored`：需要 JSON reader；当前 canonical JSON 是 write-only，核心不依赖 serde。

---

## 14. 一句话总结

**算法只认 `BreakParams`；中文规则认 `RuleSet`；字体认 `Metrics`；段落认 `ParagraphStyle`；成功标准认 `EvalParams`。默认值是 `default-v1` preset，不是代码里的常量。**
