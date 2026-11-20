# 提案：浏览器端的 K-P 切片（Web v1）

- 状态：**已拍板并实现**（W1–W4）；决策见 ADR-0071（切片与 ABI）与 ADR-0072（D94 修复）；
  交付物在 `crates/soma-web` 与 `web/`，门禁 `scripts/web.sh`
- 日期：2026-11-21
- 范围：新增 `crates/soma-web`（C ABI 切片）与 `web/`（无构建步骤的 ES 模块 + 演示页）；
  `soma-core` 只加 **3 个带默认值的 `Metrics` 方法**（行为不变）；不改 DP、代价、tie-break。
- 相关：ADR-0007（内核冻结）、ADR-0010（零拷贝与并行）、`docs/architecture-v2.md`；
  本提案通过后新增 **ADR-0071**，并登记 **D94**（emergency 候选窗口）、**D95**（preview 与 PDF 的字节口径）。
- 分支：`dev`
- 落地后的实测修正：模块 32.7 KB gzip（提案估 21.4 KB，差的 11 KB 是 span 编译、
  UTF-8 解码与句柄表）；演示语料（标点密集、1800 atoms、40 em）K-P 7.2 ms、
  180 atoms 0.18 ms；D94 已修，窄栏不再走 40 ms 的 emergency 全扫。

---

## 0. 一句话

**切片 = `soma-core` + `soma-rules`，编译成 21 KB（gzip）、零 import 的 wasm；度量留在 JS（canvas），
宽度以整数进 wasm；内核只补一个「按序号取度量」的 seam。** 典型段落 0.1–1 ms，长段落 2.4 ms，
唯一真正的问题是 **窄栏触发 emergency 后候选数从 3 278 涨到 1 255 320**（40 ms）——这个用宿主策略绕开，
不赌内核。

---

## 1. 实测：先把「优雅极速」变成数字

### 1.1 环境

| 项 | 值 |
|---|---|
| 编译器 | `rustc 1.99.1`（Arch 包，目标 `wasm32-unknown-unknown`） |
| profile | `opt-level="z"`、`lto=true`、`codegen-units=1`、`panic="abort"`、`strip=true` |
| 运行时 | node 25.9.0 与 Chromium 156（headless，`file://` 内联 base64） |
| 语料 | 与 D91 完全同源：`soma-eval::build_paragraph` 的三段基准文本，`justified_style`，`SyntheticMetrics`，`BreakConfig::default()` |

`spike` 是一次性 crate（`/tmp/kpspike`，未入库），只依赖 `soma-core` + `soma-rules`，
用 `extern "C"` 导出三个函数：`kp_spike_rules` / `kp_spike_kp` / `kp_spike_greedy`。

### 1.2 体积

| 组成 | raw | gzip -9 |
|---|---|---|
| `soma-core` 只有 K-P | 42 252 B | 18 820 B |
| **`soma-core` + `soma-rules`**（推荐切片） | **48 944 B** | **21 891 B** |
| 再加 `break_paragraph_traced`（诊断用） | 64 201 B | 28 668 B |

`WebAssembly.Module.imports()` = **`[]`**：没有 WASI、没有 JS 胶水、没有 `wasm-bindgen`。
`new WebAssembly.Instance(module, {})` 直接可用；Chromium 与 node 都一样。

**结论**：整条规则层（标点类、禁则、裁剪上限、连字符、注音）只值 **3 KB gzip**。
没有任何理由在 JS 里再写一遍。

### 1.3 时间（node，40em 栏宽，单位 ms，中位数）

| 语料 | atoms | rules（单次解析） | K-P | Greedy-Furthest |
|---|---|---|---|---|
| 纯中文 | 500 | 0.18 | 0.58 | 0.25 |
| 纯中文 | 2000 | 0.36 | 1.33 | 0.92 |
| 混排 | 500 | 0.05 | 0.43 | 0.16 |
| 混排 | 2000 | 0.22 | 2.38 | 0.96 |
| 标点密集 | 500 | 0.08 | 0.13 | 0.18 |
| 标点密集 | 2000 | 0.37 | 0.60 | 1.04 |

