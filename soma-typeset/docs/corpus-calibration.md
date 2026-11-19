# 真实语料与代理标定（`corpus/zh`）

状态：**已建语料 + 已跑代理标定**。这不是 Gate Q：语料没有人工偏好标注，
结论只对“可观测版面量”负责。

## 1. 语料

`corpus/zh/` 五篇，共 39 个块、135 行、7 页（A5、10.5pt、1.6 行距）：

| 文件 | 文体 | 压测点 |
|---|---|---|
| `technical.md` | 技术说明 | 长段落、标题、强调、英文术语 |
| `notice.md` | 通知 | 列表、日期、数字、短段落 |
| `abstract.md` | 论文摘要 | 密集标点、分号、术语混排 |
| `editorial.md` | 评论 | 长句、引号、破折号、节奏 |
| `mixed.md` | 中英混排说明 | URL、行内代码、连字符、百分号 |

来源：为本仓库原创，不含第三方文本；按 `prd.md` 的样本政策以 CC-BY-4.0 提供。
规则见 `corpus/README.md`。**它不是质量标注语料，也没有评审意见。**

## 2. 默认配置（`default-v1`）

```
document,pages,paragraphs,lines,over_compress,emergency,continuation,widows,orphans,page_free_p95,page_free_max,jitter_p95,bdev_error_p95
abstract,1,6,24,0,0,0,0,0,0,0,290,0
editorial,2,7,30,0,0,1,0,0,423,423,314,0
mixed,1,6,24,0,0,0,0,0,0,0,541,0
notice,1,12,26,1,0,0,0,0,0,0,1646,0
technical,2,8,31,1,0,0,0,0,0,0,820,0
ALL,7,39,135,2,0,1,0,0,423,423,500,0
```

读法：

- 孤行/寡行 **0**；跨页段落 1 次（`editorial`）。
- 压缩越过 shrink 下限 **2 行 / 135 行**，集中在 `notice` 与 `technical`。
- 底部留白 P95 = 423‰（约 0.42 行），只出现在 `editorial` 的跨页处。
- `notice` 的 `jitter_p95 = 1646‰` 明显偏高：短段落 + 纯 CJK 时，行间松紧差异大。

## 3. 代理标定：tolerance × badness scale

按“先少压缩越界、再少留白、再少抖动、再少页数、最后偏好 preset 的 scale=100”排序：

```
tolerance,scale,over_compress,emergency,page_free_p95,jitter_p95,pages
1500,100,0,0,423,337,7
1500,50,0,0,423,337,7
1500,200,0,0,423,377,7
2000,100,2,0,423,500,7
2500,100,2,0,423,500,7
3000,100,2,0,423,500,7
...
```

结论（仅限本语料与代理指标）：

- `tolerance = 1500` 在**所有**可观测指标上不劣于默认的 `3000`，并消除了 2 行压缩越界；
  页数不变（7 页）。
- `scale` 在 `1500` 时几乎不敏感（50/100 并列，200 略差）；保留 `100` 可避免无谓改动。
- 因此**如果**要新增一个“中文书版” preset，候选值是
  `tolerance = 1500`、`badness_scale = 100`、垂直参数保持 `default_v1`。

## 4. 不做什么

- **不修改 `default-v1`**。preset 是冻结标识；改默认值必须新增 preset id 并走 ADR
  （架构 §9）。本报告只提供候选值。
- **不宣称质量提升**。压缩越界、留白、抖动是可测量量，不等于“更好看”。
  Gate Q 仍然需要真实语料 + 至少 5 名评审的盲测（D01）。
- **不外推**。5 篇、135 行的语料能发现明显的参数问题，不能覆盖长书稿、
  表格页、公式页、脚注密集页。

## 5. 复现

```sh
bash scripts/corpus.sh
# 或
cargo run -p soma-eval --release -- corpus --dir corpus/zh
cargo run -p soma-eval --release -- calibrate --dir corpus/zh \
  --tolerances 1500,2000,2500,3000 --scales 50,100,200
```

`scripts/gate-q.sh` 会运行 `corpus` 的结构断言（无孤行/寡行、压缩越界有预算、
确定性）并打印上面的表。
