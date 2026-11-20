# 未做清单（Debt register）

状态：**2026-11-20，K-P 收口后**。这是全局台账，不随里程碑报告消失。
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
| D02 | Gate D 交付指标 | **Partial**：无浏览器 wasm 包体积、无首段延迟实测 | 移动端预算无法承诺 | A5 | 否 |
| D03 | `FallbackStage::Overfull` | 字段保留但从未发射；长英文词走 `Emergency` | 语义上等价，报告口径少一档 | 未排期 | 否 |
| D04 | trim/gap 比例 | 6 条 `ParamOnly`，比例值未标定 | Gate Q 不能引用这些数值 | A6 后标定窗口 | 否 |
| D05 | Tier A 权重 | **Proxy 已扫**：`corpus/zh` 上 `tolerance=1500` 优于 `3000`（过压 0 vs 2 行，抖动 337 vs 500，页数不变）；无人工偏好，不修改 `default-v1` | 质量上限仍未知 | 待 Gate Q 后决定是否新增 `zh-book-v1` preset | 否 |
| D06 | ~~真实字体度量~~ | **已关闭（E1）**：`soma-metrics`（零依赖、no_std）+ `tools/metrics-gen` + Noto CJK SC 整数表；`TableMetrics`/`StyledMetrics` 按字号缩放；`metricsHash` 进入 `soma.pages/2`。覆盖范围与 bold 近似见 D61/D63 | — | ✅ 已关闭 | 否 |
| D07 | wasm 执行时延 | 只证明了编译 + 字节一致，没测运行时间 | wasm 预算只有推算 | A5 | 否 |
| D08 | ~~叠标点挤压 / 按字号缩放 trim~~ | **已关闭（E3）**：`trim.pair`（1/4em，ParamOnly）+ trim 按 atom 字号缩放 + 行尾悬挂标点（`hanging-punctuation`，opt-in） | — | ✅ 已关闭 | 否 |
| D09 | 无 hyphenation | 长英文 run 只能在 `-`、`/` 后断（E6）；无词典、无 tracking | 英文长词/URL 仍可能超宽 | 未排期（跟踪见 D60） | 否 |
| D10 | `boundary_deviation` 假设无自动告警 | `Line.boundary_deviation` 是比例分配近似，`LineTrace.boundary_deviation_exact` 是精确值；两者都在 `inspect` 输出，但 `bench` 只打近似值，没有相对误差门限 | 比例分配假设失效时不会报警 | A4 后加 P95 相对误差报告项 | 否 |
| D11 | active-node 剪枝 | 未实现；atom 数窗口已够（4,000 atoms 最差 1.2 ms） | 无 | 未排期（等 wasm 实测） | 否 |
| D12 | K7 fitness / rhythm | 相邻行松紧节奏不进代价 | 「松紧交替」无法优化 | 未排期（需 A4 页面上下文 + 评估证据） | 否 |
| D13 | `emergency_stretch` 双来源 | `break_params.emergency_stretch`（默认 `0`）在进入 emergency 阶段时被 `fallback.emergency_stretch`（默认 `3em`）覆盖，之后才被读取；等于两处配置面、一个实际生效值，两处都进 `paramHash` | `default-v1` 里看起来「emergency stretch 是 0」，容易误读；未来改一处会静默不一致 | modelVersion 2（删除冗余字段会改 hash） | 否 |
| D14 | ~~legacy `semantic` / `text`~~ | **已关闭（A1）**：两个模块与公开再导出已删除，等价能力迁到 `soma-doc` 的 `plain_text`；`ir_to_markdown` 是调试辅助，不再保留（canonical JSON 取代它） | — | ✅ 已关闭 | 否 |

## 2. 架构层（冻结架构已定、代码未写）

参考：`docs/architecture-v2.md` §15.2、§18。