Chromium 同机复测：混排 2000 = 3.10 ms，纯中文 2000 = 1.60 ms，标点 2000 = 0.60 ms。
**wasm ≈ native 的 1.5–2.5 倍**（D91 的原生混排 2000 = 20.23 ms，见下节为什么不能直接比）。

注意 `Greedy` 并不总是更快：标点密集时 K-P（0.60）反而比 greedy（1.04）便宜，
因为 K-P 的候选窗口比贪心的前向扫描更窄。**所以「先贪心再 K-P」不是默认策略，只是降级策略。**

### 1.4 悬崖：窄栏 + emergency = 40 ms

同一段混排 2000 atoms，只改栏宽：

| 栏宽 | `fallback_stage` | 评估候选数 | K-P（node） | K-P（Chromium） |
|---|---|---|---|---|
| 18 em | Emergency | **1 255 320** | 29.0 | 41.1 |
| 20 em | Emergency | **1 255 320** | 28.9 | 40.5 |
| 21 em | Normal | 3 278 | 0.78 | 0.70 |
| 30 em | Normal | 48 891 | 2.12 | — |
| 40 em | Normal | 61 722 | 2.38 | 3.20 |

根因在 `breaker.rs:684`：

```rust
if allow_emergency {
    // Emergency lines may be arbitrarily short or overfull ...
    // Keep the exhaustive scan for the fallback path.
    return (1, n);
}
```

正常阶段用 `(k_min, k_max)` 把候选窗口限制在「一行的可见长度」；
emergency 阶段直接放弃窗口，退化成 `O(n²)`。**这意味着 D91 记的 1.28M 次行评估是 emergency 路径，
不是正常路径。** 同一段落正常路径只要 3 278 次评估、0.78 ms。

这条对浏览器尤其致命：**窄栏正是移动端**。所以它进本提案的降级策略，不作为内核改动硬塞进来（见 D94）。

### 1.5 度量成本（Chromium，canvas）

| 项 | 值 |
|---|---|
| 660 个 CJK 原子逐字 `measureText` | **0.30 ms**（0.45 µs/原子） |
| 首次 `measureText`（字体首次成形） | 3.3 ms |
| `measureText("中")` | 16.000030517578 px（1 em） |

浮点噪声在 1e-5 px 量级，量化到 1/4096 em 无损。
**「逐原子在 JS 里量」是免费的**，不需要宽度表、不需要缓存预热；只有首次成形与字体下载值得处理。

---

## 2. 三个决定

### 2.1 决定一：切片边界 = 规则层 + K-P，不含布局、不含度量、不含渲染

| 进切片 | 出切片 | 理由 |
|---|---|---|
| `soma-core`（IR + DP） | `soma-layout`（盒树/分页/脚注） | 网页端只做行内断行；分页是印刷的事 |
| `soma-rules`（标点/禁则/裁剪/断词/注音） | `soma-metrics`（`.smt` 表） | 度量由浏览器自己给（见 2.2） |
| | `soma-template`/`soma-doc`/渲染后端 | 模板属于服务端或构建期 |

代价：3 KB gzip。收益：**断点与 PDF 路径同源**，不是两套规则各写一遍。

### 2.2 决定二：度量在 JS，宽度以整数进 wasm

浏览器里唯一可信的度量是浏览器自己的：`canvas.measureText`（或未来的 `FontMetrics`）。
所以：

- 原子 → JS 逐原子 `measureText(text).width` → `units = round(px / fontSizePx * 4096)`；
- `space_width`、`hyphen_width`、`ascent/descent/leading` 同样由 JS 给（每个 style 一份）；
- wasm 侧是一个只读数组：`Width(widths[index])`。

**好处**：屏幕上看到的宽度就是我断行用的宽度——preview 自洽，不会有「引擎以为放得下、浏览器画出去 0.4 px」。
**代价**：与 PDF 的字节一致性不再天然成立（D95，见 §7）。

### 2.3 决定三：时序 = 常驻主线程，越界才进 Worker，fallback 就降级

