# 模板规范（`soma-template`）

状态：**A2 已落地，E4 扩展，表格规则、单元上下文、目录、章节起始母版、封面、自动编号、前置页、题注样式、索引、代码块面板与语法配色已加**
（schema `soma.template/9` / `soma.styles/7`）。
代码是权威，本文解释 DSL、单位与级联规则。

参考：`docs/architecture-v2.md` §7、ADR-0003（纯 YAML）、ADR-0009（语义/呈现边界）、
ADR-0010 §3（`StyledDoc` 为纯数据）、ADR-0014（标点/网格）、ADR-0015（页面母版）、
ADR-0019（表格规则与 `within`）、ADR-0031（封面与对齐）、ADR-0032（编号、前置页、目录扩展）、
ADR-0033（题注、可换行目录、书签与索引）、ADR-0088（Plex 默认模板）、
ADR-0089（等宽族）、ADR-0090（代码块面板）、ADR-0091（语法配色）。

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
- 单行 flow 序列 `[a, b]` 与 flow 映射 `{key: value}`（可嵌套，可出现在块位置）；
- `#` 注释（引号内不算注释）；
- 空值（后面跟更深缩进就是嵌套块，否则是 null）。

不支持：锚点/别名、标签、多行标量、`---` 分档、Tab 缩进。

> 因为 `#` 是注释，颜色请写 `color: 101010` 或 `color: "#101010"`（加了引号就必须带 `#`）。

## 3. 单位

| 单位 | 含义 | 解析基准 |
|---|---|---|
| `em` | 当前字号 | 当前节点的字号 |
| `pt` | 点 | 根字号（`document` 的 `font-size`） |
| `mm` / `cm` / `in` | 物理长度 | 根字号 |
| `%` | 精确比例 | 页面四边距按页宽；`line-height` 按解析后的字号；无基数的位置报错 |
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
  start-master: chapter    # 章节起始页用的母版，可选
  front-matter: continuous # continuous | roman | hidden
index:
  title: 索引               # 可选；省略或留空 = 无标题
numbering:
  headings: none           # none | chinese-chapter | arabic-dotted
  depth: 2                 # 1–3，编号到几级
  figures: "图 {n}"         # 可选；必须含 {n}
  tables: "表 {n}"
footnote:
  numbering: document      # document | page | section
table:
  width: justify                # justify | natural
  rule-width: 0                 # 行之间；0 = 不画
  header-rule-width: 0.6pt      # 表头下方
  outer-rule-width: 0.8pt       # 表格上下两条
  column-rule-width: 0          # 列之间
  outer-column-rule-width: 0    # 表格左右两条（框线）
  rule-style: solid             # solid | dashed
  rule-color: "#333333"         # 可省略
syntax:                     # 可选：代码 token 颜色，缺省类不配色
  comment: "#575757"
  keyword: "#9d00ec"
  string: "#008200"
  number: "#b21e00"
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

`table:` 整段可省略。`width: justify`（默认）让列按比例铺满版心；
`width: natural` 让表格保持内容宽，窄于版心时居中，并在每列内加
`0.5em` 的左右内边距（类似 LaTeX 的 `\tabcolsep`）。
Markdown 竖线表格的分隔行里，破折号越多那一列越宽；全列破折号数量相同
表示没有偏好，仍按内容量宽。普通模板在 `justify` 下把提示当版心比例，
`natural` 下用它把表格加宽（不会把列压到比内容更窄）。

省略 `table:` 或线宽全为 `0` 时不画线，表格几何与旧版一致
（页面 canonical 只差 schema 版本号）。
五条线宽是全表统一参数（暂不支持按表/按行覆盖），`pt`/`em`/`mm` 都可以，
负值报错。每条线各自独立：**没有隐式回退**，三线表只写
`outer-rule-width` + `header-rule-width`，网格表再把 `rule-width` 与
`column-rule-width`（以及左右框 `outer-column-rule-width`）写上。
`rule-style: dashed` 用固定 3:2 的线段/间隔，线段长度随线宽缩放，断点从每段线起点算。
跨行/跨列单元会挡住穿过它的线：横线按列分段，竖线按行分段；表头带单独切分，
续页重画的表头自带左右框。横线上下各留 `1/4em` 的空（ADR-0084），
表体行保持正常行高；表格整体的 `space-before/space-after` 来自
`element: table` 规则。左右内边距仍用 `within: cell` + `indent-left/right`
（ADR-0019）。

