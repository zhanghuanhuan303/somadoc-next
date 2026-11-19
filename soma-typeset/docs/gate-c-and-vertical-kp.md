# Gate C 与垂直 K-P 交付报告

状态：**两项完成。** 工作区 **252 tests**，`gates.sh` 全绿：Gate A、Gate C、
native/`wasm32-wasip1` 全管线字节一致（**8,983 bytes**）。

---

## 1. Gate C（内容门禁）

### 1.1 能力矩阵

`soma_md::CAPABILITIES` 是唯一事实来源，25 条，分三类：

| 状态 | 数量 | 含义 |
|---|---:|---|
| `Supported` | 18 | 建模成 DocIR 结构 |
| `Degraded` | 7 | 内容以文本保留，结构不建模 |
| `Rejected` | 0 | 当前没有拒绝项 |

`docs/capabilities.md` 与代码**双向**校验：文档里出现的 id 必须在代码里存在，
代码里的每条也必须在文档里有对应表格行——`every_capability_is_documented`
和 `the_documented_ids_all_exist_in_code` 两个测试锁死。

### 1.2 降级承诺

`Degraded` 不是「丢掉」，而是「保留为文本」。每个降级构造都有一条断言，检查源文本
仍出现在 `Document::plain_text()` 里：嵌套列表、setext 标题、表格、HTML 块/行内、
链接引用定义、未闭合行内代码。

### 1.3 渲染安全

`crates/soma-render-html/tests/safety.rs`：

- 文本里的 `<script>`、`onerror=`、`<img>` 全部转义，不产生标签；
- 链接与图片**不产生** `href=` / `src=`，也不泄漏 URL（图是占位框，链接只画文字）；
- 内容里的 `<style>` 不会变成样式块（输出里只有一个渲染器自己的 `<style>`）。

### 1.4 门禁

`scripts/gate-c.sh`：跑上面两个测试目标 + 检查 `docs/capabilities.md` 存在且标注 Gate C。
已接入 `gates.sh`。

---

## 2. 垂直 K-P（分页）

### 2.1 模型

分页从「贪心填充」升级为**垂直 Knuth-Plass**：在块与块之间选择断点，最小化

```
sum over pages:  page_weight + badness(leftover)
badness = badness_scale * leftover_permille^3 / 1_000_000
```

- 末页不计 badness（与段落末行同理）。
- `page_weight = 0`：页数本身不计费，靠 badness 自然抑制过松的页。
- 单块高于内容区：允许，但必须独占一页，并计 `overflow_penalty`。
- 脚注区高度**参与** DP：候选页的脚注集合随块增长，逐项累加，不需要事后迭代。

### 2.2 硬约束

- `PageBreak` / `page_break_before`：在该处必须断页；
- `keep_with_next`：不允许在该块之后断页；
- 强制断点之前的页面不能被穿透。

### 2.3 输出变化

| 字段 | 变化 |
|---|---|
| `PaginationKind` | 新增 `VerticalKp`；`Greedy` 保留为显式策略与回退 |
| `PagedLayout.pagination_cost` | 按同一模型给最终分页打分，可跨策略比较 |
| `PagedLayout.pagination` | 默认 `"verticalKp"` |
| `footnote_passes` | 垂直路径为 1（脚注已在 DP 内） |
| canonical `soma.pages/1` | 新增 `paginationCost` |

`layout()` 用垂直 K-P；`layout_with(..., PaginationStrategy, VerticalParams)` 可显式选
贪心或调垂直代价。DP 找不到可行解时回退贪心并如实报告 `Greedy`。

### 2.4 测试

- `vertical_pagination_never_costs_more_than_greedy`：同一模型下 DP 代价 ≤ 贪心代价，
  页数也不增加（相当于一个最优性差分测试）。
- `vertical_pagination_balances_a_ragged_tail`：贪心留下空尾页时，DP 代价不劣。
- `vertical_pagination_respects_forced_breaks`。
- `vertical_pagination_keeps_headings_with_next`。
- `vertical_pagination_is_deterministic_and_reports_its_cost`。
- 原有分页测试（多页、页顶对齐、溢出、空文档）继续通过。

### 2.5 限制

- 垂直 glue 只有固定 `space_before`/`space_after`，没有可拉伸的垂直胶水；badness 只
  反映剩余空间，不能像 TeX 那样把标题前的间距拉开来补页。
- 段落仍不跨页，因此没有 widow/orphan 行级规则（D49）。
- 页面母版 / 页眉页脚 / 页码仍未实现（D50）。
- `VerticalParams` 目前是布局层常量，还不是 `EngineConfig` 的一部分（D29）。

---

## 3. 门禁现状

| Gate | 内容 | 状态 |
|---|---|---|
| Gate T | 行内差分、canonical、native/wasm（含页面） | ✅ |
| Gate A | 层依赖、`no_std`、无浮点、schema 常量、DocIR 纯净 | ✅ |
| Gate C | 能力矩阵、降级承诺、渲染安全 | ✅ |
| Gate R | 跨渲染器坐标一致 | 未建（只有 HTML 一个渲染器） |
| Gate Q | 质量宣称 | `Inconclusive`（无语料/盲测） |
| Gate D | 浏览器 wasm 体积与首段延迟 | Partial |

## 4. 复现

```sh
bash scripts/gate-c.sh
cargo test -p soma-layout --test a4_pagination
bash scripts/gates.sh
```
