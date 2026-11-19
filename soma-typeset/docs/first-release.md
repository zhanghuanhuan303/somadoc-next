# 首发成品：`examples/book`

一份用 `soma render` 端到端排出来的书：封面、目录、六个章节、一个公式、一张三线表、
一条脚注。内容取自仓库里自己写的中文技术散文，模板就是一份普通的设计稿
（`examples/book/book.yaml`），没有任何针对这一份文档的代码。

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
| 页面 | 9（封面 1 + 目录 1 + 正文 7） |
| 块 | 69（段落 47、标题 6、公式 1、表格线 3、单元 12） |
| 行 | 123 |
| 目录 | 6 条，2 遍收敛（`passes: 2`、`stable: true`） |
| 越界页 / 降级块 | 0 / 0（无 `emergency`、无 `overfull`） |
| 脚注 | 1 |
| canonical hash | `c190a96c29ffc223` |

版心高度 188,992 单位（A5 210mm − 上下 36mm）；正文页填充 94k–157k，
封面 114,611，目录 57,274。

## 这一轮补上的缺口

做成品的过程中暴露了三处「看起来能用、实际不能用」的地方，都已修掉：

1. **`align: center/right` 只有模板层，没有布局层。**
   模板早就接受四个值，但内核拒绝非 justify，所以任何居中排版都会在布局时报错。
   现在内核接受 `Left`（盒子按自然宽度摆放、断点仍由带代价的模型给出），
   `Center`/`Right` 由布局在断行后按 `(target − natural)` 平移每一行。
   封面、章题、表题因此都能居中。见 ADR-0031 §1。

2. **没有封面。** `meta.cover` 从 A1 就存在，但没有任何一层会排版它。
   模板加 `cover:`（可选 `master`），布局用 `coverTitle`/`coverSubtitle`/
   `coverAuthor`/`coverDate` 生成一张**原子**的封面页：要么整页放下，要么标成溢出，
   绝不允许溢到第二页。数据取自 `meta.cover`，缺失时回退到 `meta.title`/
   `authors`/`date`。见 ADR-0031 §2。

3. **表格单元被两端对齐。** 单元继承正文的 justify，两三个字的单元会被拉到列宽
   （实测 143% 拉伸，落进 emergency）。这一份模板现在用
   `element: paragraph` + `within: cell` + `align: left`，降级块从 1 变成 0。
   这不只是模板写法问题：它说明「单元默认应该是一个可声明的选择」，模板现在能写了。

## 仍然明显的缺口

按对「像一本书」的影响排序：

| # | 缺口 | 现状 | 影响 |
|---|---|---|---|
| 1 | **选择器看不到标题级别** | `element: heading` 命中所有级别；这份书只用了 `\section` 来规避 | 二级标题无法单独设字号/间距（D75） |
| 2 | **页码不区分前置页** | 封面无页码（靠母版），但目录拿到了「2」 | 正式书通常前置页用小罗马数字或不计（D76） |
| 3 | **目录没有引导点** | 标题与页码之间是空白 | 长目录不易对齐视线（D73） |
| 4 | **没有自动编号** | 章号靠源文件手写「第一章」 | 增删章节要手工改号（D77） |
| 5 | **章首页只有「无页眉」** | 模板能做到，但没有扉页/章前空白页的概念 | 章页留白全靠 `space-before` |
| 6 | **表格没有行高控制、没有单元格内边距** | 单元贴线，行距就是行高 | 密排表格显得挤 |
| 7 | **脚注只支持整块移页** | 脚注不能跨页拆 | 超长脚注会把整块推到下一页 |

这七条都记在 `docs/debt.md`，没有一条阻塞「用现在的引擎排一本书」。

## 验收

```sh
scripts/book.sh                      # 生成三种格式
cargo run -p soma-cli -- check \
  --input examples/book/book.tex --template examples/book/book.yaml
cargo test -p soma-cli               # 其中包括这本书的回归
```

`check` 应该报 9 页、0 溢出页、0 降级块；`scripts/gates.sh` 仍然全绿。
