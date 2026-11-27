# 开发笔记 02：浏览器里打开 DOCX

- 日期：2026-11-27
- 范围：`soma-studio-ui` 的文件选择器、拖放、`DocTab` 生命周期、wasm DOCX 解析
- 结论：静态模式不经过服务器打开 `.md/.html/.tex/.docx`；文本在 wasm 内解码，
  DOCX 原始字节留在 tab 内存里，由 `soma-docx` 解析。

---

## 1. 目标与约束

公网是纯静态的，没有 `/api/file`，也没有上传端点。文件入口只能是浏览器自己的
`File` 对象：

```text
<input type=file> / drop
        │
        ▼
   File → ArrayBuffer → Uint8Array
        │
        ├── .md/.html/.tex → UTF-8 String → DocTab.source
        └── .docx          → bytes       → DocTab.bytes
        │
        ▼
   parse_document(format, source_or_bytes)
        │
        ▼
   layout → SVG proof / PDF
```

这里有一个重要的产品边界：**文件字节不离开浏览器**。没有上传、没有临时目录、
没有服务端清理任务。代价是刷新页面后，用户打开的文件不会自动恢复；要恢复就得
重新选择文件。

---

## 2. 数据模型：为什么 `DocTab` 要带 `Arc<Vec<u8>>`

迁移前，DOCX 的字节放在 `Studio.docx_bytes` 这一个全局槽里；`activate_tab`
发现当前 tab 是 DOCX 时，会重新请求 `/api/asset`。这个设计在服务端模式下勉强
成立，在静态模式下有两个问题：

1. 没有 `/api`，切回已经打开的 DOCX tab 就找不到字节；
2. 同时打开两个 DOCX tab 时，一个全局槽无法表达“每个 tab 自己的源”。

所以把字节放进 tab：

```rust
pub struct DocTab {
    pub id: u64,
    pub path: String,
    pub title: String,
    pub source: String,
    pub format: SourceFormat,
    pub dirty: bool,
    /// Original bytes for a DOCX tab. Text tabs keep `None`.
    pub bytes: Option<Arc<Vec<u8>>>,
}
```

用 `Arc<Vec<u8>>` 而不是 `Vec<u8>`，是因为 `DocTab` 需要 `Clone`（tab 列表、
`activate_tab`、`with_untracked` 都会克隆）。`Arc` 的克隆是引用计数递增，不会
复制几 MB 的 DOCX。真正需要把字节交给 `rebuild_now` 时，才做一次
`(*bytes).clone()`——这次复制是引擎 API 需要 `&[u8]` 之外的拥有权，无法避免，
但每个 tab 只在构建时发生一次。

`activate_tab` 的优先级是：**tab 自带字节 → 静态模式报错 → 服务端模式才回退
到 `/api/asset`**。

```rust
if tab.format == SourceFormat::Docx {
    if let Some(bytes) = tab.bytes.clone() {
        studio.docx_bytes.set(Some((*bytes).clone()));
        rebuild_now(studio);
    } else if studio.local {
        toast(studio, "DOCX 内容已丢失；请重新打开文件", "bad");
    } else {
        // 只有开发服务端才走这里
    }
}
```

`close_tab` 也必须把下一个 tab 的字节写回 `studio.docx_bytes`，否则关闭一个
DOCX 后切到另一个 DOCX，会拿着上一个文档的字节去构建。这个坑在服务端模式下
原本就被全局槽掩盖了，静态模式下才暴露出来。

---

## 3. 文件选择器：一个隐藏的 `<input type=file>`

Leptos 没有内置的文件选择器组件，也不需要引入额外 crate。直接创建 DOM 节点：

```rust
pub fn pick_file(studio: Studio) {
    let input = document.create_element("input")?
        .dyn_into::<web_sys::HtmlInputElement>()?;
    input.set_type("file");
    input.set_attribute("accept", ".md,.markdown,.html,.htm,.tex,.latex,.docx");
    input.set_attribute("style", "display:none");
    body.append_child(&input)?;

    let closure = Closure::once_into_js(move |event: web_sys::Event| {
        let input = event.target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            .unwrap_or_else(|| fallback.clone());
        parent.remove_child(&input);
        let file = input.files().and_then(|files| files.get(0));
        spawn_local(async move { open_upload_file(studio, file).await });
    });
    input.set_onchange(Some(closure.unchecked_ref()));
    input.click();
}
```

几个细节：

