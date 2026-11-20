# 首发成品：`examples/book`

一份用 `soma render` 端到端排出来的书：封面、目录（章、节、图、表）、六个章节、
三个小节、一张插图、一张三线表、一个公式、一条脚注、一个索引。内容取自仓库里自己写的
中文技术散文，模板就是一份普通的设计稿（`examples/book/book.yaml`），没有任何针对
这一份文档的代码。

```sh
scripts/book.sh
# -> examples/book/out/book.html, book.svg, book.json
```

生成物不入库（`.gitignore` 里的 `examples/book/out/`）：每次改动引擎或模板都应该重新生成，
而不是把上一次的输出当成基准。回归由 `crates/soma-cli/tests/cli.rs` 里的
`the_example_book_renders_cover_contents_and_chapters` 守住。

## 现在的数字

| 项 | 值 |
|---|---|
| 页面 | 11（封面 1 + 目录 1 + 正文 8 + 索引 1） |
| 块 | 123 |
| 行 | 128 |
| 目录 | 10 条（6 章 + 3 节 + 索引）+ 图目录 1 条 + 表目录 1 条，3 遍收敛（`passes: 3`、`stable: true`） |
| 索引 | 7 条，3 遍收敛；按码位排序，重复词条合并页码 |
| 书签 | 10 条（全部标题 + 索引），带物理页与印字页码 |
| 页码 | 前置页 `i`、`ii`；正文 `1`–`9`（正文 8 页 + 索引 1 页） |
| 越界页 / 降级块 | 0 / 0（无 `emergency`、无 `overfull`） |
| 脚注 | 1 |
| canonical hash | `7953d75495e108c2` |

正文页填充 94k–172k 单位；封面 114,611，目录 122,682。索引页使用
`indexTitle`/`indexEntry` 样式，引导点对齐到页码盒。

## 第一轮补上的缺口（ADR-0031）

1. **`align: center/right` 只有模板层，没有布局层。**
   模板早就接受四个值，但内核拒绝非 justify，所以任何居中排版都会在布局时报错。
   现在内核接受 `Left`（盒子按自然宽度摆放、断点仍由带代价的模型给出），
   `Center`/`Right` 由布局在断行后按 `(target − natural)` 平移每一行。
   封面、章题、表题因此都能居中。

2. **没有封面。** 模板加 `cover:`（可选 `master`），布局用
   `coverTitle`/`coverSubtitle`/`coverAuthor`/`coverDate` 生成一张**原子**的封面页：
   要么整页放下，要么标成溢出，绝不允许溢到第二页。

3. **表格单元被两端对齐。** 单元继承正文的 justify，两三个字的单元会被拉到列宽
   （实测 143% 拉伸，落进 emergency）。模板用
   `element: paragraph` + `within: cell` + `align: left` 解决。

## 第二轮补上的缺口（ADR-0032）

1. **选择器看不到标题级别（D75）。** `Selector.level: 1..=9` 参与匹配与特异性；
   这份模板用 `level: 1` 让章题居中并另起一页、`level: 2` 让节题左对齐。
2. **没有自动编号（D77）。** 模板写 `numbering.headings: chinese-chapter`，
   布局生成 `第一章` / `第一节`；图与表用 `图 {n}` / `表 {n}`。
   编号不进 DocIR，增删章节不再需要改源文件。目录条目直接复用带编号的标题文本。
3. **前置页没有独立页码（D76）。** `page.front-matter: roman` 让封面和目录印
   `i`、`ii`，正文从 `1` 重新开始；`{pages}` 只数正文页。
4. **目录没有引导点、没有图/表目录（D73）。** `toc.leaders` 生成对齐到页码盒的
   引导点；`toc.figures` / `toc.tables` 在章节条目之后生成两个列表。

配套的 LaTeX 前端补了 `figure`/`table` 环境、`\includegraphics` 与 `\caption`，
并修掉 `\midrule` 少跳一个字节、把 `e` 漏进首格的旧 bug。

## 第三轮补上的缺口（ADR-0033）

1. **题注没有样式（D78）。** `figureCaption` / `tableCaption` 成为生成元素；
   图题在图下，表题在表上并 keep-with-next。题注里的强调继承题注字号。
2. **目录行不能换行（D79）。** 标题按文本编译，长标题在标题盒内换行，
   页码与引导点跟随最后一行。
3. **没有书签数据。** 页面结果新增 `outline`：每个标题的级别、文本、物理页、
   印字页码；PDF 后端直接写书签树，索引标题也作为一级书签参与导航。
4. **没有索引。** DocIR 新增 `Inline::Index`（`soma.doc/2`），LaTeX 用
   `\index{...}`、Markdown 用 `[[index:term]]`；模板写 `index:` 后，布局在
   正文末尾生成排序、去重、带页码的索引页。

## PDF 输出（ADR-0034）

`--format pdf` 把同一份 `PagedLayout` 写成 PDF：TrueType 稀疏子集
（Identity-H、ToUnicode）、逐字坐标、表格线与公式、书签树、PageLabels。
示例书用 MiSans（约 510 KB，只含用到的字形），11 页、10 个书签；
`scripts/book.sh` 先用 `tools/metrics-gen` 从 MiSans 生成度量表，再以
`--metrics` 交给 PDF 渲染，断行与 PDF 画出的字宽来自同一张脸。
图片嵌入与流压缩不在这一版（D82）；CFF 字体（Noto/Source Han 的 CID-keyed
与多数西文 OTF 的 name-keyed）已可嵌入。

## 仍然明显的缺口

按对「像一本书」的影响排序：

| # | 缺口 | 现状 | 影响 |
|---|---|---|---|
| 1 | **索引按码位排序** | 没有拼音/笔画排序 | 中文索引排序不合部分出版习惯（D80） |
| 2 | **索引标记覆盖有限** | 表格单元与脚注里的标记不收集；无 see/see also | 复杂索引需人工整理（D81） |
| 3 | **章首页只有「无页眉」** | 模板能做到，但没有扉页/章前空白页的概念 | 章页留白全靠 `space-before` |
| 4 | **表格没有行高控制、没有单元格内边距** | 单元贴线，行距就是行高 | 密排表格显得挤 |
| 5 | **脚注只支持整块移页** | 脚注不能跨页拆 | 超长脚注会把整块推到下一页 |
| 6 | **PDF 只支持 `glyf` 与 CFF** | 图片是占位框；无 tagged PDF；无流压缩 | 扫描版与无障碍交付需另做（D82） |

这六条都记在 `docs/debt.md`，没有一条阻塞「用现在的引擎排一本书」。

## 验收

```sh
scripts/book.sh                      # 生成三种格式
cargo run -p soma-cli -- check \
  --input examples/book/book.tex --template examples/book/book.yaml
cargo test -p soma-cli               # 其中包括这本书的回归
```

`check` 应该报 11 页、0 溢出页、0 降级块；`scripts/gates.sh` 仍然全绿。
