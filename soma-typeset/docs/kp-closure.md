# K-P 收尾报告（K1–K6 / K8）

状态：**K-P 内联内核（inline kernel）工程收尾完成，Gate T 全绿。**
Gate Q 仍为 **Inconclusive**（无真实盲测与质量标注语料），Gate D 为 **Partial**
（无浏览器包体积 / 首段延迟实测）。K7（相邻行 fitness / rhythm 状态）未启动，
按冻结决策保持“仅在评估证明需要时才做”。

本文件是 K1–K6/K8 的**收口清单**：Gate T 逐项打勾、性能数字、长度策略决定、
以及仍然诚实的缺口。规范来源仍是 `docs/architecture-v2.md`、`docs/kp-model.md`、
`docs/params.md`、`docs/adr/`；本文件不复述模型定义。

---

## 1. Gate T（技术验收）

| # | 项目 | 验证方式 | 状态 |
|---|---|---|---|
| T1 | DP 与暴力枚举最优解一致 | `brute_force_matches_dp`、`brute_force_matches_dp_across_lengths`、`brute_force_matches_dp_with_forced_breaks`（n ≤ 16 全枚举） | ✅ |
| T2 | `K-P ≤ Greedy-C` 硬不变量 | `optimal_is_not_worse_than_greedy_baselines`（core + `soma-rules` 端到端） | ✅ |
| T3 | canonical 字节稳定 | `result_is_byte_identical_across_runs`、`canonical_json_is_stable_and_integer_only`、schema `soma.layout/6` | ✅ |
| T4 | `paramHash` 只随参数变化 | `param_hash_is_stable_and_weight_sensitive`、`override_changes_only_the_expected_field`、`validation_rejects_bad_values` | ✅ |
| T5 | native / `wasm32-wasip1` 同哈希 | `scripts/wasm-hash.sh` → `native/wasm canonical bytes match (4901 bytes)` | ✅ |
| T6 | 核心无浮点 | 全整数 `Width` / `Permille` / `Cost`；`soma-core` 无 `f32`/`f64`；badness 用整数幂 | ✅ |
| T7 | 依赖边界 | `gates.sh` 中 `cargo tree -p soma-core` 拦截 typst / font / wasm-bindgen / comrak / serde_json | ✅ |
| T8 | clean-room | `gates.sh` 拦截活跃文件出现旧项目名；`docs/archive/` 例外 | ✅ |
| T9 | 长度策略确定性 | `bench --max-kp-atoms 2000 --check`：超限段落走 `GreedyC`，无时间型 fallback | ✅ |
| T10 | `cjk_latin_adjustments` 自洽 | 逐边界分配量求和 == 该行 CjkLatin 总调整量；`Line.boundary_deviation` 与 eval 精确值同时输出 | ✅ |
| T11 | 规则盘点完整 | `soma_rules::RULES`：8 `Verified` / 0 `Tbd` / 6 `ParamOnly`；`verified_rules_do_not_carry_provisional_sources`、`hard_break_rules_are_verified`、`tbd_rules_are_never_hard_break_rules` | ✅ |
| T12 | K3 / K4 / K5 fixtures | `k3_styles`(6)、`k4_trim`(10)、`k5_forced`(7) 全通过 | ✅ |
| T12b | 非 Justify 全部报 `UnsupportedAlign` | `every_non_justify_align_variant_is_unsupported`（Left/Center/Right 逐个）；枚举已保留完整四值 | ✅ |
| T13 | 剪枝不改变结果 | `pruning_matches_brute_force_on_varied_widths`、`pruning_reduces_the_candidate_count` | ✅ |
| T14 | fallback ladder 可解释 | `FallbackStage` / `emergency_lines` / `forbidden_violations` 逐字段有测试；跨阶段总代价不可比 | ✅ |
| T15 | 单原子行 + 双侧 trim | 行宽、`adjustment = 0`、渲染坐标自洽（corner case 已覆盖） | ✅ |
| T16 | breaker 不认识中文规则 | `breaker_ignores_punctuation_class`：同一 IR 在标点类为 `None` 时断点完全相同；标点语义只通过 `Boundary`/`compress_*` 进入 | ✅ |

