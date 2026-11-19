# 模板规范（`soma-template`）

状态：**A2 已落地，E4 扩展，表格规则与单元上下文已加**（schema `soma.template/4` / `soma.styles/2`）。
代码是权威，本文解释 DSL、单位与级联规则。

参考：`docs/architecture-v2.md` §7、ADR-0003（纯 YAML）、ADR-0009（语义/呈现边界）、
ADR-0010 §3（`StyledDoc` 为纯数据）、ADR-0014（标点/网格）、ADR-0015（页面母版）、
ADR-0019（表格规则与 `within`）。

---

## 1. 模板是什么

| 问题 | 答案 |
|---|---|
| 它拥有什么 | 页面几何、样式规则、脚注编号、表格线宽 |
| 它不拥有什么 | 内容结构、断行参数、分页算法、字体解析 |
| 输入 | 一份 YAML 文本 |
| 输出 | `Template`（已校验、可 hash） → `resolve(doc, template)` → `StyledDoc` |
| 依赖 | 只依赖 `soma-core`（`Width`/`StyleId`）与 `soma-doc`（节点与 `NodeId`） |

硬约束：

1. **模板不能写引擎旋钮**（`tolerance`、`badness_scale`、`max_kp_atoms`、`preset`）。
   出现即报错；Gate A 也有静态检查。
2. **单位全部精确**：十进制解析成有理数，最终落到 1/4096 em，除法向零截断。
   全链路没有浮点。
3. **未知属性/未知元素/未知键都是错误**，不静默忽略。

## 2. YAML 子集

只支持下列语法，其余一律报错（带行列号）：

- 块映射 `key: value`，用空格缩进嵌套；
- 块序列 `- value` / `- key: value`；
- 普通标量、单引号 `'...'`（`''` 表示一个引号）、双引号 `"..."`（`\"`、`\\`、`\n`、`\t`）；
- `#` 注释（引号内不算注释）；
- 空值（后面跟更深缩进就是嵌套块，否则是 null）。

不支持：流式集合 `[a, b]` / `{a: b}`、锚点/别名、标签、多行标量、`---` 分档、
Tab 缩进。

> 因为 `#` 是注释，颜色请写 `color: 101010` 或 `color: "#101010"`（加了引号就必须带 `#`）。

## 3. 单位

| 单位 | 含义 | 解析基准 |
|---|---|---|
| `em` | 当前字号 | 当前节点的字号 |
| `pt` | 点 | 根字号（`document` 的 `font-size`） |
| `mm` / `cm` / `in` | 物理长度 | 根字号 |
| 裸数字 | 仅允许 `0` | — |

换算常量：`1in = 72pt`、`1mm = 360/127 pt`、`1cm = 3600/127 pt`。
除法向零截断，例如 `1/3em` 在 4096 单位下是 `1365`。

**根字号必须用绝对单位**写在 `document` 规则里，否则模板报错——`1em` 没有定义。

## 4. 模板结构

```yaml
name: book                 # 必填，token
version: "1"               # 可选
page:
  size: A4                 # A4 | A5 | Letter | custom
  width: 210mm             # 仅 custom 时必填
  height: 297mm
  margin: 20mm             # 可选；四边统一的简写
  margin-top: 25mm         # 可单独覆盖
  margin-bottom: 25mm
  margin-left: 20mm
  margin-right: 20mm
  header: 6mm              # 页眉带高度，位于上边距内
  footer: 8mm              # 页脚带高度，位于下边距内
  header-text: running-title   # 或字面量字符串
  folio: "第 {page} 页 / 共 {pages} 页"
footnote:
  numbering: document      # document | page | section
table:
  rule-width: 0                 # 行之间；0 = 不画
  header-rule-width: 0.6pt      # 表头下方
  outer-rule-width: 0.8pt       # 表格上下两条
  column-rule-width: 0          # 列之间
  outer-column-rule-width: 0    # 表格左右两条（框线）
  rule-style: solid             # solid | dashed
  rule-color: #333333         # 可省略
styles:                    # 必填，非空序列
  - selector:
      element: paragraph
      role: body
      class: lead
      classes: [first, wide]
    declarations:
      first-indent: 2em
  - selector:
      element: paragraph
      within: cell         # 只匹配单元里的段落
    declarations:
      first-indent: 0
      indent-left: 0.25em
```

页面默认：A4，四边 20mm。页边距属于模板（呈现）；只有 `window` 属于引擎配置。

`table:` 整段可省略；省略或全为 `0` 时不画线，表格几何与旧版一致
（页面 canonical 只差 schema 版本号）。
五条线宽是全表统一参数（暂不支持按表/按行覆盖），`pt`/`em`/`mm` 都可以，
负值报错。每条线各自独立：**没有隐式回退**，三线表只写
`outer-rule-width` + `header-rule-width`，网格表再把 `rule-width` 与
`column-rule-width`（以及左右框 `outer-column-rule-width`）写上。
`rule-style: dashed` 用固定 3:2 的线段/间隔，线段长度随线宽缩放，断点从每段线起点算。
跨行/跨列单元会挡住穿过它的线：横线按列分段，竖线按行分段；表头带单独切分，
续页重画的表头自带左右框。

## 5. 选择器与级联