- `Closure::once_into_js` 把 Rust 闭包转成一次性 JS 函数；`set_onchange` 需要的
  是 `&js_sys::Function`，所以用 `unchecked_ref()`；
- 选择完成后立刻从 DOM 移除 input。**用户取消时 `change` 不会触发**，这个隐藏
  input 会留到下一次选择；当前实现接受这个极小的泄漏（每次取消一个空元素），
  没有为它引入 `oncancel` 兼容分支；
- `accept` 只是给文件对话框的提示，真正的格式判定仍在
  `SourceFormat::from_path(&name)`；用户强行选 `.png` 会得到“不支持的文件类型”。

读取字节没有用 `FileReader` 的事件回调，而是直接用 `Blob.array_buffer()`：

```rust
let buffer = JsFuture::from(file.array_buffer()).await?;
let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
```

在 `web-sys 0.3.106` 里，`array_buffer` 定义在 `Blob` 上：

```rust
#[wasm_bindgen(method, js_class = "Blob", js_name = "arrayBuffer")]
pub fn array_buffer(this: &Blob) -> js_sys::Promise;
```

`web_sys::File` 通过 `extends = Blob` 拥有到 `Blob` 的 `Deref`，所以
`file.array_buffer()` 能直接编译。`JsFuture` 把 Promise 变成可 `.await` 的
future，`Uint8Array::to_vec()` 把 JS 堆里的字节复制进 wasm 线性内存。到这里，
文件已经完全脱离浏览器的 File 句柄，后续不再依赖 JS 对象生命周期。

文本与 DOCX 的分叉只有一处：

```rust
let (source, bytes) = if format == SourceFormat::Docx {
    (String::new(), Some(Arc::new(bytes)))
} else {
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{name} 不是 UTF-8 文本"))?;
    (text, None)
};
```

DOCX 是 ZIP，不是 UTF-8；文本格式如果出现非法字节，直接报错，不做 Latin-1
猜测。这个选择让错误可解释，也避免“看起来能打开、排版全是乱码”。

---

## 4. 拖放：只拦截真正的文件拖拽

窗口级 drop 的实现比文件选择器短，但更容易误伤。关键不是 `drop`，而是
`dragover`：浏览器默认不允许 drop，必须在 `dragover` 上 `preventDefault()`。
如果无差别阻止，会破坏源文 textarea 里拖动选中文本的行为。

所以先看 `DataTransfer.types()` 里有没有 `"Files"`：

```rust
let has_files = event.data_transfer()
    .map(|data| data.types().iter()
        .any(|value| value.as_string().as_deref() == Some("Files")))
    .unwrap_or(false);
if !has_files {
    return;
}
event.prevent_default();
```

`drop` 里再取第一个文件，走同一个 `open_upload_file`。多文件拖入当前只打开
第一个；这不是技术限制，而是产品上“一个 tab 一个源”的边界，避免一次 drop
突然创建五个激活状态不明确的 tab。

---

## 5. Tab 生命周期：四个必须成对处理的点

### 5.1 同名文件：替换而不是叠加

`open_upload_file` 用 `file.name()` 作为路径。如果已经有一个同名 tab，就替换
它的 `source` / `format` / `bytes`，而不是再推一个同名 tab：

```rust
let existing = studio.tabs.with_untracked(|tabs| {
    tabs.iter().position(|tab| tab.path == name)
});
```

替换的语义是“重新打开同一份文件”，符合编辑器习惯；如果要保留两个版本，用户
应该先改文件名，或者未来做 in-memory VFS 时用路径区分。

### 5.2 新 tab 的 id 不能再用 `len + 1`

`apply_project` 时代只有一个 tab，id 固定为 1。上传功能会不断加 tab，关闭后
再新建时 `len + 1` 可能与仍存活的 id 冲突。现在用：

```rust
let id = tabs.iter().map(|tab| tab.id).max().unwrap_or(0) + 1;
```

id 只用于 Leptos 的 keyed `For` 和 tab 点击定位，不参与 canonical 哈希，所以
只要单调递增即可。

### 5.3 切换 tab 时要同时切换 `source` 与 `docx_bytes`

`activate_tab` 先把当前 textarea 的 `source` 写回当前 tab，再读下一个 tab。
如果下一个是 DOCX，就把 `tab.bytes` 放进 `docx_bytes`；如果是文本，则清空
`docx_bytes`。不清空的直接后果是：从 DOCX 切到 Markdown 后再点“重新排版”，
`format` 是 Markdown，构建会忽略 `docx_bytes`，看起来没事；但下一次切回
DOCX 时，`docx_bytes` 可能是旧 tab 的值——所以清理和恢复必须成对。