| 情形 | 路径 | 预算 |
|---|---|---|
| 段落 ≤ 800 atoms | **主线程同步**跑 K-P；若回包 `fallback_stage != Normal`，同线程立刻换 greedy 重算并采用 | < 1.5 ms（含降级） |
| > 800 atoms | **Worker**，带 generation 取消；旧版继续显示 | 首帧不阻塞 |
| Worker 里也触发 fallback | `break_paragraph_greedy`，`breaker_used = Greedy` | < 1.1 ms |

800 是阈值而非硬边界：它来自 §6 的「一帧」预算（本机 2000 atoms/3.1 ms × 移动端 4× ≈ 12 ms）。

不让 emergency-KP 上主线程：40 ms = 掉 2–3 帧，而 greedy 与它的视觉差异在一段上几乎看不出来。
`LayoutResult` 已经有 `breaker_used` 与 `fallback_stage`，宿主不需要自造标记。

---

## 3. ABI：数字进、数字出

### 3.1 为什么不用 `wasm-bindgen`

- 它给的是「字符串/结构体自动转换」，而我们的热路径**不该有字符串**；
- 它的胶水与 `serde` 依赖会把 21 KB 变成 60 KB+，还要一个 bundler；
- 我们需要的只是「一块线性内存 + 5 个导出函数」，手写 120 行 JS 就够，而且**没有构建工具链**。

### 3.2 导出面（全部 `extern "C"`，`u32` 句柄 + 指针）

```text
soma_alloc(len: usize) -> *mut u8              // 给 JS 申请输入缓冲区
soma_dealloc(ptr: *mut u8, len: usize)
soma_para_new(text: *const u8, len: usize, style: *const i32) -> u32   // 0 = 错误
soma_para_dump(handle: u32, out: *mut u32, cap: usize) -> u32          // 写入原子表 + 文本块
soma_break(handle, input: *const i32, out: *mut i32, cap: usize) -> u32  // K-P
soma_break_greedy(handle, input: *const i32, out: *mut i32, cap: usize) -> u32
soma_para_free(handle: u32)
```

`style` 7 个 i32：`width, first_indent, hang, tracking, line_height, style_id, flags`（+1 保留）。

### 3.3 原子表（`soma_para_dump`，SoA，每原子 6 个字）

| 偏移 | 内容 |
|---|---|
| w0 | `kind:8 \| class:8 \| flags:8 \| style_id:8`（style ≤ 255，够用） |
| w1..w3 | `text_byte_start`、`text_byte_len`、`text_utf16_len`（指向同一块 UTF-8 文本块） |
| w4..w5 | `compress_left`、`compress_right`（i32） |

文本块 = 所有原子 `text` 的 UTF-8 拼接。JS 一次 `TextDecoder.decode` 得到一个大字符串，
再用 `utf16_len` 逐原子 `slice`（O(n)，无二次解码）。

**边界（boundary）不导出**：JS 不需要它。禁则、glue、penalty、角色都只在 wasm 内部消费。
这样 ABI 面只有「原子」一个概念，规则层改实现也不必动 JS。

### 3.4 断行输入（`soma_break`）

```text
i32[0..8]  = space_width, hyphen_width, ascent, descent, leading,
             padding, padding, padding
i32[8..]   = 每原子 advance（1/4096 em），长度 == atom_count
```

### 3.5 断行输出

```text
i32[0] = line_count
每行 9 个 i32: start, end, trim_left, trim_right, adjustment,
               ascent, descent, leading, flags
i32[..] = box_count
每 box 3 个 i32: atom, x, advance
```

`flags` 位：0 overfull、1 emergency、2 forced_end、3 hyphenated。
（`cost` 不上线：JS 不消费，且 i64 对齐会污染布局。）

box 坐标由 wasm 给出而不是 JS 自己算——**这是「渲染器不重排」在 web 端的落地**：
同一份 `LayoutResult`，PDF/SVG/HTML/canvas 都念同一组 `x`。

### 3.6 调用序列（一次编辑）