脚注区由引擎留出一条 `1/32em` 分隔线、`1/4em` 间距和 `1.5em` 编号列；
`footnote:` 只决定编号策略，`element: footnote` 只决定字号、颜色和行距（ADR-0083）。

`masters:` 定义命名页母版；省略时只有 `page:` 这一套（名为 `default`）。
每套母版自己写 `header` / `footer` / `header-text` / `folio`（没有隐式继承），
名字唯一且不能叫 `default`，页眉/页脚高度必须能放进页边距。

- `page.start-master: <name>` 指定**章节起始页**用的母版（名字或 `default`）；
  页以一个 `Section` 标记后的第一个内容块、或一个一级标题开头时用它，正文版心不变。
  章节页通常就是「去掉页眉」的那一套。
- `cover:` 生成封面页；`master` 指定封面用的母版（通常是「无页眉无页脚」那套）。
  内容来自文档元数据 `meta.cover`，缺失时回退到 `meta.title`/`authors`/`date`；
  四行分别用 `element: coverTitle`、`coverSubtitle`、`coverAuthor`、`coverDate`。
  封面是**一个原子块**：放不下就是溢出页，不会溢到第二页。
- `toc:` 打开自动目录：`depth`（1–3，默认 2）决定收录到几级标题，
  `title` 是目录标题（留空则没有标题）。条目样式用 `element: toc`，
  标题样式用 `element: tocTitle`，默认继承正文。
  目录占自己的页，之后强制分页；页码由布局反解，最多迭代 4 遍
  （报告在页面结果的 `toc` 字段里，`stable: false` 表示未收敛）。
  `leaders`（默认 `true`）在条目与页码之间生成引导点；
  `figures` / `tables` 各给一个列表标题值：（省略 = 不生成该列表；
  空字符串 `""` = 生成无标题列表）。列表条目使用题注文本（含编号）和题注所在页的
  打印页码，排在章节条目之后。

索引（`index:`）：出现该段才生成索引；条目来自文档里的 `\index{...}`（LaTeX）或
`[[index:term]]`（Markdown）。词条按 Unicode 码位排序，重复词条合并页码，
索引另起一页，样式用 `element: indexEntry` / `indexTitle`。词条内的标记在表格单元
与脚注定义中不收集（见 D81）。

题注（`figureCaption` / `tableCaption`）：图题在图下方，表题在表上方并
`keep-with-next`；题注内的强调继承题注字号，不回到正文字号。

编号（`numbering:`）：

- `headings: chinese-chapter` 生成 `第一章` / `第一节` / `一、`；
  `arabic-dotted` 生成 `1` / `1.2` / `1.2.3`。`none` 不编号。
- `depth` 同时限制编号级别和点号编号的层数；更深的标题保持原样。
- 编号由布局生成，**不写进 DocIR**；目录条目拿到的就是带编号的标题文本。
- 图/表计数全文档独立；`{n}` 作文本替换，无题注的浮动体不占号。
- 编号前缀与标题之间是一个正常的空格边界（宽度来自度量表），
  不是硬编码间距。

前置页（`page.front-matter:`）：

- `continuous`：物理页从 1 连续编号。
- `roman`：封面/目录用小罗马数字 `i, ii, iii…`，正文从 `1` 重新开始。
- `hidden`：前置页不印页码，正文从 `1` 开始。

前置页 = 垂直流头部**只含生成内容**（封面、目录）的连续页；`Page.number` 仍是物理页，
`Page.printed` 才是印在页上的标签。标签为空时不生成页脚块，所以 `hidden` 与
`footer: 0` 的母版都不需要特例。`{pages}` 在 `roman`/`hidden` 下等于正文页数。

DocIR 的 `Section.page_template: <name>` 选择母版；换母版会强制新页
（节首不会和上一节挤在同一页），文档开头的节直接作用于第 1 页；未知名字是布局错误。
母版只改页边带，**版心几何仍是全局一套**，所以不影响分页与页数。

## 5. 选择器与级联

- 选择器可以是 `element`、`role`、`class`/`classes`、`within`、`level`、`parent`、
  `ancestors` 的任意组合；**所有给出的部分都必须匹配**。