- 选择器可以是 `element`、`role`、`class`/`classes`、`within` 的任意组合；
  **所有给出的部分都必须匹配**。
- `within: cell` 表示“某个块级祖先的元素名是 `cell`”，用来把单元内的段落和正文段落
  分开。可用值同 `ELEMENTS`，写错是错误。
- 元素名来自 DocIR 节点种类（`paragraph`、`heading`、`strong`、`table`、`cell`、`footnote`…），
  完整清单在 `soma_template::ELEMENTS`；写错名字是错误，不会静默不匹配。
- 特异性从低到高：`element` < `role` / `within` < 每个 `class`。
- 同特异性按**声明顺序**，后面的覆盖前面的。
- 规则内属性按固定顺序应用：`font-size` → `line-height` → 其余，因此 `em` 长度看到的是新字号。
- 同一规则内同属性重复声明时，最后一条生效。

继承：

| 继承 | 不继承（每个节点重置） |
|---|---|
| `font-size`、`line-height`、`align`、`family`、`color`、`weight`、`slant`、`small-caps` | `first-indent`、`indent-left`、`indent-right`、`space-before`、`space-after`、`space-before-plus`、`space-before-minus`、`space-after-plus`、`space-after-minus`、`widow-lines`、`orphan-lines`、`hanging-punctuation`、`hanging-cap`、`baseline-grid`、`underline`、`keep-with-next`、`page-break-before` |

布局属性不继承，所以单元内边距不是“继承 body 的缩进”，而是显式写
`element: paragraph` + `within: cell` + `indent-left/indent-right`；
这样正文段落和单元段落可以各自独立设置。

## 6. 属性白名单

| 属性 | 类型 | 说明 |
|---|---|---|
| `font-size` | length | 根规则必须绝对单位；子节点可用 `em` |
| `line-height` | 无单位倍数或 length | 解析成绝对行距 |
| `first-indent` | length | 首行缩进 |
| `indent-left` / `indent-right` | length | 块级左右缩进 |
| `space-before` / `space-after` | length | 块间距 |
| `space-before-plus` / `space-before-minus` | length | 可拉伸/可收缩的垂直胶水 |
| `space-after-plus` / `space-after-minus` | length | 同上，块后 |
| `widow-lines` / `orphan-lines` | `0`–`9` | 段末/段首至少保留的行数 |
| `hanging-punctuation` | `true` / `false` | 行尾标点悬挂进右边距 |
| `hanging-cap` | length | 悬挂上限（默认 `0.5em`） |
| `baseline-grid` | length | 行高与间距量化到网格；`0` 关闭 |
| `align` | `justify` | **model v1 只实现 justify**；其他值在 resolve 时报错 |
| `family` | 文本 | 字体族 token，由渲染器解析 |
| `color` | `#rrggbb` / `rrggbb` | 打包成 `0xRRGGBB` |
| `weight` | `normal` / `bold` | |
| `slant` | `normal` / `italic` | |
| `underline` / `small-caps` / `keep-with-next` / `page-break-before` | `true` / `false` | |

布尔值也接受 `yes` / `no`。

## 7. `StyledDoc`（纯数据）

`resolve(&Document, &Template) -> StyledDoc`，产物无生命周期：

```rust
pub struct StyledDoc {
    node_style: Vec<(NodeId, StyleId)>, // 按 NodeId 升序
    table: StyleTable,                  // 去重后的 ComputedStyle
    page: ResolvedPage,                 // em 单位
    footnote: FootnoteNumbering,
    table_rules: TableRules,            // 模板 table: 段解析后的线宽
    template_hash: TemplateHash,
}
```

- `style_of(NodeId)` 二分查找；`table().styles()` 是去重表。
- `ComputedStyle` 全部是整数/布尔/字符串，可比较、可 hash。
- `style_hash()` 覆盖模板 hash、页面几何、样式表与 `NodeId` 映射。
- 内容改动但样式不变时 `style_hash` 不变；模板改动会改变它。
- 布局层只读 `StyledDoc`，不读模板文件（架构 §4）。

## 8. 两个 hash

| Hash | 覆盖 |
|---|---|
| `template_hash()` | `soma.template/4` canonical JSON：名称、版本、页面（含母版）、脚注、表格线宽、全部规则与声明 |
| `style_hash()` | `soma.styles/2` canonical JSON：模板 hash + 解析后的页面/样式/节点映射 |

两者都是 FNV-1a 64，与 `paramHash` / `docHash` 同一原语。

## 9. 校验与诊断

`TemplateError { line, column, path, reason }`，`Display` 形如
`12:7: styles[2]: unknown style property`。

覆盖：未知键/属性/元素、重复键、重复 class、空选择器、缺失必填键、非法长度、
非法颜色、非法布尔、非法 page size、根字号缺失或非绝对单位、空规则。

## 10. 明确不做（v1）

- 选择器组合子（更深的后代、子代、伪类）与 `@` 规则（`within` 只覆盖“某一层祖先”）；
- 百分比长度（需要知道参照系，先不做）；
- 模板继承 / `@import`；
- 命名槽位与封面布局（`GeneratedBlock` 已有，模板侧待 A6）；
- 每章切换页面的自动规则（`Section.page_template` 已存在，模板侧待 A4）；
- 字体族映射到实际字体文件（渲染器/度量层职责）。