`bash scripts/gates.sh` 顺序执行：
`fmt` → `clippy -D warnings` → `tests` → `wasm32 check` → `wasm32-wasip1 hash`
→ `demo render` → `dependency boundary` → `clean-room` → `all gates passed`。

`bash scripts/kp-verify.sh` = gates + 基准预算 + 长度策略，全部 PASS。

## 2. 测试规模

**116 tests**（K-P 部分；A1 加入 `soma-doc` 后工作区总数为 159），按 seam 分布：

| crate / suite | 数量 |
|---|---:|
| `soma-core` 单测（params / breaker / result / metrics） | 25 |
| `k1_breaker` | 4 |
| `k1_contract` | 5 |
| `k1_fixtures` | 10 |
| `k2_boundary` | 5 |
| `k3_styles` | 6 |
| `k4_trim` | 10 |
| `k5_forced` | 7 |
| `k6_reporting` | 7 |
| `k6_fallback` | 4 |
| `soma-rules` 单测 | 13 |
| `vertical_slice`（rules → core → 结果） | 6 |
| `soma-eval` 单测 | 10 |
| `soma-demo` 单测 | 3 |
| `soma-wasm` 单测 | 1 |

其中**差分测试**（暴力 vs DP、DP vs 剪枝、K-P vs Greedy）共 5 个，是 Gate T 的主力。

## 3. 性能（native release，合成度量，5 repeats）

`cargo run -p soma-eval --release -- bench --repeats 5 --check` → **budget PASS**。

| 语料 | atoms | lines | P50 ms | P95 ms | evaluated | stage | overfull |
|---|---:|---:|---:|---:|---:|---|---:|
| pure-cjk | 100 | 5 | 0.008 | 0.022 | 90 | normal | 0 |
| pure-cjk | 500 | 25 | 0.056 | 0.094 | 1,580 | normal | 0 |
| pure-cjk | 1,000 | 50 | 0.120 | 0.121 | 3,580 | normal | 0 |
| pure-cjk | 2,000 | 100 | 0.248 | 0.302 | 7,580 | normal | 0 |
| pure-cjk | 4,000 | 200 | 0.549 | 0.551 | 15,580 | normal | 0 |
| mixed | 100 | 7 | 0.011 | 0.012 | 260 | normal | 6 |
| mixed | 500 | 34 | 0.064 | 0.087 | 1,544 | normal | 33 |
| mixed | 1,000 | 67 | 0.119 | 0.121 | 3,140 | normal | 66 |
| mixed | 2,000 | 134 | 0.239 | 0.277 | 6,344 | normal | 133 |
| mixed | 4,000 | 267 | 0.554 | 0.568 | 12,740 | normal | 266 |
| punct-dense | 100 | 5 | 0.006 | 0.007 | 16 | normal | 1 |
| punct-dense | 500 | 27 | 0.141 | 0.147 | 4,403 | emergency | 0 |
| punct-dense | 1,000 | 54 | 0.283 | 0.288 | 8,906 | emergency | 0 |
| punct-dense | 2,000 | 107 | 0.569 | 0.574 | 17,929 | emergency | 0 |
| punct-dense | 4,000 | 214 | 1.199 | 1.206 | 36,028 | emergency | 0 |

- **peak RSS ≈ 3.7 MiB**（`# peak_rss_kb=3748`）。
- 小 size 的 P95 有定时器 granularity 抖动（例如 100 atoms 的 P95 0.022 ms）；
  以 P50 为准，量级结论不变。
- 4,000 atoms 最差语料 ≈1.2 ms P50，**远低于原生预算**；`evaluated` 只随可
  断行对数增长，说明 conservative window 生效（punct-dense 4,000 只评估
  36k 对，不是 8M 对全量）。
- `mixed` 的 `overfull > 0` 是**预期**：长英文 run 不可断，只能 overfull。
- `punct-dense` 进入 `emergency` stage 但 `emergency_lines = 0`：这是 relaxed
  阶段无 emergency flag 成功，属正常路径，不是错误。

