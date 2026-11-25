# 提案：预览改为 SVG 校样，PDF 只在下载时生成

- 状态：提案（待拍板）
- 日期：2026-11-26
- 关联：ADR-0093（全 Rust wasm Studio）、ADR-0103（TikZ 导出）、
  `docs/studio.md`、`crates/soma-render-svg`、`crates/soma-render-pdf`
- 不改变：DocIR、模板、K-P 内核、`PagedLayout` 的 canonical 形状

## 1. 结论

**可以，而且代价比看起来小：现在的预览和 PDF 已经共用同一条排版管线，
只差最后一层渲染器。**

```text
Markdown/LaTeX/HTML/Word
        ↓
DocIR → StyledDoc → PagedLayout（唯一事实）
                         ├── HTML 校样（当前 Studio 预览）
                         ├── SVG（当前 CLI 导出，可做新预览）
                         └── PDF（当前下载）
```

真正不一致的不是布局，而是：

- HTML 校样让浏览器重新排版文字（字体回退、行高、数学定位），
  所以数学/脚注/行盒在屏幕和 PDF 里会“看起来”漂移；
- PDF 使用嵌入字体子集和自己的坐标，屏幕无法逐像素复现。

把预览换成 SVG 后，屏幕上的每个 `x/y` 都来自 `PagedLayout`，与 PDF
共享同一组坐标；PDF 仍然只在用户点“下载”时生成。

## 2. 目标与非目标

### 目标

- 预览默认渲染 SVG；每一页一个 `<svg>`，内部元素带 `data-page/node/atom`。
- 预览不再生成 HTML 校样；HTML 渲染器保留给 HTML 导出/无障碍/测试。
- PDF 仍按需生成，不在每次输入/预览时运行 PDF 后端。
- 预览、SVG、PDF 三者页数、行数、块坐标一致；数学不再漂移。
- 保持 S1–S11 门禁；新增“SVG 与 PDF 坐标一致”检查。

### 非目标

- 不把文字转成路径；SVG 继续输出 `<text>`，保留可复制、可搜索。
- 不追求浏览器与 PDF 的像素级字形一致（同一字体文件 + 固定 atom 坐标已经足够）。
- 不改 `PagedLayout`、canonical JSON、模板 schema。
- 不把 PDF 变成预览格式；PDF 仍然只在下载/打印时生成。

## 3. 现状

| 层 | 代码 | 说明 |
|---|---|---|
| 布局 | `soma-layout` | 唯一 `PagedLayout`，HTML/SVG/PDF 共用 |
| HTML 校样 | `soma-render-html::render_fragments_with` | Studio 当前预览；每页一个 fragment |
| PDF | `soma-render-pdf` | 下载按钮调用；嵌入字体子集 |
| SVG | `soma-render-svg::render` | 整本一个 SVG 字符串；只给 CLI 用 |
| 检查叠加 | `soma-studio-ui/src/proof.rs` | 读取 `PagedLayout` 的行盒，不读 DOM 尺寸 |

因此不需要重写布局、代价模型或断行；只需要：

1. 给 `soma-render-svg` 增加“每页一个 fragment”的 API；
2. Studio 构建结果同时携带 `svg_fragments`（或按页懒生成）；
3. 校样面板挂载 SVG，而不是 HTML；
4. 检查叠加保持现有布局坐标。

## 4. 方案

### 4.1 SVG 分页 API

在 `soma-render-svg` 增加：

```rust
pub struct PageFragment {
    pub width_px: i32,
    pub height_px: i32,
    pub body: String, // <svg viewBox="0 0 W H">…</svg>
}

pub fn render_fragments(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
) -> Vec<PageFragment>;

pub fn render_fragments_with(
    layout: &PagedLayout,
    styles: &StyleTable,
    text: &dyn TextSource,
    assets: &dyn ImageSource,
) -> Vec<PageFragment>;
```

约束：

- 每个 fragment 只含一页；`viewBox` 用页面单位，CSS 宽度 100%。
- 所有文字、数学、规则、图片、TikZ 路径都使用当前 `page_body` 的
  绘制逻辑；不重新计算。
