# 未做清单（Debt register）

状态：**2026-11-25，Web Studio 后**。这是全局台账，不随里程碑报告消失。
当前：**110 条，83 条已关闭，27 条未关闭**（23 个技术债 + 4 个非目标；其中 D01 需人工评审）。
关闭一项就改成 ~~删除线~~ 并写明关闭点；新增债务必须给 ID。

口径：
- **阻塞** = 是否阻塞下一个里程碑；`否` 表示可以带着走。
- **关闭点** 写里程碑编号（A1–A6）或 `未排期`。
- 非目标不叫债务，单独列在 §3，附触发条件。

---

## 1. K-P 内核（已冻结，只修 bug）

参考：ADR-0007、`docs/kp-closure.md`。

| ID | 项 | 现状 | 影响 | 关闭点 | 阻塞 |
|---|---|---|---|---|---|
| D01 | Gate Q 质量宣称 | **Inconclusive**：工具链已闭环（kit 生成 → `--key` 反解 → Wilson/Fleiss → `--out` 报告，见 `docs/blind-dry-run.md`），但没有人工评审数据；`corpus/blind/dry-run/` 是**模拟**面板，只证明机制通，不计质量 | 不能对外宣称「比贪心好」 | 需 30 段 × 5 评审按 `docs/blind-test.md` 的 runbook 采集 | 否 |
| D02 | ~~Gate D 交付指标~~ | **已关闭（ADR-0071 / Gate W）**：`scripts/web.sh` 的 W2 量 gzip 体积（40 KB 预算），W4 量 Node/Chromium 里的段落时延（1800 atoms ≤25 ms、180 atoms ≤5 ms）；实排 32 KB gzip / 7.2 ms / 0.18 ms | — | ✅ 已关闭 | 否 |
| D03 | ~~`FallbackStage::Overfull`~~ | **已关闭**：该变体从未被发射，已从 `FallbackStage` 删除；canonical 输出从未包含 `"overfull"`，字节不变 | — | ✅ 已关闭 | 否 |
| D04 | ~~trim/gap 比例~~ | **已关闭（ADR-0051）**：实测了各标点类的侧空（开/闭 0.627 em，中 0.184/0.702，半角 0.039/0.067，`cargo run -p soma-metrics --example punct`），但换上实测值会让 `notice` 出现 2 处可避免的 fitness 跳变、抖动 p95 368→428（ADR-0042 的门槛是 3）。trim 上限是断行的应急预算而不是空档，因此**维持原值**并记录证据，见 `docs/trim-calibration.md` | — | ✅ 已关闭 | 否 |
| D05 | ~~Tier A 权重~~ | **已关闭（publish-v1）**：proxy 扫描的 `tolerance=1500` 已作为 `publish-v1` 落地（`default-v1` 仍冻结）；无人工数据前不再新增 preset | — | ✅ 已关闭 | 否 |
| D06 | ~~真实字体度量~~ | **已关闭（E1）**：`soma-metrics`（零依赖、no_std）+ `tools/metrics-gen` + Noto CJK SC 整数表；`TableMetrics`/`StyledMetrics` 按字号缩放；`metricsHash` 进入 `soma.pages/2`。覆盖范围与 bold 近似见 D61/D63 | — | ✅ 已关闭 | 否 |
| D07 | ~~wasm 执行时延~~ | **已关闭（ADR-0071 / Gate W4）**：`scripts/web.sh` 每次重跑 Node/Chromium harness，记录 1800 atoms 中位 7.2 ms、180 atoms 0.18 ms | — | ✅ 已关闭 | 否 |
| D08 | ~~叠标点挤压 / 按字号缩放 trim~~ | **已关闭（E3）**：`trim.pair`（1/4em，ParamOnly）+ trim 按 atom 字号缩放 + 行尾悬挂标点（`hanging-punctuation`，opt-in） | — | ✅ 已关闭 | 否 |
| D09 | ~~无 hyphenation~~ | **已关闭（ADR-0059）**：`soma-hyphen`（Liang 算法，模式集外置），CLI `--hyphenation`；内核新增 `BoundaryKind::Hyphen` 与 `Line.hyphenated`，连字符宽度计入行宽。URL/路径/全大写词不参与 | — | ✅ 已关闭 | 否 |
| D10 | ~~`boundary_deviation` 假设无自动告警~~ | **已关闭**：`scripts/gate-q.sh` 解析 scorecard 的 `bdev_rel_error_p95`，超过 20‰ 直接失败；当前为 0 | — | ✅ 已关闭 | 否 |
| D11 | ~~active-node 剪枝~~ | **已关闭（不做了）**：候选窗口已足够，`bench --check` PASS（混排 4000 atoms p95 2.26 ms）；没有质量或时延触发条件 | — | ✅ 已关闭 | 否 |
| D12 | ~~K7 fitness / rhythm~~ | **已关闭（ADR-0064）**：四档松紧等级进入 DP 状态，跨一档记一次代价；`rhythm-v1` preset 启用，其他 preset 字节不变 | 页级灰度仍只报告不优化 | ✅ 已关闭 | 否 |
| D13 | ~~`emergency_stretch` 双来源~~ | **已关闭**：删除 `BreakParams.emergency_stretch`，只保留 `fallback.emergency_stretch`；emergency 阶段由内部 `BreakConfig.emergency_stage` 标记，`modelVersion` 升到 3 | — | ✅ 已关闭 | 否 |
| D14 | ~~legacy `semantic` / `text`~~ | **已关闭（A1）**：两个模块与公开再导出已删除，等价能力迁到 `soma-doc` 的 `plain_text`；`ir_to_markdown` 是调试辅助，不再保留（canonical JSON 取代它） | — | ✅ 已关闭 | 否 |

## 2. 架构层（冻结架构已定、代码未写）

