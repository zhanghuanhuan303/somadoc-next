# HTML 前端（`soma-html`）

状态：**v1 已落地**（schema 不变：输出 `soma.doc/2`）。
代码是权威；本文说明支持的语义、降级方式和限制。

参考：`docs/architecture-v2.md` §3、ADR-0004（语法前端）、ADR-0009（语义/呈现边界）、
ADR-0024（本前端）。

---

## 1. 入口

```rust
use soma_html::{parse_html, parse_html_with_meta};

let document = parse_html("<h1>标题</h1><p>正文 <strong>重点</strong>。</p>");
```

- 零依赖、`no_std + alloc`、`#![forbid(unsafe_code)]`。
- 只产出 DocIR；不认识模板、样式、页面或断行。
- `<html lang="zh-Hans">` 会在调用方未设置时填充 `DocumentMeta.language`。
- 不合法/未闭合标签不会报错，尽量保留文本。

## 2. 支持的语义

| HTML | DocIR |
|---|---|
| `h1`–`h6` | `Block::Heading(level)` |
| `p` | `Block::Paragraph` |
| `ul` / `ol`（`start`） | `Block::List(ordered, start)` |
| `dl` / `dt` / `dd` | `Block::List`（无序；`dt`/`dd` 按源顺序扁平为条目） |
| `li` | `ListItem`（可含多个块） |
| `blockquote` | `Block::BlockQuote` |
| `pre`（`class="language-x"`） | `Block::CodeBlock(lang)` |
| `hr` | `Block::ThematicBreak` |
| `table` / `thead` / `tbody` / `tr` / `th` / `td` | `Block::Table`（`colspan`/`rowspan`，表头前缀） |
| `figure` + `img` + `figcaption` | `Block::Figure` + caption |
| `div`/`section`/`article`/`main`/`header`/`footer`/`nav`/`aside` | 透明容器（子节点成为块） |
| `strong`/`b`、`em`/`i`、`u`/`ins`、`s`/`del`/`strike` | 行内容器 |
| `code`/`kbd`/`samp`/`var` | `Inline::Code` |
| `a[href]` | `Inline::Link` |
| `img[src,alt]` | `Inline::Image` 或 `Figure` |
| `br` | `Inline::LineBreak(Hard)` |
| `sup` / `sub` | `Inline::Superscript` / `Subscript` |
| `ruby` + `rt` | `Inline::Ruby(base, annotation)` |

属性只保留语义：`class` → `Attrs.classes`、`role` → `Attrs.role`、
`id` → `Attrs.anchor`。`style`、`on*`、`data-*` 一律丢弃。

## 3. 降级与恢复

- 未知标签：**保留子节点**，文本不丢；不产生 `RawInline`。
- `script` / `style`：整段丢弃（内容与标签都不进入 DocIR）。
- 注释、`<!doctype>`、`<?...?>`：丢弃。
- 未闭合标签在 EOF 关闭；游离的闭标签忽略。
- `p`/`li`/`tr`/`td`/`th` 在新兄弟开始时自动闭合。
- 字符引用：常见命名实体（`amp lt gt quot apos nbsp hellip mdash ndash
  ldquo rdquo lsquo rsquo middot copy`）+ 十进制/十六进制数字引用。

## 4. 表格规范化

- 列数 = 考虑 `colspan`/`rowspan` 后最宽的一行。
- 行被补齐到列数，`colspan`/`rowspan` 被夹到网格内。
- 前导 `thead` 或全 `th` 行标为 `Row.header`；表头必须是前缀。
- 输出保证通过 `Document::validate()`（占位网格不重叠）。

## 5. 限制（见 `docs/debt.md` D69）

- 不执行 CSS、脚本、表单、框架；`style` 属性不进入 DocIR。
- `<caption>` 成为表题；`<colgroup>`/`<col>` 的 `width`/`span` 成为列的相对宽度（ADR-0065）。不映射脚注标记、`<math>`。
- 实体只覆盖常见集合，完整 HTML5 实体表未做。
- 不保留源位置（`SourceSpan` 为空），错误诊断只有“能解析/不能解析”。

## 6. 测试

`crates/soma-html/tests/html.rs` 覆盖：

- 标题/段落/强调/链接/上下标映射；
- 列表、引用、代码块、`hr`；
- 表格的 colspan/rowspan/表头/补格，并通过 `validate`；
- 未知标签保留文本、`script`/`style` 丢弃；
- 实体解码；
- 畸形 HTML 不 panic 且保留文本；
- `lang` 继承、canonical 确定性；
- HTML → 模板 → 分页 → HTML 渲染的端到端。