- `parent: cell` 匹配直接父级；`ancestors: [table, cell]` 匹配从外到内的祖先链
  （不要求相邻）。`within` 仍表示“某一层祖先”。
- `level: 1`–`9` 匹配标题（`heading`）与节（`section`）的级别；其他元素一律
  `level: null`。可以只写 `level`（不带 `element`）。
- `within: cell` 表示“某个块级祖先的元素名是 `cell`”，用来把单元内的段落和正文段落
  分开。可用值同 `ELEMENTS`，写错是错误。
- 元素名来自 DocIR 节点种类（`paragraph`、`heading`、`strong`、`table`、`cell`、`footnote`…），
  完整清单在 `soma_template::ELEMENTS`；写错名字是错误，不会静默不匹配。
- 特异性从低到高：`element` < `role` / `within` < `level` < 每个 `class`。
- 同特异性按**声明顺序**，后面的覆盖前面的。
- 规则内属性按固定顺序应用：`font-size` → `line-height` → 其余，因此 `em` 长度看到的是新字号。
- 同一规则内同属性重复声明时，最后一条生效。

继承：

| 继承 | 不继承（每个节点重置） |
|---|---|
| `font-size`、`line-height`、`align`、`family`、`color`、`weight`、`slant`、`small-caps` | `first-indent`、`indent-left`、`indent-right`、`background`、`padding-top`、`padding-right`、`padding-bottom`、`padding-left`、`space-before`、`space-after`、`space-before-plus`、`space-before-minus`、`space-after-plus`、`space-after-minus`、`widow-lines`、`orphan-lines`、`hanging-punctuation`、`hanging-cap`、`baseline-grid`、`underline`、`keep-with-next`、`page-break-before` |

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
| `background` | `#rrggbb` / `rrggbb` | 块背后的填充色；默认不画（ADR-0090） |
| `padding-top` / `padding-right` / `padding-bottom` / `padding-left` | length | 块内容与面板边缘的距离；负值报错（ADR-0090） |
| `space-before` / `space-after` | length | 块间距 |
| `space-before-plus` / `space-before-minus` | length | 可拉伸/可收缩的垂直胶水 |
| `space-after-plus` / `space-after-minus` | length | 同上，块后 |
| `widow-lines` / `orphan-lines` | `0`–`9` | 段末/段首至少保留的行数 |
| `hanging-punctuation` | `true` / `false` | 行尾标点悬挂进右边距（默认 `false`；行末标点的光学对齐默认由断行期压缩完成，ADR-0036） |
| `hanging-cap` | length | 悬挂上限（默认 `0.5em`） |
| `baseline-grid` | length | 行高与间距量化到网格；`0` 关闭 |
| `align` | `justify` / `left` / `center` / `right` | `justify` 两端对齐；`left` 只按自然宽度摆放；`center`/`right` 在断行后整行平移（ADR-0031） |
| `family` | 文本 | 字体族 token：`serif`（默认）、`sans`、`mono`（ADR-0089） |
| `color` | `#rrggbb` / `rrggbb` | 打包成 `0xRRGGBB` |
| `weight` | `normal` / `bold` | |
| `slant` | `normal` / `italic` | |
| `underline` / `small-caps` / `keep-with-next` / `page-break-before` | `true` / `false` | |

布尔值也接受 `yes` / `no`。

### 6.1 代码块面板

`background` 与四个 `padding-*` 是普通声明，不限于代码块，但第一个使用者是
`element: codeBlock`。布局在分页前就计算出面板高度：

- 面板宽度 = 版心宽度；
- 面板高度 = `padding-top` + 各行行高 + `padding-bottom`；
- 每行 `y` 从 `padding-top` 起算，每个字形的 `x` 右移 `padding-left`；
- 代码行不换行；过长命令从行尾溢出，而不是被拆成不可复制的两段。

因此三种渲染器只画背景矩形和已有坐标，不再各自计算内边距。Plex 参考模板的
值取自 Rust book：`background: "#f6f7f6"`，水平 `0.8em`（代码字号）、
垂直 `0.5em`，等宽族 `mono`。

### 6.2 语法配色

顶层 `syntax:` 是**可选的**逐个 token 类的颜色表：