参考：`docs/architecture-v2.md` §15.2、§18。

| ID | 项 | 现状 | 影响 | 关闭点 | 阻塞 |
|---|---|---|---|---|---|
| D20 | ~~`soma-doc`（DocIR）~~ | **已关闭（A1）**：`soma-doc` 零依赖、`no_std`、schema `soma.doc/1`；13 个块 + 16 个行内变体、span/source map、校验、canonical JSON、双 hash | — | ✅ 已关闭 | 否 |
| D21 | ~~`soma-template`（纯 YAML）~~ | **已关闭（A2）**：YAML 子集解析、精确单位、级联/继承、`StyledDoc`（纯数据）、`templateHash` + `styleHash`；schema `soma.template/1` / `soma.styles/1` | — | ✅ 已关闭 | 否 |
| D22 | ~~`soma-layout`（盒子树）~~ | **已关闭（A3）**：`PagedLayout`/页面/放置块/行；段落盒与直接 K-P 逐行一致 | — | ✅ 已关闭 | 否 |
| D23 | ~~分页~~ | **已关闭（A4）**：确定性贪心、强制分页、keep-with-next、溢出标记、脚注区重排 | — | ✅ 已关闭 | 否 |
| D24 | ~~Word 前端~~ | **已关闭（ADR-0063 / ADR-0066）**：zip/inflate + OOXML；段落、标题（按样式名）、`basedOn` 样式继承、行内强调、表格、列表、元数据、内嵌图片（→ 图块）。浮动位置、绕排、裁剪、域、脚注仍不承诺保真 | 复杂 Word 会降级，符合 N1 | ✅ 已关闭 | 否 |
| D25 | ~~正式 HTML 渲染器~~ | **已关闭（A5）**：`soma-render-html` 消费 `PagedLayout` + `TextSource`，不重排 | — | ✅ 已关闭 | 否 |
| D26 | ~~脚注 / 图 / 表 / 公式适配~~ | **已关闭（A6 起，ADR-0017/0019/0026/0047）**：脚注整块移页 + 编号、图片 XObject、表格网格与行线/列线/colspan/rowspan、数学适配器全都在；剩余边界分别记在 D51、D71 | — | ✅ 已关闭 | 否 |
| D27 | ~~全管线 native/wasm 哈希~~ | **已关闭（A6）**：fixture 含 DocIR + 行内 + 页面，8,979 bytes 一致 | — | ✅ 已关闭 | 否 |
| D28 | ~~Gate A / C / R~~ | **已关闭**：Gate A、Gate C、Gate V（SVG 字节锁）、Gate R（SVG 坐标 = 布局坐标；HTML/SVG 覆盖同一 atom 集）全部接入 `gates.sh` | — | ✅ 已关闭 | 否 |
| D29 | ~~`EngineConfig` YAML~~ | **已关闭（ADR-0016）**：`soma.engine/1` 固定形状解析、校验、canonical + hash；`soma-engine` 只依赖 `soma-core` | 只有 `paragraph` 窗口（D66） | ✅ 已关闭 | 否 |
| D30 | ~~层哈希~~ | **已关闭（A3）**：`paramHash` + `docHash`/`contentHash` + `templateHash`/`styleHash` + 页面结果里的 `engineHash` | — | ✅ 已关闭 | 否 |
| D31 | ~~DocIR 没有 reader~~ | **已关闭**：新增 `soma_doc::reader` / `Document::from_canonical_json`，严格解析 writer 的全部字段；9 个 round-trip/错误测试，writer 字节不变 | — | ✅ 已关闭 | 否 |
| D32 | canonical writer 两份实现 | `soma-core::result` 与 `soma-doc::canonical` 各有一个手写 writer | 改格式要改两处；容易出现风格漂移 | 第三个消费者出现时抽公共 crate | 否 |
| D33 | ~~`rowspan > 1` 未做网格占位校验~~ | **已关闭（ADR-0017）**：`rows × columns` 占位图；越界、重叠、未填满一行都报错 | — | ✅ 已关闭 | 否 |
| D34 | ~~`Date` 不校验月份天数~~ | **已关闭**：按公历校验每月天数与闰年；非法日期在前端阶段报错，合法日期的 canonical 字节不变 | — | ✅ 已关闭 | 否 |
| D35 | ~~DocIR 两条平行遍历~~ | **已关闭**：`for_each_meta` / `for_each_meta_mut` 由同一份变体/字段声明宏生成；新增变体会编译失败直到同步登记 | — | ✅ 已关闭 | 否 |
| D36 | ~~Raw 内容未做安全白名单~~ | **已关闭（Gate C）**：渲染器只画 fallback 文本，输出转义且不含 `href`/`src`/脚本；内容侧无执行路径 | — | ✅ 已关闭 | 否 |
| D37 | ~~Ruby 只有数据没有行为~~ | **已关闭（ADR-0060）**：`\ruby{base}{reading}` 与 `[[ruby:base\|reading]]`；基字为单原子，行盒向上预留注音，PDF/SVG/HTML 均绘制 | — | ✅ 已关闭 | 否 |
| D38 | ~~`Atom.text: String` 每 atom 一次分配~~ | **已关闭（不做了）**：4,000 atoms 混排 p95 2.26 ms，预算 80 ms；分配不在当前瓶颈，改内核需 modelVersion 且收益无证据 | — | ✅ 已关闭 | 否 |
| D39 | ~~wasm/FFI 边界形状未实现~~ | **已关闭（ADR-0071）**：`soma-web` 实现 C ABI 句柄表（`soma_para_new` → `soma_para_dump` → `soma_break`/`soma_break_greedy` → `soma_para_free`），`web/soma-kp.js` 直接调用；`tests/abi.rs` 锁 native/JS 两侧 | — | ✅ 已关闭 | 否 |
| D40 | ~~`soma-layout-par` 不存在~~ | **已关闭（不做了）**：串行路径已满足预算；并行是 ADR-0010 的可选 P2，没有触发条件 | — | ✅ 已关闭 | 否 |
| D41 | ~~模板属性集是否够用~~ | **已复核（A2）**：首批 16 个属性覆盖字体/行距/缩进/间距/对齐/颜色/装饰/分页控制 | — | ✅ 已关闭（A2 出口再复核一次） | 否 |
| D42 | ~~YAML 子集能力有限~~ | **已关闭（部分实现）**：支持单行 flow 序列/映射、嵌套与引号；锚点/多行标量/标签仍不支持并在文档中列明 | 复杂模板写法仍受限 | 有真实需求时再扩 | ✅ 已关闭 | 否 |
| D43 | ~~无模板继承 / `@import`~~ | **已关闭**：`parse_template_inherit(base, overlay)` 做纯数据级递归合并；`styles`/`masters` 追加，标量/映射覆盖；无文件 IO | — | ✅ 已关闭 | 否 |
| D44 | ~~不支持百分比长度~~ | **已关闭（受控）**：新增精确 `Percent`；页面四边距按页宽、`line-height` 按字号解析；无基数的位置仍明确报错 | — | ✅ 已关闭 | 否 |
| D45 | ~~模板槽位与封面布局未接~~ | **已关闭（ADR-0031/0032）**：封面由 `cover:` + 文档元数据生成（`cover_rows`、`coverTitle`/`coverSubtitle` 样式），目录由 `toc:` 生成；不需要通用 `slot` 机制 | — | ✅ 已关闭 | 否 |
| D46 | ~~页面母版未定义~~ | **已关闭（E4）**：`page.header`/`page.footer`/`header-text`/`folio`；页眉页脚住在页边距内，不改变分页 | 每章不同版式仍未做（D62） | ✅ 已关闭 | 否 |
| D47 | ~~选择器无组合子~~ | **已关闭**：新增 `parent`（直接父级）与 `ancestors`（从外到内的祖先链）；`within` 行为不变 | — | ✅ 已关闭 | 否 |