### 5.4 保存：文本下载文本，DOCX 下载原始归档

`save_active` 在 DOCX 分支不做“把空字符串写成 .docx”这种事：

```rust
if tab.format == SourceFormat::Docx {
    let Some(bytes) = tab.bytes.clone() else {
        toast(studio, "DOCX 内容不在内存里；请重新打开文件", "bad");
        return;
    };
    api::download(&path, bytes.as_ref().as_slice())?;
    return;
}
```

文本分支仍然走原来的 Blob 下载。DOCX 是不可编辑的二进制源，静态模式下没有
“写回磁盘”的语义，下载原归档是唯一诚实的保存动作。

---

## 6. 侧栏不是文件系统：`tree ∪ tabs`

静态模式没有 `/api/tree`。`local::tree()` 只返回 `welcome.md`；上传的文件只存在
于 tab 列表里。如果侧栏只渲染 `tree`，用户打开 `notes.md` 后只能在 tab 栏看到
它，侧栏的“稿件”列表不会更新。

`visible_files` 把两者合并：

```rust
fn visible_files(studio: &Studio) -> Vec<FileEntry> {
    let mut files = studio.tree.get();
    for tab in studio.tabs.get() {
        if !files.iter().any(|file| file.path == tab.path) {
            files.push(FileEntry {
                path: tab.path.clone(),
                name: tab.title.clone(),
                kind: String::from("file"),
                size: 0,
            });
        }
    }
    files
}
```

这样：

- 默认的 `welcome.md` 来自 tree；
- 上传的文件来自 tabs；
- 关闭 tab，侧栏条目自动消失；
- 点击侧栏条目会先命中 `open_file` 的“已存在 tab”分支，直接激活，不会重新读文件。

没有做持久化 VFS，也没有做 zip/文件夹导入。这是有意的：静态公网的产品单位是
“一个文件”，不是“一个工程”。要支持多文件工程，需要 File System Access API
或内存 VFS，那是另一个决策。

---

## 7. S13：怎么在 headless 里测试文件打开

文件选择器会弹出原生对话框，headless 无法自动点“打开”。但真正容易错的是
“File → bytes → tab → 构建”这段，不是对话框本身。S13 直接构造 `File`：

```rust
let bytes = js_sys::Uint8Array::from(&html[..]);
let parts = js_sys::Array::new();
parts.push(&bytes);
let bits: wasm_bindgen::JsValue = parts.into();
let file = web_sys::File::new_with_buffer_source_sequence(&bits, "upload.html")?;
state::open_upload_file(studio, file).await?;
```

然后断言：

- tab 列表里出现 `upload.html`；
- `format == SourceFormat::Html`；
- `source` 含“上传测试”；
- 构建结束且没有 `build_error`；
- 关闭 tab 后回到原来的文档。

S13 在 `scripts/static.sh` 中运行，也在线上 `?selftest=1&local=1` 中运行。
它证明的是浏览器内的文件路径，不证明操作系统的文件对话框；后者只能靠人工
验收。

---

## 8. 坑清单

1. **全局 `docx_bytes` 不足以表达多个 DOCX tab。** 必须把字节挂到 tab 上。
2. **`activate_tab` / `close_tab` 都要恢复字节。** 只处理激活路径，关闭路径会
   留下上一个文档的字节。
3. **`File` 的 `array_buffer` 来自 `Blob`。** 不要手动拼 `FileReader`，除非
   需要进度事件。
4. **拖放只拦截 `Files`。** 无差别 `preventDefault` 会破坏 textarea 的文本拖动。
5. **文本必须按 UTF-8 解码。** 失败就报错，不要静默替换。
6. **同名文件替换，不要叠加。** 否则侧栏和 tab 会出现两个同名条目。
7. **取消选择会留下一个隐藏 input。** 当前接受；如果以后要严格，监听
   `cancel` 或复用同一个 input。
8. **DOCX 每次 rebuild 都重新解析。** 这是 MVP 的简单性；大 DOCX 需要缓存解析
   结果或 Worker。
9. **关闭 tab 等于丢弃上传文件。** 没有 VFS；重新打开要重新选择。
10. **刷新页面会回到 `welcome.md`。** 文件字节只在当前页面内存里。