```text
text 变更 → soma_para_new/dump（0.2 ms @2000）→ 量宽度（0.3 ms @660）→ soma_break（≤ 2.4 ms）
宽度变更（resize）→ 复用 handle，只重跑 soma_break
参数/preview→faithful 切换 → 复用 handle，只重跑 soma_break
```

两级缓存：**编译后的段落**（按文本版本）与**断行结果**（按 段落版本 × 宽度 × 参数）。

---

## 4. 内核改动：一个 seam，不碰 DP

### 4.1 问题

`Metrics::advance(&self, text: &str, style)` 只能按**文本**取宽度。浏览器拿到的是**位置**（第几个原子）。
今天想绕过它只有两条路，都难看：

- 用调用顺序驱动游标：`Shaped::new` 恰好按原子顺序各调一次，但 `build_result` 之后的
  `line_vertical` 会再按行内次序调一次（`breaker.rs:578`），游标就错了；`Hyphen` 边界还会
  插一次 `advance("-")`（`:473`）。我在 spike 里就是这么写的，`Cell` 还被 `Sync` 拒绝——**这就是缺口的证据**。
- 给每个原子塞一个唯一字符串再查哈希表：2000 原子 × 每次查询一次哈希，白付。

### 4.2 改动（3 个默认方法，默认行为 = 今天）

```rust
// crates/soma-core/src/metrics.rs
/// 按原子序号取度量。默认退化为今天的实现；宿主若能预先测量（浏览器），
/// 可以直接用 `index` 查表，从而不需要原子文本。
fn atom_advance(&self, _index: u32, atom: &Atom, style: StyleId) -> Width {
    match atom.kind {
        AtomKind::InlineObject => self.object_advance(&atom.text, style),
        _ => self.advance(&atom.text, style),
    }
}

fn atom_glyph_metrics(&self, _index: u32, atom: &Atom, style: StyleId) -> GlyphMetrics {
    match atom.kind {
        AtomKind::InlineObject => self.object_metrics(&atom.text, style),
        _ => self.glyph_metrics(&atom.text, style),
    }
}

/// 断行处连字符的宽度。默认沿用 `advance("-")`。
fn hyphen_width(&self, style: StyleId) -> Width {
    self.advance("-", style)
}
```

调用点只有三处：`Shaped::new`（`:425`）、`line_vertical`（`:578`）、连字符（`:473`）。

**为什么这算「默认 0 的扩展」**：所有默认实现逐字复刻今天的分派逻辑，
`SyntheticMetrics`、`TableMetrics`、`MathAdapter` 一个都不用改；
`ParamHash`、`LayoutResult` 字节、语料基线都不该动。验收方式就是跑门禁看 `ALL` 行不动。

**为什么值得改内核而不是在 JS 里绕**：这条 seam 不是为浏览器造的。
它让「宿主已经知道宽度」这件事成为一等公民——未来 ARM 上的字形缓存、服务端预测量、
增量重排版（只重测改动的那几个原子）全都吃这条缝。这是 3 个方法换一类能力。

---

## 5. 前端形态

### 5.1 交付物

```text
web/
  soma-kp.js       # ES 模块，~150 行：加载、内存视图、build/measure/break
  soma-kp.worker.js# Worker 包装（同一份模块，postMessage）
  demo.html        # 单文件演示：可调栏宽/字号、显示 fallback 与耗时
  harness.html     # 门禁用的自检页：零 import、体积、时间、与 native 对拍
```

没有 npm、没有 bundler、没有 CDN。`<script type="module">` 直接 `import`。

### 5.2 渲染策略：DOM 为主，canvas 为可选

| 策略 | 做法 | 优 | 劣 |
|---|---|---|---|
| `render=dom`（默认） | 每行一个 `<span>`，`text-align: justify`；断点来自引擎 | 浏览器自己成形，连字/字距最好；可选中、可搜索、可无障碍 | 行宽与引擎模型有 ~0.1% 的浮点差 |
| `render=canvas` | 按 `box.x/advance` 逐 run 画 | 与 PDF/SVG 逐像素同源 | 失去选择/搜索，要自己做命中测试 |