### 长度策略实测

`bench --sizes 2000,4000 --max-kp-atoms 2000`：

| 语料 | atoms | breaker | length_limited | 结果一致性 |
|---|---:|---|---|---|
| pure-cjk | 2,000 | optimal | false | cost 与无上限时相同 |
| pure-cjk | 4,000 | greedyC | true | 确定性回退 |
| mixed | 2,000 | optimal | false | 同上 |
| mixed | 4,000 | greedyC | true | 同上 |
| punct-dense | 2,000 | optimal | false | 同上 |
| punct-dense | 4,000 | greedyC | true | 同上 |

## 4. 决定：`max_kp_atoms` 保持 `None`

`default-v1` **不设** `max_kp_atoms`。理由：

1. 实测 4,000 atoms 最差 1.21 ms P95，距原生预算有两个数量级余量；
2. 长度策略已实现并被测试锁定（`length_limited` / `breaker_used = GreedyC`），
   需要时可作为 **preset 改动**启用，不是模型改动；
3. 设置上限会静默降低长段落质量，而当前没有证据表明需要。
4. 若未来浏览器 WASM 实测超预算，把 `max_kp_atoms = Some(2000)` 写进新 preset
   并在评估里**单列 `length_limited` 子集**，避免用 Greedy-C 结果冒充 K-P 质量。

## 5. 仍然诚实的缺口

完整台账见 [`docs/debt.md`](debt.md)（D01–D30）。这里是 K-P 相关的子集：

| ID | 缺口 | 影响 | 阻塞什么 |
|---|---|---|---|
| D01 | 无真实盲测与质量标注语料 | Gate Q 只能 `Inconclusive` | 质量宣称，不阻塞技术交付 |
| D02 | 无浏览器 WASM 包体积 / 首段延迟 | Gate D 只能 Partial | 交付指标（A5 关闭） |
| D03 | `FallbackStage::Overfull` 未发射 | 长英文词统一走 emergency | 无（语义等价，字段保留） |
| D04 | trim/gap 比例为 `ParamOnly` 未标定 | Gate Q 不得引用比例数值 | 标定阶段（A6 后） |
| D05 | Tier A 权重未做真实语料标定 | `default-v1` 是起点不是最优 | 标定阶段（A6 后） |
| D06 | 无真实字体度量（`SyntheticMetrics`） | 性能数字不含字体查询 | A3 前关闭 |
| D09 | 无 hyphenation（长英文词只能 overfull） | mixed 4,000 atoms 有 266 行 overfull | 未排期 |
| D10 | `boundary_deviation` 比例分配假设无自动告警 | 假设失效时不报警 | A4 后加报告项 |
| D12 | K7 fitness/rhythm 未做 | 相邻行松紧节奏不建模 | 仅当评估证明需要 |
| D14 | ~~`soma-core::semantic` / `text` legacy~~ | 已在 A1 删除（ADR-0008） | ✅ 已关闭 |

## 6. 精度敏感性

1/4096 em 的整数量化不会改变断行质量：在 `corpus/zh` + 表格/公式/书稿
（606 块、1213 行）上，对每个字形做 ±1 单位的确定性扰动，真实舍入形态
（`jitter`）下断点变化 **0%**；把所有字宽系统性 +1/−1，行位置变化也只有
0.17% / 1.40%。详见 [`docs/precision.md`](precision.md)。

## 7. 复现命令

```sh
bash scripts/kp-verify.sh                  # gates + 基准 + 长度策略
bash scripts/gates.sh                      # 仅门禁
bash scripts/bench.sh                      # 基准预算检查
bash scripts/wasm-hash.sh                  # native vs wasm32-wasip1
cargo run -p soma-eval --release -- inspect --kind mixed --atoms 500
cargo run -p soma-eval --release -- sweep-tolerance
cargo run -p soma-eval --release -- sweep-weight --param badness_scale
cargo test -p soma-eval --test perturbation -- --ignored --nocapture  # 精度敏感性
```