```yaml
syntax:
  comment: "#575757"
  keyword: "#9d00ec"
  string: "#008200"
  number: "#b21e00"
  type: "#b21e00"
  function: "#0030f2"
  variable: "#d70025"
  meta: "#b21e00"
  literal: "#b21e00"
```

缺省的类保持代码块自己的 `color`。只有非空表才写进 canonical 模板；旧模板的
`templateHash` 不变。着色只在 `codeBlock` 内进行，语言由围栏 info string 的
第一个 token 选择（`rust,ignore` 仍选 `rust`）；未知语言按纯代码排版。
支持 `rust`、`c`、`cpp`、`shell`、`markdown`、`latex`、`python`、
`javascript`/`typescript`、`json`、`yaml`、`toml`、`html`、`css`、`sql`、
`go`、`java`、`kotlin`、`swift`、`csharp`、`ruby`、`php`、`lua`、`haskell`。
词法规则见 ADR-0091。

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
| `template_hash()` | `soma.template/9` canonical JSON：名称、版本、页面（含母版、起始母版、前置页编号）、脚注、目录（含引导点与图/表目录）、索引、封面、编号、表格线宽与 `width`、全部规则与声明 |
| `style_hash()` | `soma.styles/7` canonical JSON：模板 hash + 解析后的页面/样式/目录/索引/封面设置/节点映射 |

两者都是 FNV-1a 64，与 `paramHash` / `docHash` 同一原语。

## 9. 校验与诊断

`TemplateError { line, column, path, reason }`，`Display` 形如
`12:7: styles[2]: unknown style property`。

覆盖：未知键/属性/元素、重复键、重复 class、空选择器、缺失必填键、非法长度、
非法颜色、非法布尔、非法 page size、根字号缺失或非绝对单位、空规则。

## 10. 明确不做（v1）

- 伪类、`@` 规则、跨文档选择器；`parent`/`ancestors` 已覆盖直接父级与祖先链；
- 锚点/别名、多行标量（YAML 子集边界，见 §2）；
- 命名槽位与封面布局（`GeneratedBlock` 已有，模板侧待 A6）；
- 目录的引导点（dot leaders）、按级自定义编号格式、图目录/表目录/索引
  （`GeneratedKind` 其余变体仍无生成器）（D73）；
- 字体族映射到实际字体文件（渲染器/度量层职责）。

### 10.1 模板继承（数据级）

`parse_template_inherit(base, overlay)` 在纯数据层合并两份 YAML：
`styles`/`masters` 按 base→overlay 追加，其余映射递归合并、标量覆盖；
`base` 必须自身合法。它不读文件，宿主负责加载文本；没有 `@import` 文件语法。

## 11. 参考设计：`examples/templates/plex.yaml`

这份模板把 pandoc 的 `soma.latex` 页面设计搬成纯 YAML：A4、1in 页边距、
IBM Plex Sans SC 正文 10pt / 1.44 行高、零首行缩进 + 20pt 段距、LaTeX article
的标题尺寸、斜体引文、booktabs 三线表、拼音索引，以及 Rust book 的代码块
面板与 mdBook Rust 语法配色。它也是 CLI 的**内置默认模板**（ADR-0088）；
旧默认已归档为 `examples/templates/latex-article.yaml`。

运行 `scripts/plex.sh` 会用本机 Plex 面与 Noto Sans Mono 生成度量表
（不随仓库分发字体），并排出：

- `examples/templates/out/plex.pdf`：`examples/book/full.tex` 的全能力样张；
- `examples/templates/out/code.pdf`：`examples/templates/code.md` 的
  Rust/C/C++/Shell/Markdown/LaTeX/Python/JS/JSON/HTML 代码块样张。

参考模板里仍有两项是近似而非等价：带框引文只有缩进 + 斜体（模板没有
border）、`\maketitle` 的首页
标题块变成独立封面页。`tikz`/`pgfplots`/`forest` 不是模板能力：
`tikzpicture` 由 `soma-tikz` 作为显示对象解析核心 PGF 绘图语言
（ADR-0092，子集见 `docs/tikz.md`），其余 TikZ 库语法走可见的源码回退。
代码块背景与内边距已由 ADR-0090 补齐，三线表列宽
由 ADR-0087 按内容/破折号分配。详见 ADR-0081/0087/0088/0090/0091/0092。