默认 `dom`：因为度量也来自同一个浏览器，2.3 的「自洽」在 DOM 下依然成立——
引擎说这一行放得下，浏览器也放得下。`canvas` 留给「印刷预览」模式。

### 5.3 首屏

字体没就绪时 `measureText` 会用回退字体（1.5 节：首次成形 3.3 ms，字体本身还要下载）。
所以：`document.fonts.ready` 之前先用**宽度表估算**（同一份 `.smt` 抽出的前 300 个 CJK + ASCII 宽度，
约 1 KB JSON）出一版结果，`ready` 后重排一次。避免「先闪一下再跳」。

### 5.4 取消与并发

Worker 一次只跑一个任务；JS 侧维护 `generation`，回包时比对，过期即丢。
不需要 `SharedArrayBuffer`（那要求 COOP/COEP 头，部署门槛太高），也不需要中断 DP——
单段最坏 40 ms（emergency），而 emergency 已经被 2.3 的降级策略挡在 Worker 之外。

---

## 6. 预算与降级阶梯

| 场景 | 目标 | 实测/推算 |
|---|---|---|
| 典型段（200–400 atoms） | ≤ 1 ms | 0.2–0.5 ms（Chromium） |
| 长段（1000 atoms） | ≤ 2 ms | ~1.6 ms |
| 长段（2000 atoms，normal） | ≤ 5 ms | 3.1 ms |
| 任意段（fallback） | ≤ 2 ms | greedy ≤ 1.1 ms |
| 模块 | ≤ 32 KB gzip | 21.4 KB（28.7 KB 含 trace） |
| 首屏到可读 | ≤ 120 ms | 字体就绪前用宽度表 |

移动端折算：本机是桌面 x86；按 4× 保守估计，典型段仍在 2 ms 内，长段 12 ms——
**刚好是一帧**，这也是 800 atoms 主线程阈值的由来（而不是 2000）。

### D94（新登记）：emergency 阶段没有候选窗口

- **现象**：`allow_emergency` 时 `line_length_bounds` 直接返回 `(1, n)`，评估数 = `O(n²)`。
- **数据**：混排 2000 atoms，18–20 em 栏宽 → 1 255 320 次评估、40 ms；21 em → 3 278 次、0.7 ms。
- **影响**：D91 的基线数字（20.23 ms / 79.0 ms）测的是这条路径，不是正常路径；
  移动端窄栏必然踩到。
- **修法（不在本提案内）**：emergency 下把 `k_max` 用 `emergency_stretch` 界住，
  预期 13× 削减；但 `allow_overfull = true` 时可能改变断点，需要 ADR + 语料 diff。
- **本提案的对策**：宿主看到 `fallback_stage != Normal` 就换 greedy（2.3），零内核风险。

---

## 7. 确定性口径：preview 与 faithful

必须说清楚，否则「同哈希」会变成假的。

| 模式 | 度量来源 | 保证 |
|---|---|---|
| `preview`（默认） | 浏览器 `measureText` | 与**浏览器自己的绘制**自洽；不与 PDF 同字节 |
| `faithful` | 下载 `.smt`，wasm 内解析 | 与 PDF 同断点、同坐标；但与浏览器字体可能有 1–2% 宽度差 |

`faithful` 的实现代价：`soma-metrics` 解析器（小）+ 一个 `.smt` 子集（按字符集裁剪，估 30–80 KB）。
本期只做 `preview`，但 ABI 已经允许换度量源（`atom_advance` 换实现即可），**D95** 记录这件事。

---

## 8. 验收

### 8.1 门禁（新增 `scripts/web.sh`，不进现有 12 项）