| ID | 项 | 现状 | 影响 | 关闭点 | 阻塞 |
|---|---|---|---|---|---|
| D20 | ~~`soma-doc`（DocIR）~~ | **已关闭（A1）**：`soma-doc` 零依赖、`no_std`、schema `soma.doc/1`；13 个块 + 16 个行内变体、span/source map、校验、canonical JSON、双 hash | — | ✅ 已关闭 | 否 |
| D21 | ~~`soma-template`（纯 YAML）~~ | **已关闭（A2）**：YAML 子集解析、精确单位、级联/继承、`StyledDoc`（纯数据）、`templateHash` + `styleHash`；schema `soma.template/1` / `soma.styles/1` | — | ✅ 已关闭 | 否 |
| D22 | ~~`soma-layout`（盒子树）~~ | **已关闭（A3）**：`PagedLayout`/页面/放置块/行；段落盒与直接 K-P 逐行一致 | — | ✅ 已关闭 | 否 |
| D23 | ~~分页~~ | **已关闭（A4）**：确定性贪心、强制分页、keep-with-next、溢出标记、脚注区重排 | — | ✅ 已关闭 | 否 |
| D24 | 语法前端 | **Markdown（A5）、HTML（ADR-0024）、LaTeX（ADR-0025）子集已完成**；Word 未做 | Word 源仍不可用 | 后续 | 否 |
| D25 | ~~正式 HTML 渲染器~~ | **已关闭（A5）**：`soma-render-html` 消费 `PagedLayout` + `TextSource`，不重排 | — | ✅ 已关闭 | 否 |
| D26 | 脚注 / 图 / 表 / 公式适配 | **脚注整块移页 + 编号、图占位框、表格单元盒已做（A6）**；Math 适配器、表格行线/colspan 仍未做 | 复杂表格与公式质量受限 | 后续 | 否 |
| D27 | ~~全管线 native/wasm 哈希~~ | **已关闭（A6）**：fixture 含 DocIR + 行内 + 页面，8,979 bytes 一致 | — | ✅ 已关闭 | 否 |
| D28 | ~~Gate A / C / R~~ | **已关闭**：Gate A、Gate C、Gate V（SVG 字节锁）、Gate R（SVG 坐标 = 布局坐标；HTML/SVG 覆盖同一 atom 集）全部接入 `gates.sh` | — | ✅ 已关闭 | 否 |
| D29 | ~~`EngineConfig` YAML~~ | **已关闭（ADR-0016）**：`soma.engine/1` 固定形状解析、校验、canonical + hash；`soma-engine` 只依赖 `soma-core` | 只有 `paragraph` 窗口（D66） | ✅ 已关闭 | 否 |
| D30 | ~~层哈希~~ | **已关闭（A3）**：`paramHash` + `docHash`/`contentHash` + `templateHash`/`styleHash` + 页面结果里的 `engineHash` | — | ✅ 已关闭 | 否 |
| D31 | DocIR 没有 reader | canonical JSON 是 write-only；只有 writer 与 golden 测试 | 不能回读、不能做 round-trip fuzz，调试只能看输出 | 未排期（真需要的场景：跨进程传递 DocIR） | 否 |
| D32 | canonical writer 两份实现 | `soma-core::result` 与 `soma-doc::canonical` 各有一个手写 writer | 改格式要改两处；容易出现风格漂移 | 第三个消费者出现时抽公共 crate | 否 |
| D33 | ~~`rowspan > 1` 未做网格占位校验~~ | **已关闭（ADR-0017）**：`rows × columns` 占位图；越界、重叠、未填满一行都报错 | — | ✅ 已关闭 | 否 |
| D34 | `Date` 不校验月份天数 | 接受 2 月 30 日；只检查 year/month/day 范围 | 无法发现源侧的非法日期 | 未排期（来源已经是日期文本） | 否 |
| D35 | DocIR 两条平行遍历 | `for_each_meta` / `for_each_meta_mut` 是两份手写 match，靠测试锁同步 | 加字段时两处都可能漏 | 用宏/visitor 合并 | 否 |
| D36 | ~~Raw 内容未做安全白名单~~ | **已关闭（Gate C）**：渲染器只画 fallback 文本，输出转义且不含 `href`/`src`/脚本；内容侧无执行路径 | — | ✅ 已关闭 | 否 |
| D37 | Ruby 只有数据没有行为 | IR 保留 `base`/`annotation`，模板与布局还没有 ruby 策略 | 注音不会渲染 | A4/A6 | 否 |
| D38 | `Atom.text: String` 每 atom 一次分配 | 冻结内核里 `Atom` 拥有 `String`；`DocIR -> ParaIR` 必然物化 | 4,000 atoms ≈ 4,000 次分配，是当前 0.5 ms 的一部分 | A3 先测量；改内核需 ADR-0007 + modelVersion | 否 |
| D39 | wasm/FFI 边界形状未实现 | ADR-0010 §7 已定原则（句柄表或 JSON 进出，不用 `Arc`），但无代码 | wasm 宿主拿不到文档/结果 | A5 | 否 |
| D40 | `soma-layout-par` 不存在 | ADR-0010 §5 决定并行在独立 std crate；未创建 | 长文档墙钟未优化 | A6 后（P2） | 否 |
| D41 | ~~模板属性集是否够用~~ | **已复核（A2）**：首批 16 个属性覆盖字体/行距/缩进/间距/对齐/颜色/装饰/分页控制 | — | ✅ 已关闭（A2 出口再复核一次） | 否 |
| D42 | YAML 子集能力有限 | 无流式集合、锚点、多行标量、`@` 规则；`#` 需避开颜色写法 | 复杂模板可能写不出来 | 有真实模板需求时换解析器（API 不变） | 否 |
| D43 | 无模板继承 / `@import` | 每个模板必须自包含 | 多套模板重复声明 | 未排期 | 否 |
| D44 | 不支持百分比长度 | 只支持 em/pt/mm/cm/in | 响应式或依赖容器宽度的尺寸写不了 | A4（分页需要相对页宽时再评估） | 否 |
| D45 | 模板槽位与封面布局未接 | DocIR 有 `GeneratedBlock`，模板没有 `slot` 定义 | 封面/目录还不能由模板生成 | A6 | 否 |
| D46 | ~~页面母版未定义~~ | **已关闭（E4）**：`page.header`/`page.footer`/`header-text`/`folio`；页眉页脚住在页边距内，不改变分页 | 每章不同版式仍未做（D62） | ✅ 已关闭 | 否 |
| D47 | 选择器无组合子 | 已支持 `within`（只要求某一层祖先元素匹配）；仍无子代、伪类、多层后代链 | 复杂的关系选择器写不出来 | 有真实需求时再扩展 | 否 |

