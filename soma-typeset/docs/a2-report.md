# A2 report（`soma-template`）

状态：**A2 complete.** `soma-template` 落地：YAML 子集解析、精确单位、级联与继承、
`StyledDoc` 纯数据、两个 hash、Gate A 扩展。工作区 **187 tests**，
`scripts/gates.sh` / `scripts/gate-a.sh` 全绿。

关闭债务：**D21**（模板层）、**D30** 的 `templateHash` / `styleHash`、**D41**（属性集复核）。

---

## 1. 交付物

| 文件 | 内容 |
|---|---|
| `crates/soma-template/Cargo.toml` | 只依赖 `soma-core` + `soma-doc` |
| `src/yaml.rs` | 零依赖 YAML 子集解析器；值带行列号；重复键报错 |
| `src/units.rs` | `Rational`（精确十进制）、`Unit`、`LengthSpec`、`LineHeightSpec`；向零截断 |
| `src/template.rs` | `Template`/`PageSpec`/`PageSize`/`StyleRule`/`Selector`/`Property`/`PropertyValue`；解析与校验 |
| `src/style.rs` | `ComputedStyle`、`StyleTable`、`ResolvedPage`、`StyledDoc`、`resolve` 级联与继承 |
| `src/canonical.rs` | `soma.template/1` 与 `soma.styles/1` canonical JSON + FNV 哈希 |
| `src/lib.rs` | 契约、`TemplateHash`/`StyleHash`、`TemplateError` |
| `tests/a2_template.rs` | 10 个：解析、规范化、hash、各类错误 |
| `tests/a2_units.rs` | 7 个：精确换算、截断、严格语法 |
| `tests/a2_resolve.rs` | 11 个：级联、继承、页面几何、去重、hash |
| `docs/template.md` | 模板 DSL 规范 |

Gate A 增加：`soma-template` 依赖白名单、`no_std`、无浮点、两个 schema 常量、
模板不得接受引擎键。

## 2. 关键决定

1. **零依赖 YAML 子集，而不是引入解析器**。理由：错误必须带行列号；DSL 由我们定义；
   核心里已经有「无 serde」的纪律。公共 API `parse_template(&str)` 与解析器解耦，
   将来换成完整解析器不影响上层。
2. **十进制 → 有理数，不经过浮点**。`10.5` 是 `21/2`，`1mm` 是 `360/127 pt`，
   除法向零截断（与内核 `Width::from_em_frac` 同一规则）。
3. **`1em` 由根字号定义**，所以模板必须给 `document` 写绝对 `font-size`；物理单位
   都以它为锚点。没有这个锚点，`mm` 和 `em` 无法互相换算。
4. **`StyledDoc` 是纯数据**（ADR-0010 §3）：无生命周期、可缓存、可 hash、可跨线程。
   `style_hash` 因此是机械产物，而不是另造一套机制。
5. **属性按固定顺序应用**，`font-size` 在前。YAML 键顺序不影响结果。
6. **`align` 只接受 justify**（模型 v1）。其余值在 resolve 时报错，而不是悄悄退回左对齐。
7. **未知即错误**：未知键、未知属性、未知元素名、重复键、重复 class、空选择器。
8. **页面几何进模板，窗口进引擎**（架构 §9）。模板写不了 `window`。

## 3. Gate A2 清单

| # | 项目 | 验证 | 状态 |
|---|---|---|---|
| A2.1 | `10.5pt / 1.6 / 2em` 精确换算 | `a2_units` 7 个测试；`210mm → 232214`、`20mm → 22115`、`1in → 28086` | ✅ |
| A2.2 | 无浮点 | `Rational` + i128 中间量；Gate A grep；`canonical` 无小数 | ✅ |
| A2.3 | 错误带行列号 | `unknown_property_is_rejected_with_a_position` 等 | ✅ |
| A2.4 | 未知键/属性/元素/重复键报错 | `a2_template` 6 个错误用例 | ✅ |
| A2.5 | 级联与特异性 | `class_selectors_beat_element_selectors`、`later_rules_win_on_equal_specificity` | ✅ |
| A2.6 | 继承规则 | `paragraph_inherits_font_and_gets_its_indent`、`inline_styles_inherit_and_override` | ✅ |
| A2.7 | `StyledDoc` 无生命周期、按 NodeId 排序 | `node_styles_are_sorted_and_complete`（编译期无生命周期 + 运行期排序） | ✅ |
| A2.8 | 样式去重 | `computed_styles_are_deduplicated` | ✅ |
| A2.9 | 页面几何 | `page_geometry_is_resolved_in_em_units`（内容框 = 页宽 − 双边距） | ✅ |
| A2.10 | `templateHash` 稳定且敏感 | `template_hash_is_sensitive_to_the_declarations` | ✅ |
| A2.11 | `styleHash` 只随样式变化 | `style_hash_tracks_the_resolution`（改内容不变，改模板变） | ✅ |
| A2.12 | 模板不能写引擎旋钮 | Gate A 静态检查 + `reject_unknown` | ✅ |
| A2.13 | `no_std` + 依赖白名单 | Gate A：`cargo tree` 只允许 3 个名字 | ✅ |
| A2.14 | wasm32 可编译 | `gates.sh` 的 wasm 检查包含 `soma-template` | ✅ |

## 4. 测试

| suite | 数量 |
|---|---:|
| `a2_template.rs` | 10 |
| `a2_units.rs` | 7 |
| `a2_resolve.rs` | 11 |
| **`soma-template` 小计** | **28** |

工作区总数 **187**（A1 后 159 + 28）。

## 5. 已知限制（已记账）

- YAML 子集不含流式集合、锚点、多行标量、`@` 规则（D42）。
- 无模板继承 / `@import`（D43）。
- 百分比长度不支持；需要参照系的单位先不做（D44）。
- 命名槽位与封面布局未接（`GeneratedBlock` 已在 DocIR，模板侧待 A6）（D45）。
- `Section.page_template` 已进 DocIR，但模板侧还没有页面母版定义（A4）（D46）。
- 选择器没有组合子；只做「全部部分都匹配」（D47）。

## 6. 复现

```sh
cargo test -p soma-template
bash scripts/gate-a.sh
bash scripts/gates.sh
```