| ID | 检查 | 判据 |
|---|---|---|
| W1 | 零 import | `WebAssembly.Module.imports(m).length == 0` |
| W2 | 体积 | gzip ≤ 32 KB |
| W3 | 与 native 对拍 | 同一段落 + 同一宽度表：断点序列、`trim`、`adjustment`、box 坐标逐项相等 |
| W4 | 时间 | 混排 2000 @40em ≤ 5 ms；400 atoms ≤ 1 ms（本机中位数） |
| W5 | 无泄漏 | 同一段落连续 50 次 break，`memory.buffer.byteLength` 不增长 |
| W6 | 降级生效 | 构造一个 ≤20em 的混排段：结果 `breaker_used == Greedy`，且耗时 ≤ 2 ms |

W3 是核心：它证明 ABI 与量化没有把引擎改坏。实现方式是在 `soma-web` 里写一个
`#[cfg(test)]` 的 native 测试，用同一组宽度跑 `break_paragraph`；
`harness.html` 在浏览器里跑同一组，两边把断点序列打到页面/终端再比对（脚本 `web/compare.js`）。

### 8.2 现有门禁必须不动

`soma-core` 的 trait 扩展跑完后：`cargo test --workspace` 770 通过、`scripts/gates.sh` 全绿、
Gate Q′ 语料行 `ALL,48,599,1213,0,0,3,0,0,3,486,1580,377,9` **逐字不变**。
只要有一个字节变了，说明默认实现没复刻对，回头修。

---

## 9. 风险

| 风险 | 影响 | 对策 |
|---|---|---|
| 默认方法没完全复刻旧行为 | 全量 golden 变动 | W3 + 语料基线；先写测试再改分派 |
| 浏览器字体与 `.smt` 不一致 | preview 与 PDF 断点不同 | 明确两种模式（§7），不承诺跨模式同字节 |
| emergency 40 ms 上主线程 | 掉帧 | 阈值 800 atoms + `fallback_stage` 降级（§6） |
| `measureText` 逐原子太慢 | 打字卡顿 | 实测 0.45 µs/原子；>2000 原子时按 run 缓存 |
| DOM 渲染与引擎宽度有浮点差 | 偶发溢出 0.4 px | 给行尾 `padding-inline-end: 1px` 兜底；`render=canvas` 可选 |
| wasm 内存增长 | 长会话泄漏 | 句柄显式 `free` + W5 |
| 移动端没有实测 | 预算乐观 | 阈值按 4× 折算定；真机数据回填 §6 |

---

## 10. 明确不做

- **不做瀑布流/整页排版**：分页、脚注、目录都在服务端；web 端只断行。
- **不做增量断行**（只重算改动那一行）：两级缓存已经够；增量需要 DP 状态外化，另立提案。
- **不做 `faithful` 模式的字符集裁剪工具**：本期只预留 ABI。
- **不做 WebGL/OffscreenCanvas 渲染**：DOM 先跑通。
- **不改 DP、不改代价、不改 tie-break**：内核仍然冻结。
- **不引入 npm 依赖/打包器**：交付物必须是能被 `<script type="module">` 直接吃下的文件。

---

## 11. 顺序与可停点

| 阶段 | 内容 | 出口 | 可停？ |
|---|---|---|---|
| **W0**（半天） | 拍板本文；登记 D94/D95 | 提案合并 | — |
| **W1**（1 天） | 3 个默认方法 + 3 个调用点 + 单测；ADR-0071 | 770 通过、语料基线不变 | 可停：切片可延后 |
| **W2**（2 天） | `crates/soma-web` C ABI + `web/soma-kp.js` | W1/W3 绿 | 可停 |
| **W3**（2 天） | `demo.html` + 主线程/Worker/降级策略 | W4/W6 绿 | 可停 |
| **W4**（1 天） | `harness.html` + `scripts/web.sh` + `docs/web.md` | 六项全绿 | 可停 |

总计约 **6.5 个工作日**。W1 结束就能验证「内核没被动坏」，W2 结束就能在浏览器里看到断行，
是三个天然的停点。

---

## 12. 一句话总结

**用 21 KB、零 import 的 wasm 把「规则 + K-P」原样搬到浏览器；度量交给浏览器，
内核只补一条按序号取宽度的缝；窄栏的 40 ms 悬崖用 greedy 降级绕开，并记为 D94。**
不重写规则、不分裂引擎、不为 web 造第二套断行。