- 每个 atom/math/rule 元素保留 `data-page`、`data-node`、`data-atom`，
  供检查叠加和测试定位。
- 不输出 `<script>`、外链、`<a href>`；图片仍是内联 data URI。

### 4.2 Studio 校样

- `Build` 增加 `svg_fragments: Vec<SvgPage>`（或按需函数）。
- `ProofPane` 把每页挂到一个固定尺寸的容器里；SVG 作为 `innerHTML`，
  外层仍保留 Shadow DOM，避免污染 Studio 样式。
- 页面缩放仍用现有 `zoom`；SVG 的 `viewBox` 自动缩放。
- 检查叠加（行框、margin guide、trim、overfull/emergency 标记）继续
  使用 `PagedLayout` 的行盒坐标，不依赖 SVG DOM 尺寸；
  点击行仍按布局坐标命中。
- 字体：继续用 Studio 的 `@font-face`（正文/标题/等宽/数学）；
  SVG 只引用 family，不嵌入轮廓。
- 如果浏览器缺少某字体，回退仍可能发生；这与 PDF 的嵌入子集不同，
  但 atom 的 `x` 坐标已固定，不会像 HTML 重排那样漂移。

### 4.3 PDF 按需生成

保持现有 `/api/pdf`（POST）行为：

- 预览/输入只构建 `PagedLayout` 与 SVG；
- 下载按钮才调用 `soma-render-pdf`；
- 不在每次 build 时生成 PDF，不缓存 PDF 字节（可缓存短期结果）。

### 4.4 wasm 与原生

- 预览 SVG 在 wasm 内生成：`soma-render-svg` 已是 `no_std + alloc`，
  无新依赖；`soma-studio-ui` 增加一个依赖即可。
- 原生 Studio 也走同一函数；S5/S11 的 native/wasm 哈希继续一致。
- 若 wasm 体积增长超过预算（当前约 3.2 MB），按页懒生成只保留当前页。

## 5. 验收

1. 同一输入下，预览 SVG 的页数、每页 block/line 数与 PDF 相同。
2. 抽查数学页：SVG 与 PDF 的同一个 atom 坐标差 ≤ 1 layout unit
   （用现有 Gate R 的坐标断言）。
3. 预览里数学、脚注标记、行首标点不再出现“屏幕偏一点、PDF 正常”。
4. PDF 只在点击下载时生成；输入 200 ms 防抖期间不触发 PDF 后端。
5. S1–S11 全绿；S8 仍无外部资源、无 script、无 `a[href]`。
6. SVG fragment 不含 `style` 中的外部 URL，不引入新的 CSP 例外。

## 6. 分阶段

| 阶段 | 内容 | 门禁 |
|---|---|---|
| V0 | `soma-render-svg::render_fragments*` + 单测 | 每页 viewBox、data 属性、无 script |
| V1 | Studio `Build` 携带 SVG；校样面板可切换 HTML/SVG | 新开关下截图/页码/行数 |
| V2 | 默认 SVG；检查叠加仍可点选 | S5/S7/S8/S9 |
| V3 | 删除默认 HTML 校样路径，保留 HTML 导出 | S11、HTML 导出测试 |
| V4 | 文档与 debt 更新 | `docs/studio.md`、`docs/debt.md` |

## 7. 风险

| 风险 | 对策 |
|---|---|
| SVG 文本选择/复制退化 | `<text>` 保留；用 `user-select:text`，不转路径 |
| 大文档 SVG DOM 过重 | 只挂当前页；离屏页懒加载 |
| 浏览器字体缺失导致字形不同 | 固定 atom 坐标；Studio 提供字体诊断（S9） |
| 检查叠加与 SVG 坐标不一致 | 叠加继续用 `PagedLayout`，不读 DOM |
| wasm 体积增长 | 按页懒生成；记录 gzip 预算 |

## 8. 一句话

**把“预览”从 HTML 复排版改成 SVG 绘制同一份 `PagedLayout`，
PDF 仍是下载时才跑的第三个消费者；三者共享坐标，预览不再漂移。**