| D48 | ~~垂直 K-P 未实现~~ | **已关闭**：垂直 K-P 为默认分页，末页免费、脚注参与 DP、强制断点与 keep-with-next 为硬约束；贪心保留为显式策略与回退 | — | ✅ 已关闭 | 否 |
| D49 | ~~段落不跨页~~ | **已关闭（E2）**：垂直流原子降到行；段落跨页、`line_offset`/`lines_total` 行守恒、widow/orphan 硬可测 | — | ✅ 已关闭 | 否 |
| D58 | ~~垂直方向没有可拉伸胶水~~ | **已关闭（E2）**：`space-before/after-plus/minus`；页面空白按行高归一化；拉伸量真实分配到块坐标 | — | ✅ 已关闭 | 否 |
| D59 | ~~垂直代价参数不在 EngineConfig 里~~ | **已关闭（ADR-0016）**：`vertical:` 段覆盖 `VerticalParams`；`verticalHash` 进页面结果 | — | ✅ 已关闭 | 否 |
| D60 | 无 tracking 兼底 | E6 只做了 `-`/`/` 断点；未做字间 letter-spacing 兼底 | 长 run 仍可能压缩越界 | 未排期（需与内核 emergency 模型一起评估） | 否 |
| D61 | 斜体仍用 upright advance | **粗体已关闭（ADR-0022）**：Serif/Sans 的 Regular+Bold 四面进表，按 `weight` 选面；CJK 没有斜体面，`slant: italic` 仍用正体宽度（斜体只影响渲染姿态） | 斜体拉丁的换行点可能偏差 | 有斜体字面时再生成表 | 否 |
| D62 | 母版只有页边带可变 | **已支持命名母版、按节切换、章节起始母版（ADR-0021/0028）与自动封面（ADR-0031/0032）**：`masters:` + `Section.page_template` + `page.start-master` + `cover:`；母版只改页眉/页脚/folio，**版心几何仍全局一套** | 封面等特殊版心做不了 | 后续 | 否 |
| D63 | 度量表是声明子集 | 覆盖 Latin/标点/假名/CJK 基本区与扩展 A；扩展 B+ 与少数字形回退到 1em | 生僻字宽度不准 | 需要时重新生成表 | 否 |
| D64 | ~~页眉页脚只排一行~~ | **已关闭**：超长 running title/字面量按测量宽度二分截断并加 `…`，保留 folio 对齐且不进入版心 | — | ✅ 已关闭 | 否 |
| D65 | 引擎配置用固定形状解析器 | 不复用模板的 YAML 子集：无序列、无锚点、长度只支持整数 `em` | 配置 DSL 与模板 DSL 是两套错误面 | 有真实需求时再抽公共 YAML crate | 否 |
| D66 | 引擎只支持 `paragraph` 窗口 | `window: page`/`chapter` 在配置层被拒绝；内核本来也报 `UnsupportedWindow` | 页级/章级搜索未实现 | 未排期（需要页级 DP 上下文） | 否 |
| D67 | 语料规模仍有限 | `corpus/zh` 现有 12 个文档/1211 行：长篇（30 页）、脚注密集、访谈、科普、colspan/rowspan 表格、表头重复、三线表、真实行间公式已覆盖；仍缺目录/章节页与质量标注 | 代理标定的外推能力有限 | 持续补充 | 否 |
| D70 | ~~LaTeX 子集有边界~~ | **已关闭（ADR-0058 / ADR-0067）**：表格单元 inline、`\item[label]`、`description`；`\newcommand`/`\renewcommand`/`\providecommand`/`\def`、参数替换、计数器与 `\arabic`/`\roman`/`\alph`/`\value`/`\the<name>`。仍不做参考文献与交叉引用（`\label`/`\ref`） | 复杂 LaTeX 会降级 | ✅ 已关闭 | 否 |
| D71 | ~~数学子集有边界~~ | **已关闭（ADR-0054 / ADR-0056）**：斜体变量（面里有 U+1D434… 才启用）、`array` 列格式、`\substack`、`\overbrace`/`\underbrace`（窄时用字形，宽时用线段拼，标注居中）、`\color`、多行公式编号、`\tag`/`\tag*`/`\notag`、`\text{}` 内嵌 `$…$`、嵌套 `aligned` 不再重复编号。仍缺：`\intertext`、`alignat`/`flalign`、`\tag` 对齐到独立列（每行标签都在版心右边界）；拼出的括弧比例固定，不读字体 MATH 表 | — | ✅ 已关闭 |
| D72 | 行内数学复用显示常量 | **已关闭（ADR-0030）**：`text_frac_scale`/`text_frac_gap`/`text_big_scale` 三组常量；显示分式改为全尺寸分子分母；行内上下限不堆叠 | 无 | ✅ 已关闭 | 否 |
| D73 | ~~目录能力有边界~~ | **已关闭（ADR-0032）**：`toc.leaders`（默认开）生成引导点；`toc.figures`/`toc.tables` 生成图目录/表目录；目录条目带自动编号。仍无索引与 PDF 书签（并入 D79） | — | ✅ 已关闭 | 否 |
| D74 | ~~CLI 只做单文件~~ | **已关闭**：`--input -` + `--from` 读 stdin；`--input <dir>` + `--out <dir>` 非递归批处理；`--watch` 明确不做 | — | ✅ 已关闭 | 否 |
| D75 | ~~选择器看不到标题级别~~ | **已关闭（ADR-0032）**：`Selector.level: 1..=9` 参与匹配与特异性，可只写 `level` | — | ✅ 已关闭 | 否 |
| D76 | ~~前置页页码不区分~~ | **已关闭（ADR-0032）**：`page.front-matter: continuous/roman/hidden`；前置页=只含生成内容的连续页；`Page.printed` 输出印字标签，`{pages}` 在 roman/hidden 下为正文页数 | — | ✅ 已关闭 | 否 |
| D77 | ~~没有自动编号~~ | **已关闭（ADR-0032）**：`numbering:` 生成中文/点号标题号与图/表号；编号在布局期生成，不进 DocIR；目录条目带编号 | — | ✅ 已关闭 | 否 |
| D78 | ~~题注没有专用样式~~ | **已关闭（ADR-0033）**：`figureCaption`/`tableCaption` 是生成元素，题注内联在题注元素下解析；表题在表上方并 keep-with-next | — | ✅ 已关闭 | 否 |
| D79 | ~~目录行是不可折断的整体~~ | **已关闭（ADR-0033）**：目录/索引标题按文本编译，可在标题盒内换行；页码与引导点跟随最后一行 | — | ✅ 已关闭 | 否 |
| D90 | ~~表格列宽一律平分~~ | **已关闭（ADR-0058）**：布局先量每列的自然宽（含 colspan 摊派），再按比例分配，`measure / 6n` 作下限，最大余数法保证列宽之和精确等于版心 | — | ✅ 已关闭 | 否 |
| D91 | ~~混排 2000 atoms 的性能预算擦边~~ | **已关闭（ADR-0072 / D94 候选窗口）**：`bench --check` 现在 PASS；本机 release p95：混排 2000 atoms 1.117 ms、4000 atoms 2.259 ms，纯中文 2000 atoms 0.221 ms（预算 20/80 ms） | — | ✅ 已关闭 | 否 |
| D92 | ~~垂直页 badness 的 body 参照取「页首项高度」~~ | **已关闭**：垂直参数新增 `body_height`（0=自动取文档行高），`page_badness` 不再用页首项高度；语料基线未变 | — | ✅ 已关闭 | 否 |
| D93 | ~~数学定界符用连续缩放，不取字体变体~~ | **已关闭（ADR-0075 / ADR-0079）**：`soma.metrics/6`–`7` 记录基础字形的纵向 `MathVariants`（ADR-0079 后内嵌表为 New Computer Modern Math：144 个基础字形、490 条变体；调查期 Noto 963 条、DejaVu 385 条），`soma-math` 按 TeX 规则选「最小可覆盖」的设计字形，PDF 用字形号绘制；根号横线改从墨迹右端起笔，根指数按 `radicalDegreeBottomRaisePercent` 落位，kern 改为有符号。语料基线一行未动 | 大算符、根号与定界符的笔画不再随放大变粗 | ✅ 已关闭 | 否 |
| D97 | 数学装配件与横向变体未用 | 三款已装数学字体（Noto Sans Math、DejaVu Math TeX Gyre、TeX Gyre Pagella Math）的纵向 `GlyphAssembly` part 数都是 0，`soma.metrics/7` 因此只写变体列表；超过最大变体时用最大设计字形（TeX 在没有更大字形时的做法）。横向变体仍未读：`\overbrace` 在该字体没有横向构造时退回规则拼装（Noto 的 U+23DE 正是如此） | 极端高的根式/括号不能继续加高；横向大括号/宽重音仍是拉伸或规则拼装，不是字体设计件 | 有字体真的提供装配件，或横向变体出现视觉需求时，加 `soma.metrics/8` | 否 |
| D98 | 重音不用 `MathKernInfo` 与扁平重音 | **部分关闭（ADR-0076）**：顶部重音锚点（`MathTopAccentAttachment`）与组合重音已读，`\hat`/`\bar`/`\tilde`/`\vec` 不再画 `.notdef`；仍未读 `MathKernInfo`、`flattenedAccentBaseHeight` 与扁平重音变体，宽重音只用横向 `stretch` | 宽重音/高基底的细节与 LaTeX 仍有差距 | 与横向变体（D97）一起做，加 `soma.metrics/8` | 否 |
| D94 | ~~emergency 阶段没有候选窗口~~ | **已关闭（ADR-0072）**：`line_length_bounds` 以前在 `allow_emergency` 时直接返回 `(1, n)`；现在过宽方向也有界——两个旗标同时打开时按 `emergency_stretch` 界住，否则按满 shrink（`tolerance_eff = min(tolerance, 1000)`，顺带修掉 `emergency_tolerance = 10.0` 让窗口算式失效的问题）。混排 2000 atoms @20 em：候选 1 255 320 → 30 316，Chromium 40.5 ms → 2.5 ms，断点序列与语料基线逐字不变 | — | ✅ 已关闭 | 否 |
| D95 | ~~浏览器 preview 与 PDF 不同字节~~ | **已关闭（页面几何部分，ADR-0093）**：Studio 把整个引擎编进 wasm，浏览器与 Axum 都跑 `soma-studio-core`、同一份 `.smt` 表与同一棵 `PagedLayout`；`/api/verify` 与 `scripts/studio.sh` 锁 canonical 哈希相等。剩余差异只是字形墨迹，单独记为 D101 | — | ✅ 已关闭 | 否 |
| D96 | ~~内嵌度量表是 v1，没有墨迹~~ | **已关闭（ADR-0073/0074）**：内嵌 `noto-cjk.smt` 重新生成到 `soma.metrics/5`，含 4 个 CJK 面（标点墨迹）与 1 个数学面（35 个 MATH 常数 + 2,901 个字形墨迹）；CFF 没有 `glyf`，改用「覆盖区间拆成连续 run 的 block」来压缩 advance，8.4 万 → **28.8 万字节**（逐条写是 110 万）。语料重新基线：断点/页数一行未动，`fitness_jumps` 3 → 8、`jitter_p95` 377 不变 | 默认路径的行末标点现在也贴边 | ✅ 已关闭 | 否 |
| D89 | ~~列表没有悬挂缩进~~ | **已关闭（ADR-0058）**：内核 `ParagraphStyle.hang`（默认 0，行为不变）；模板里写负的 `first-indent` 即悬挂缩进，记号落在文字列左侧 1.2em，续行回到文字列 | — | ✅ 已关闭 | 否 |
| D80 | ~~索引按码位排序~~ | **已关闭（ADR-0061）**：`soma-collate` 内置 27,584 个码位的拼音序表；模板 `index.sort: codepoint\|pinyin`（默认 codepoint，canonical 字节不变） | — | ✅ 已关闭 | 否 |
| D81 | ~~索引标记覆盖有限~~ | **已关闭（ADR-0061）**：`a!b` 子条目、`@排序键`、`\|see{}`/`\|seealso{}`；表格单元与脚注定义里的标记按所在页收集；父项未标时自动补父行；连续三页以上合并为 `3–5` | — | ✅ 已关闭 | 否 |
| D82 | ~~PDF 后端有边界~~ | **已关闭（ADR-0034…0047）**：`glyf`/CFF 嵌入（0039/0040）且 CFF 子集压缩排（0046）；动态 Huffman 流压缩（0045）；下划线/删除线与链接注解（0044）；PNG/JPEG 图片 XObject 与 tagged PDF（0047）。仍有的窄边界：无 ICC/CMYK 配置、图片只按盒缩放、不支持 >8-bit PNG 与 inline image、表格单元在结构树里当段落、PDF 只在 CLI 暴露 | — | ✅ 已关闭 | 否 |
| D83 | ~~部分全角标点没有标点类~~ | **已关闭（ADR-0058）**：`…`/`—`/`–`/`·` 归入新的 `PunctClass::Dash`——前面不许断行、两侧不裁量、不悬挂；`——` 与 `……` 保持 2em | — | ✅ 已关闭 | 否 |
| D85 | ~~name-keyed CFF 不能嵌入~~ | **已关闭（ADR-0040）**：name-keyed 脸被提升为 CID-keyed（合成 ROS/FDArray/FDSelect），`seac` 复合由 Type 2 解释器展开成显式轮廓（子程序内联、hint 丢弃、笔位置跟踪）；解释器不认识的算子直接报错 | — | — | ✅ 已关闭 |
| D84 | ~~行末侧空是常量而不是墨迹边界~~ | **已关闭（ADR-0037）**：`soma.metrics/2` 记录标点的 `ink_left`/`ink_right`，`scale_trim` 优先用真实轮廓；v1 表或无 `glyf` 的字体回退到类别常量 | — | — | ✅ 已关闭 |
| D86 | ~~拉丁文独排时段落词间空会被压到很窄~~ | **已关闭（ADR-0041）**：词间空的可压缩量改为 `min(1/8 em, space_advance/3)`；`overfull` 的齐行行不再当作正常断点，只经 fallback 到达（`modelVersion` 2）。语料 `over_compress_lines` 3 → 0 | — | — | ✅ 已关闭 |
| D88 | ~~公式排版有可见缺陷~~ | **已关闭（ADR-0053）**：`Radical`/`Delimited`/`Matrix` 三个节点忽略父节点给的 baseline，所以分式里的根号会压到分数线上；分式自身又用「分子高度 + 分母深度」避让，避让的边正好选反。两处都改成按 baseline 下推、按朝向分数线的边避让，`\frac{\sqrt{\pi}}{2}` 现在是三层分离。遗留：数学变量仍是正体（没有数学斜体面），D71 继续跟踪 | — | ✅ 已关闭 | 否 |
| D87 | ~~SVG 根元素用 `em` 尺寸~~ | **已关闭（ADR-0049）**：真正的问题是坐标也写成 `em`（相对元素自身 font-size），与 `viewBox` 的 layout unit 不同源，因此单页抽出来渲染会错位。现在坐标与 `font-size` 都是无单位 layout unit，根元素宽高是像素（1 em = 16 px），Gate R golden 重算为 `9092984200964852902` | — | ✅ 已关闭 | 否 |
| D69 | ~~HTML 子集有边界~~ | **已关闭（扩展）**：新增 `<dl>/<dt>/<dd>`、`<ins>`、`<sup>/<sub>`、`<hr>`、`<figure>/<figcaption>`；未知标签仍保留文本 | — | ✅ 已关闭 | 否 |
| D68 | 表格线只能整表配置 | **已关闭（ADR-0020）**：五条线宽、竖线/左右框、实线/虚线、颜色都进模板；剩余限制是 `table:` 只能全局一套，不能按表/按行覆盖 | 多套表格外观要多个模板 | ✅ 已关闭 | 否 |
| D50 | ~~无页面母版 / 页眉页脚 / 页码~~ | **已关闭（E4）**：页眉（running-title 或字面量）、页脚、`{page}`/`{pages}` 页码；两遍无重排 | 首页/封面变体未做（D62） | ✅ 已关闭 | 否 |
| D51 | 盒子树是扁平的 | **colspan、rowspan、表头重复、单元格跨页续排、行线、tagged 表格层次已支持**（ADR-0017/0018/0019/0050）：结构树现在是 `/Table` → `/TR` → `/TD`，跨行列带 `/RowSpan`/`/ColSpan`，一个单元里的多段共用一个 `/TD`。仍无嵌套 VBox/HBox、逐表覆盖线宽、单元格四边框 | 嵌套布局与逐表线宽受限 | 后续 | 否 |
| D52 | ~~公式是文本占位~~ | **已关闭（ADR-0026）**：`soma-math` 解析 TeX 子集 → 整数 `MathLayout`（符号 + 分数线/根号横线）；`soma-layout` 居中放一个 `MathBox`（带基线），页面 schema 升到 `soma.pages/5`；解析失败仍回退到文本占位。剩余边界见 D71 | — | ✅ 已关闭 | 否 |
| D53 | ~~Markdown 子集有限~~ | **已关闭（ADR-0062）**：嵌套列表（按缩进递归）、GFM 表格（对齐与 `\|` 转义）、setext 标题。仍无 HTML 块、链接引用定义 | 纯语义子集，未知语法保留为文本 | ✅ 已关闭 | 否 |
| D54 | ~~行内代码无独立度量~~ | **已关闭**：代码 span 的 atom 带 `code` 元素样式，宽度按该样式的面与字号量（测试 `inline_code_is_measured_with_the_code_style` 锁住；实测 9pt 无衬线窄于 10.5pt 正文） | — | ✅ 已关闭 | 否 |
| D55 | ~~图只画占位框~~ | **已关闭（ADR-0047/0048）**：PNG/JPEG 在 PDF 里是 XObject（PNG 走 Flate、JPEG 直通），HTML/SVG 用 `data:` URI；盒子的宽高比来自图片本身。Ruby 见 ADR-0060 | — | ✅ 已关闭 | 否 |
| D56 | ~~`Section` 级脚注编号回退~~ | **已关闭**：`footnote.numbering: section` 按节重置，脚注区编号引用同一 mark，跨页/同页/重复引用均覆盖 | — | ✅ 已关闭 | 否 |
| D57 | ~~`Line` 序列化上提到 kernel~~ | **已关闭（无动作）**：`Line::write_canonical` 已是唯一 writer，页面结果复用；没有第二个消费者需要抽取 | — | ✅ 已关闭 | 否 |
| D99 | TikZ 只做核心绘图语言 | **ADR-0092 已交付**：路径/节点/坐标/网格/裁剪与三种后端的矢量绘制；仍不包含 `plot`、`decorations`、`graphs`、`matrix`、`pic`、`clip`、patterns、shadings、3D、`positioning` 库；箭头是几何 barbs 而非 `arrows.meta` 字形；没有旋转选项；节点文字不能 `\\` 换行 | 复杂 TikZ 图会走可见的源码回退，而不是画错 | 有真实文档用到对应库时按 ADR 扩子集 | 否 |
| D100 | 字对 kerning 未接入 | `Metrics::kerning` 仍是默认返回 0 的钩子，断行与渲染都没有调用点；度量表也不带 pair kerning | 拉丁紧密字对的宽度与真实字体有偏差（中文不受影响） | 有真实拉丁排版需求时生成 pair 表并接进 atom 宽度 | 否 |
| D101 | 字形墨迹与引擎度量不同源 | Studio 的页盒、断点与坐标来自 `.smt`，浏览器用自己的字体文件成形；同一字体下差异应 <5‰，缺字体时由标题栏探针报 `不一致`。探针是诊断，不进入断行或渲染 | 极端字体（合成粗体、hinting 差异大）的墨迹可能与 PDF 有可见差别 | 需要把模板字体子集化并内嵌网页字体时再做 | 否 |
| D102 | `Line` 只导出总 `cost` | 检视页能显示 ratio、裁剪、边界偏差与总代价，但逐项代价（badness、short-last、line 等）没有进入 canonical 或 UI；`inspect-detail` 显示 `—` | 标定时看不到某一行为什么贵 | 有调试/教学需求时加一个非 canonical 的 trace 导出层 | 否 |
| D103 | atom 没有源文偏移 | 行→源文映射是块级精确、行级尽力；点击行只能定位到块，不能精确选中该行的源文片段 | 编辑器无法做行级双向高亮 | 编辑器需要行级选中或诊断时，给 atom 加源文 span 索引 | 否 |
| D104 | Studio 构建在主线程 | 打字后 220 ms 防抖构建；提案里的「>800 atoms 走 Worker」没有实现。需要可序列化的 `AppBuild` 与第二个 wasm 二进制 | 超大文档（4k+ atoms）输入时可能掉帧 | 真实项目实测超预算时再做 | 否 |
| D105 | Studio 没有单文件静态导出 | 项目文件、字体与 PDF 依赖 Axum；`web/` 的 K-P slice 仍是唯一 server-free 嵌入件 | 不能把一个离线 HTML 直接分享给他人 | 有离线分享/静态托管需求时做 | 否 |
| D107 | 流程图只覆盖 Mermaid flowchart 与 DOT 子集 | 子图只画成员包围框，不约束 rank；sequence/class/state/ER、图标、HTML 标签、外部链接没有实现；宽图低于 60% 或高于页高回退源码 | 复杂组织结构图会退化成较松的层次图；非 flowchart 图种仍显示为代码 | 有真实图种需求时扩 `soma-diagram` 的 frontend；子图 rank 约束优先 | 否 |
| D108 | Studio 的 TikZ 导出只取第一张图 | `/api/tikz` 按文档顺序取第一张 Mermaid/DOT 图，导出自然尺寸；不能选择后续图、导出页面缩放后的几何，也不能导出 SVG | 含多张图的文档需要手工定位并裁剪导出的文件 | 有按图导出需求时给图块加稳定 target，UI 增加图列表选择 | 否 |
| D109 | **部分完成（ADR-0107）**：Markdown 内嵌 LaTeX 的脚注已提升为真实定义，脚注内 `\index` 会收集；导言宏/计数器语义仍不完整 | 行内 `\footnote{...}` 现在生成 FootnoteRef + FootnoteDef；块级环境的脚注合并仍受标签冲突限制；Markdown 没有 LaTeX 导言区，`\newcommand`/计数器宏不参与 | 混合文档里的 LaTeX 脚注可以直接用；复杂宏包与计数器语义仍不可用 | 有真实混合文档依赖导言宏时，补一层最小导言上下文 | 否 |
| D110 | **已完成（ADR-0106）**：`figure: width: natural/half/full` 与 `align`；LaTeX 仍只保留路径与语义尺寸提示 | 模板决定图宽/对齐；`natural` 按 96 dpi 的本征像素折算并封顶版心 | 作者不能直接写 `width=0.5\linewidth`；要由模板改 `figure.width` | 有逐图宽度需求时再加语义尺寸提示 | 否 |
| D111 | 索引暂不排双栏 | `index_rows` 逐条生成单列 flow item；超过 24 条时仍是一列，页数浪费 | 长索引（>24 条）会多占页面 | 有长索引样书时按栏高切分生成双栏索引项 | 否 |
| D112 | 页眉下没有细线 | `runningHead` 样式已独立，但模板没有 `header-rule` 参数，渲染器不画页眉分隔线 | 页眉与正文的视觉分层弱一些 | 需要时加 `page.header-rule` 与百分比宽度，PDF/SVG 同步 | 否 |
| D113 | 数学字体样式只解析不变形 | `\mathbf`/`\mathrm` 等现在透明解析，保证整条公式不失败；数学面还没有粗体/正体变体，`\mathbf{A}` 仍是普通数学字 | 需要粗体的数学标题/向量只是不致于回退源码，视觉上不够重 | 有真实需求时给 `MathItem` 加字形变体并选粗体数学面 | 否 |
| D114 | ~~行内代码无空格且无分隔符时不可断~~ | **已关闭（ADR-0112）**：无空格、无分隔符的长代码串（base64、超长驼峰标识符）先按驼峰/`_`/`.` 接缝切开；仍无接缝且超过 12 字符时逐字符给断点（`INTRUSIVE_BREAK_PENALTY = 2000`），文本拼接仍等于原文 | 单个字形比版心还宽时仍会 overfull（字形本身的属性） | ✅ 已关闭 | 否 |
| D115 | ~~单字行（非末行）不计费~~ | **已关闭（ADR-0112）**：`short_score` 现在对每条齐行行计费（同一 `short_last_threshold`/`short_last_scale`），末行仍走 `ParagraphEnd` 分支；`ForcedEnd` 行按作者意图豁免 | 非末行孤字不再免费，emergency 阶段会优先把它放到代价最小的位置 | ✅ 已关闭 | 否 |
| D116 | 图边标签可能与箭头/节点重叠 | 边标签居中在边的中点，没有避让；TACTUS 页 7 的 `render plan` 与箭头相撞 | 复杂流程图的可读性下降 | 给边标签留避让通道或垂直偏移 | 否 |
| D117 | ~~本地模式只打开内置 `full.tex`~~ | **按设计关闭（ADR-0119）**：公共入口就是一份可编辑的 `welcome.md`；任意本地文件走 `?server=1` 的服务端文件树 | 线上不再展示示例工程树；用户改这一份文件即可 | 需要浏览器内打开任意文件时再加 File System Access | ✅ 已关闭 | 否 |
| D118 | ~~本地 PDF 在子集覆盖不足时回退完整字体~~ | **已关闭（ADR-0119）**：GB2312 一、二级子集同时供校样与 PDF，不再做覆盖检查，也没有 19 MB 回退；一个 TTF 集被两边共用 | 首次导出只需解析已缓存的子集，welcome 文档实测 223 KB / 50 ms | — | ✅ 已关闭 | 否 |
| D119 | 浏览器字体只到 GB2312 | Plex 正/粗体按 GB2312 一、二级（7 199 字，含标点与模板补充）子集；数学用完整 NewCM，等宽用拉丁子集。字集外的生僻字在浏览器校样和 PDF 中显示为空框；CLI 仍可用完整字体 | 普通中文稿完全覆盖；生僻字需换字或走本地 CLI | 需要时在 UI 里检测缺字并提示，或按文档动态请求服务端子集 | 否 |
| D106 | Ghostscript 渲染 CJK TrueType 子集时字形错位 | Firefox/pdf.js 与 Poppler（`pdftoppm`/`pdftotext`）渲染正确，`gs` 会把正文 CJK 换成别的字形（标题粗体正常）。用整字体替换子集、加 `cmap`/`name`/`post`、写显式 `CIDToGIDMap` 均未改变结果，倾向是 Ghostscript 对保留 GID 空洞的 CIDFontType2 子集的解释差异 | 用 Ghostscript 打印或某些基于 gs 的查看器会看到错字 | 需要时改为紧凑 CID + 重排 GID 的子集格式，并加 gs 渲染回归 | 否 |