| D48 | ~~垂直 K-P 未实现~~ | **已关闭**：垂直 K-P 为默认分页，末页免费、脚注参与 DP、强制断点与 keep-with-next 为硬约束；贪心保留为显式策略与回退 | — | ✅ 已关闭 | 否 |
| D49 | ~~段落不跨页~~ | **已关闭（E2）**：垂直流原子降到行；段落跨页、`line_offset`/`lines_total` 行守恒、widow/orphan 硬可测 | — | ✅ 已关闭 | 否 |
| D58 | ~~垂直方向没有可拉伸胶水~~ | **已关闭（E2）**：`space-before/after-plus/minus`；页面空白按行高归一化；拉伸量真实分配到块坐标 | — | ✅ 已关闭 | 否 |
| D59 | ~~垂直代价参数不在 EngineConfig 里~~ | **已关闭（ADR-0016）**：`vertical:` 段覆盖 `VerticalParams`；`verticalHash` 进页面结果 | — | ✅ 已关闭 | 否 |
| D60 | 无 tracking 兼底 | E6 只做了 `-`/`/` 断点；未做字间 letter-spacing 兼底 | 长 run 仍可能压缩越界 | 未排期（需与内核 emergency 模型一起评估） | 否 |
| D61 | 斜体仍用 upright advance | **粗体已关闭（ADR-0022）**：Serif/Sans 的 Regular+Bold 四面进表，按 `weight` 选面；CJK 没有斜体面，`slant: italic` 仍用正体宽度（斜体只影响渲染姿态） | 斜体拉丁的换行点可能偏差 | 有斜体字面时再生成表 | 否 |
| D62 | 母版只有页边带可变 | **已支持命名母版、按节切换、章节起始母版（ADR-0021/0028）**：`masters:` + `Section.page_template` + `page.start-master`；母版只改页眉/页脚/folio，**版心几何仍全局一套**，也没有自动封面（`GeneratedKind::Cover` 无生成器） | 封面等特殊版心做不了 | 后续 | 否 |
| D63 | 度量表是声明子集 | 覆盖 Latin/标点/假名/CJK 基本区与扩展 A；扩展 B+ 与少数字形回退到 1em | 生僻字宽度不准 | 需要时重新生成表 | 否 |
| D64 | 页眉页脚只排一行 | running title 过长时溢出而不是换行/缩略 | 超长标题可能压到页边 | 未排期（需要字串截断策略） | 否 |
| D65 | 引擎配置用固定形状解析器 | 不复用模板的 YAML 子集：无序列、无锚点、长度只支持整数 `em` | 配置 DSL 与模板 DSL 是两套错误面 | 有真实需求时再抽公共 YAML crate | 否 |
| D66 | 引擎只支持 `paragraph` 窗口 | `window: page`/`chapter` 在配置层被拒绝；内核本来也报 `UnsupportedWindow` | 页级/章级搜索未实现 | 未排期（需要页级 DP 上下文） | 否 |
| D67 | 语料规模仍有限 | `corpus/zh` 现有 12 个文档/1211 行：长篇（30 页）、脚注密集、访谈、科普、colspan/rowspan 表格、表头重复、三线表、真实行间公式已覆盖；仍缺目录/章节页与质量标注 | 代理标定的外推能力有限 | 持续补充 | 否 |
| D70 | LaTeX 子集有边界 | `soma-latex` 不做宏/计数器/参考文献/真实公式排版；表格单元只当纯文本；无 `\item[label]`、`description`；无源位置。`figure`/`table` 环境、`\includegraphics`、`\caption`（ADR-0032）与 `\index`（ADR-0033）已支持 | 复杂 LaTeX 会降级 | 有真实需求时扩 | 否 |
| D71 | 数学子集有边界 | **主要缺口已关闭（ADR-0030）**：定界符按内容伸缩、`matrix/pmatrix/bmatrix/vmatrix/cases/aligned`、数学字体表（Noto Sans Math 第 5 面）都已实现；仍缺 `\overbrace`/`\underbrace`、`\color`、`array` 列格式、`\substack`、多行公式编号 | 少见的复杂公式回退为文本占位 | 按真实稿件补 | 否 |
| D72 | 行内数学复用显示常量 | **已关闭（ADR-0030）**：`text_frac_scale`/`text_frac_gap`/`text_big_scale` 三组常量；显示分式改为全尺寸分子分母；行内上下限不堆叠 | 仍无数学斜体与字间微调（并入 D71） | ✅ 已关闭 | 否 |
| D73 | ~~目录能力有边界~~ | **已关闭（ADR-0032）**：`toc.leaders`（默认开）生成引导点；`toc.figures`/`toc.tables` 生成图目录/表目录；目录条目带自动编号。仍无索引与 PDF 书签（并入 D79） | — | ✅ 已关闭 | 否 |
| D74 | CLI 只做单文件 | 无目录批量、无 `--stdin`、无 watch、无模板外的元数据覆盖；格式只有 html/svg/json | 批量流程要自己写脚本 | 有自动化需求时补 | 否 |
| D75 | ~~选择器看不到标题级别~~ | **已关闭（ADR-0032）**：`Selector.level: 1..=9` 参与匹配与特异性，可只写 `level` | — | ✅ 已关闭 | 否 |
| D76 | ~~前置页页码不区分~~ | **已关闭（ADR-0032）**：`page.front-matter: continuous/roman/hidden`；前置页=只含生成内容的连续页；`Page.printed` 输出印字标签，`{pages}` 在 roman/hidden 下为正文页数 | — | ✅ 已关闭 | 否 |
| D77 | ~~没有自动编号~~ | **已关闭（ADR-0032）**：`numbering:` 生成中文/点号标题号与图/表号；编号在布局期生成，不进 DocIR；目录条目带编号 | — | ✅ 已关闭 | 否 |
| D78 | ~~题注没有专用样式~~ | **已关闭（ADR-0033）**：`figureCaption`/`tableCaption` 是生成元素，题注内联在题注元素下解析；表题在表上方并 keep-with-next | — | ✅ 已关闭 | 否 |
| D79 | ~~目录行是不可折断的整体~~ | **已关闭（ADR-0033）**：目录/索引标题按文本编译，可在标题盒内换行；页码与引导点跟随最后一行 | — | ✅ 已关闭 | 否 |
| D80 | 索引按码位排序 | 词条按 Unicode 标量排序，没有拼音/笔画排序；`index:` 尚无 `sort` 字段 | 中文索引的排序不符合部分出版习惯 | 需要时加模板字段（不改 DocIR） | 否 |
| D81 | 索引标记覆盖有限 | 表格单元与脚注定义里的 `\index`/`[[index:]]` 不收集；无 `see`/`see also`、无子条目、无页码范围合并 | 复杂索引需人工整理 | 有真实索引需求时扩 | 否 |
| D82 | PDF 后端有边界 | 接受 `glyf` 与 CFF（ADR-0039/0040）；流压缩已做（ADR-0043，固定 Huffman deflate）；图片只画占位框（无 XObject 嵌入）；无 tagged PDF/链接注解；下划线不绘制；PDF 只在 CLI 暴露 | 扫描版/带图/无障碍交付需另做 | 有真实交付需求时扩 | 否 |
| D83 | 部分全角标点没有标点类 | `punct_class` 没有 `…`、`—`、`·` 的条目，它们按 `PunctClass::None` 处理，既无侧空可裁也无法参与禁则（`…` 落在行首不被拦） | 省略号/破折号多的地方行末对齐与禁则会退化 | 扩 `punct_class` 表并补 golden；不改 IR | 否 |
| D85 | ~~name-keyed CFF 不能嵌入~~ | **已关闭（ADR-0040）**：name-keyed 脸被提升为 CID-keyed（合成 ROS/FDArray/FDSelect），`seac` 复合由 Type 2 解释器展开成显式轮廓（子程序内联、hint 丢弃、笔位置跟踪）；解释器不认识的算子直接报错 | — | — | ✅ 已关闭 |
| D84 | ~~行末侧空是常量而不是墨迹边界~~ | **已关闭（ADR-0037）**：`soma.metrics/2` 记录标点的 `ink_left`/`ink_right`，`scale_trim` 优先用真实轮廓；v1 表或无 `glyf` 的字体回退到类别常量 | — | — | ✅ 已关闭 |
| D86 | ~~拉丁文独排时段落词间空会被压到很窄~~ | **已关闭（ADR-0041）**：词间空的可压缩量改为 `min(1/8 em, space_advance/3)`；`overfull` 的齐行行不再当作正常断点，只经 fallback 到达（`modelVersion` 2）。语料 `over_compress_lines` 3 → 0 | — | — | ✅ 已关闭 |
| D69 | HTML 子集有边界 | `soma-html` 只做语义白名单：无 CSS/脚本/表单/框架，无 `<caption>`/`<colgroup>`/脚注标记，实体集有限，`SourceSpan` 为空 | 复杂 HTML 会降级或丢表现 | 有真实需求时扩 | 否 |
| D68 | 表格线只能整表配置 | **已关闭（ADR-0020）**：五条线宽、竖线/左右框、实线/虚线、颜色都进模板；剩余限制是 `table:` 只能全局一套，不能按表/按行覆盖 | 多套表格外观要多个模板 | ✅ 已关闭 | 否 |
| D50 | ~~无页面母版 / 页眉页脚 / 页码~~ | **已关闭（E4）**：页眉（running-title 或字面量）、页脚、`{page}`/`{pages}` 页码；两遍无重排 | 首页/封面变体未做（D62） | ✅ 已关闭 | 否 |
| D51 | 盒子树是扁平的 | **colspan、rowspan、表头重复、单元格跨页续排、行线已支持**（ADR-0017/0018/0019）：页面只在未被跨行单元穿过的行边界断开，续页重画表头并整体平移单元块；表格线占真实垂直空间且跳过跨行单元；仍无嵌套 VBox/HBox、竖线/单元格边框、逐表覆盖线宽 | 嵌套布局与单元格边框受限 | 后续 | 否 |
| D52 | ~~公式是文本占位~~ | **已关闭（ADR-0026）**：`soma-math` 解析 TeX 子集 → 整数 `MathLayout`（符号 + 分数线/根号横线）；`soma-layout` 居中放一个 `MathBox`（带基线），页面 schema 升到 `soma.pages/5`；解析失败仍回退到文本占位。剩余边界见 D71 | — | ✅ 已关闭 | 否 |
| D53 | Markdown 子集有限 | 无嵌套列表、setext 标题、表格、HTML 块、链接引用定义 | 复杂 Markdown 会退化为文本 | 未排期 | 否 |
| D54 | 行内代码无独立度量 | 行内代码仍是单个 `AtomKind::Other` atom，用正文字体；**行内公式已由 ADR-0027 解决**：对象 atom 走 `object_advance`/`object_metrics`，宽高来自数学盒 | 行内代码宽度不准 | 未排期 | 否 |
| D55 | 图不加载、ruby 不渲染 | 图只画占位框；`Ruby` 退化为 base 文本 | 图文混排不完整 | 未排期 | 否 |
| D56 | `Section` 级脚注编号回退 | `numbering: section` 按全文连续处理 | 分节编号不准确 | 未排期 | 否 |
| D57 | `Line` 序列化上提到 kernel | `Line::write_canonical` 是增量 API（页面结果复用），`soma.layout/6` 字节不变 | 无 | — | 否 |

## 3. 非目标（不是债务）

触发条件写清楚，条件不到就不做。

| ID | 项 | 触发条件 |
|---|---|---|
| N1 | Word import + 样式映射 | 有真实 `.docx` 样本需求；只做 import，不承诺版式保真 |
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
| 未排期 | D01、D03、D04、D05、D10、D11、D12、D13、D60、D62、D69、D70、D71、D74、D80、D81、D83、N1–N6 |