## 3. 非目标（不是债务）

触发条件写清楚，条件不到就不做。

| ID | 项 | 触发条件 |
|---|---|---|
| N1 | ~~Word import + 样式映射~~ | **已交付（ADR-0063/ADR-0066）**：`soma render --input draft.docx` 读取 zip/OOXML，映射段落、标题、行内强调、表格、列表、元数据与嵌入图片；不读浮动体、绕排、裁剪、域与脚注，不承诺版式保真（剩余边界见 N3） |
| N2 | ~~PDF / SVG 渲染器~~ | **已交付（ADR-0034）**：SVG（Gate R）与 PDF（TrueType 子集、书签、页面标签）；边界见 D82 |
| N3 | 浮动体、多栏、竖排、bidi、整书级优化、真首字下沉 | 单栏 MVP 稳定后再评估；首字下沉需要变行宽 + 绕排 |
| N4 | 增量重排（按 NodeId 局部重排） | 全量重排实测超预算（当前 4,000 atoms 行内 1.2 ms，余量充足） |
| N5 | 富文本编辑器 | 不属于排版管线 |
| N6 | 让 AI 直接输出断点或页面坐标 | 永不；核心是唯一布局真相 |

---

## 4. 与里程碑的关系

| 里程碑 | 关闭的债务 |
|---|---|
| A1 | **已完成**：D14、D20、D28（Gate A）、D30（docHash）；新增 D31–D37 |
| A2 | **已完成**：D21、D30（templateHash + styleHash）、D41；新增 D42–D47 |
| A3 | **已完成**：D22、D30（engineHash）；D06（真实字体）与 D29（EngineConfig YAML）仍开 |
| A4 | **已完成**：D23；D08、D10 仍开 |
| A5 | **已完成**：D25、D24（Markdown）；D02、D07、D28（C/R）仍开 |
| A6 | **已完成**：D27、D26（脚注/图/表）；D04（标定）仍开 |
| E1 | **已完成**：D06、D28（metricsHash）；新增 D61、D63 |
| E2 | **已完成**：D49、D58；新增 D60 |
| E3 | **已完成**：D08；新增 test/pair 规则 |
| E4 | **已完成**：D46、D50；新增 D62、D64 |
| E5 | **已完成**：基线网格（`baseline-grid`，opt-in） |
| E6 | **已完成**：D09 的断点部分、D28 的 Gate V/R |
| Q′ | **已完成**：`soma-eval score` 效果记分卡与 `scripts/gate-q.sh` |
| Q′+ | **已完成**：`corpus/zh` 真实语料 + `soma-eval corpus/calibrate` + 代理标定报告；部分关闭 D05 的语料前提 |
| E7 | **已完成**：D29、D59（`soma.engine/1`，ADR-0016）；新增 D65、D66 |
| E7+ | **已完成**：D33、D51 的表格部分（ADR-0017：网格、rowspan、表头重复；ADR-0018：行带续排；ADR-0019：行线与 `within`）；新增 D68 |
| 数学 | **主要缺口已关闭（ADR-0026/0027/0030）**：D52 关闭；D54 收窄；D71 大部分关闭；D72 关闭 |
| 工具链 | **已完成（ADR-0028/0029）**：目录与章节起始母版；`soma` CLI；盲测 dry run；新增 D73、D74 |
| 首发成品 | **已完成（ADR-0031）**：`align: left/center/right` 真正可用；生成封面；`examples/book` 十页成品 |
| 书稿能力 | **已完成（ADR-0032）**：级别选择器、自动编号、前置页页码、目录引导点与图/表目录；新增 D78、D79 |
| 长文档能力 | **已完成（ADR-0033）**：题注样式、可换行目录、书签 outline、索引；关闭 D78、D79，新增 D80、D81 |
| PDF 输出 | **已完成（ADR-0034/ADR-0039）**：TrueType 稀疏子集与 CID-keyed CFF、Identity-H、书签树、PageLabels；边界见 D82/D85 |
| 全量补课 | **已完成（ADR-0059–0063）**：关闭 D09、D24、D37、D53、D80、D81 |
| 遗留清账 | **已完成（ADR-0064–0067）**：关闭 D12、D70；D24/D69/D81 补齐（Word 样式与图片、HTML `colgroup`、索引页码区间） |
| Web Studio | **已完成（ADR-0093/0094）**：`soma-studio-core/-ui/-` 三个 crate、S1–S10、字体探针、SSE 重载、CSP；关闭 D95，新增 D101–D105 |
| 流程图 | **已完成（ADR-0102）**：`Block::Diagram`、Mermaid/DOT frontend、确定性分层布局、三种渲染器零改动、TikZ 导出；新增 D107 |
| 混合语法 | **已完成（ADR-0104）**：Markdown 复用 `soma-html`/`soma-latex`，支持行内/行间数学、LaTeX 命令与环境、语义 HTML 与 HTML 块；新增 D109 |
| 未排期 | D01、D32、D51、D60、D61、D62、D63、D65、D66、D67、D97、D98、D99、D100、D101、D102、D103、D104、D105、D106、D107、D108、D109、D111、D112、D113、D116、D119、N3–N6 |
