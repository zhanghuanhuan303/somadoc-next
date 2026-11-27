# 开发笔记 03：让 wasm 校验不说谎

- 日期：2026-11-27
- 范围：`verify.rs`、`shell.rs`、S1/S12/S13、`wasm-hash.sh`、静态门禁
- 结论：本地模式只展示 wasm 构建摘要；native 与 wasm 的字节一致性由 CI 证明，
  不能由浏览器里的“自己和自己比”证明；wasm 文件哈希也不是布局哈希。

---

## 1. 一个看起来通过的绿色徽标

迁移前，本地模式的 `verify_now` 是这样写的：

```rust
if studio.local {
    match studio.summary() {
        Some(summary) => {
            studio.verify.set(Some(summary));
            studio.verify_error.set(None);
        }
        None => { /* 还没有构建 */ }
    }
    return;
}
```

标题栏的 `verify()` 又把它和 wasm 摘要比较：

```rust
match studio.verify.get() {
    Some(native) if native.hash == wasm.hash
        && native.params_hash == wasm.params_hash => {
        (String::from("wasm = native"), "ok", false)
    }
    Some(_) => (String::from("不同哈希"), "bad", false),
    None => (String::from("校验中"), "idle", false),
}
```

`studio.verify` 和 `studio.summary()` 是**同一个 wasm 构建的两个副本**，哈希当然
相等。于是页面显示绿色的 `wasm = native`，但实际上没有任何 native 进程参与。

这比“没有校验”更糟：它把“未检查”变成了“检查通过”。S1 门禁还断言了这个字符串，
等于把错误固化成契约。

---

## 2. 三种“一致性”必须分开

这次修正的核心不是改文案，而是承认三个不同的东西：

| 名称 | 来源 | 它证明什么 | 不证明什么 |
|---|---|---|---|
| `canonicalHash` | `PagedLayout` 的 canonical 字节 | 同一份输入 + 同一套参数，布局结果逐字节相同 | 浏览器里的字体渲染是否完全一样 |
| `paramHash` | 模板、引擎、规则、度量版本 | 两次构建是否可比 | 结果是否正确 |
| wasm 文件哈希 | 构建产物文件名里的内容哈希 | 浏览器缓存失效、资源版本配对 | 引擎行为是否改变 |

CI 的 `scripts/wasm-hash.sh` 比较的是**canonical 字节**，不是 wasm 文件哈希：

```text
== native fixture ==
== wasm32-wasip1 build ==
== wasm32-wasip1 run ==
native/wasm canonical bytes match (9068 bytes)
```

native 与 `wasm32-wasip1` 跑同一个 fixture，输出同样的 canonical 字节。这个结论
可以写进 CI，因为两个目标都能在构建机上执行；它不能由浏览器里的 wasm 单独证明，
因为浏览器里没有 native 二进制。

---

## 3. 一个反直觉的观察：wasm 文件哈希变了，行为不一定变

静态迁移过程中出现过一次产物哈希变化：

```text
e609c32f…_bg.wasm   →   cd63f4f9…_bg.wasm
```

触发变化的是把 `#[cfg(test)] mod tests` 从 `local.rs` 中部移到文件末尾，以及
`cargo fmt`。这段代码不会进入 wasm 的运行时逻辑，但源文件的行号变了。Rust 的
panic 位置、文件/行号元数据以及调试信息会影响最终二进制，所以**wasm 文件的内容
哈希变了**。

这类变化通常来自：

- panic 的 `file!()` / `line!()` 元数据；
- 编译器的布局/去重顺序；
- 依赖版本或 profile 差异。

它不是布局结果变化的证据。反过来也成立：**wasm 文件哈希相同，也不能证明布局
没变**——布局行为应该看 canonical 摘要和测试，而不是看文件名。

工程纪律因此是：

1. 任何会改变二进制元数据的源文件改动（包括移动 `cfg(test)`、格式化导致行号
   变化）都要重新构建并重新部署，避免线上产物和源码不一致；
2. 不要用“wasm 文件名没变”判断“这次改动没影响”；
3. 不要把 `canonicalHash` 和 wasm 文件哈希混在一个门禁里比较。

---

## 4. 修正后的 UI：本地摘要与服务端对照彻底分开

`verify.rs` 现在有两个独立视图，而不是在一个视图里到处 `if studio.local`：

```text
VerifyPane
├── local_verify   → 构建摘要（wasm）
└── server_verify  → 校验：wasm vs native
```

本地视图只做三件事：

1. 显示当前 wasm 构建的 `canonicalHash`、`paramHash`、页数、块数、行数；
2. 显示逐页行数表；
3. 明确写出：native 与 wasm 的字节一致性由 CI 的 `wasm-hash.sh` 锁定。

它不再显示第二个“本地摘要（同一构建）”面板，因为那只是同一份数据的复制。
标题栏徽标改为 `wasm 构建`；面包屑同理。开发服务端模式仍然显示
`wasm = native`，因为那里确实有一个 native 构建可以比较。

S1 也改成了模式感知：

```rust
let verify_label = if studio.local { "wasm" } else { "wasm = native" };
```

这样门禁不会再把“本地自比”当成“native 校验通过”。

---

## 5. 门禁怎么覆盖这条链路

| 门禁 | 覆盖点 |
|---|---|
| `cargo test --workspace` | 1,009 passed / 1 ignored；含 3 个 `is_local` 单元测试 |
| `cargo clippy --workspace --all-targets -- -D warnings` | UI 与内核无警告 |
| `cargo check -p soma-studio-ui --target wasm32-unknown-unknown` | 浏览器构建能编译 |
| `scripts/wasm-hash.sh` | native 与 wasm 的 canonical 字节一致（9,068 B） |
| `scripts/static.sh` | 无 Axum 的静态宿主上 S1/S3–S10/S12/S13 全过；零 `/api`；`/api/health` 404 |
| `scripts/studio.sh` | 开发服务端 S1–S11 全过（S1 期望 `wasm = native`） |
| 线上 headless | 静态包 S1/S3–S10/S12/S13 全过；PDF 224 KB / 50 ms |

S12 是本地 PDF：`soma-render-pdf` 链入 wasm，字体从 `/fonts/v2/` 取；它证明
“校验通过”不是只校验了一个空壳，而是真的能产出 `%PDF-` 字节。S13 是本地文件
打开：构造 `File`，HTML 前端重建，再关闭 tab；它证明静态模式下没有服务端也能
进入构建路径。

---

## 6. 为什么不在浏览器里调用 native

技术上当然可以：让页面把源文 POST 到 `/api/verify`，再比较两个摘要。但这和
ADR-0121 的目标直接冲突：

- 需要保留一个公网应用服务器；
- 需要处理它的鉴权、CPU、文件写入和生命周期；
- 静态部署的全部收益都会被这一条校验请求抵消。

native/wasm 一致性是**发布时的构建属性**，不是**每次打开文档时的运行时属性**。
把它放在 CI 是正确的位置：构建机同时拥有 native 和 wasm 工具链，fixture 固定，
结果可复现；浏览器只负责记录当前 wasm 构建的事实。

---

## 7. 可复现实验

```sh
cd soma-typeset
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p soma-studio-ui --target wasm32-unknown-unknown
bash scripts/wasm-hash.sh
SOMA_STATIC_SKIP_TRUNK=1 bash scripts/static.sh
bash scripts/studio.sh
```

线上复核：

```sh
curl -s -o /dev/null -w '%{http_code}\n' https://somadoc.art/api/health
# 404
curl -sI https://somadoc.art/ | grep -i content-security-policy
# CSP
```

浏览器复核：

```text
https://somadoc.art/?selftest=1&local=1&theme=dark
```

结果应包含 S1、S3–S10、S12、S13，且 S1 的校验徽标是 `wasm`，不是
`wasm = native`。

---

## 8. 结论

- **本地模式不再声称 native 校验。** 它展示 wasm 构建摘要，并说明 CI 的位置。
- **canonical 哈希、参数哈希、wasm 文件哈希是三种东西。** 门禁不能混用。
- **构建产物哈希不是行为哈希。** 行号/元数据会改变它；源文件变化后要重新部署。
- **“绿色徽标”必须能指向一个可复现的比较。** 指不到，就应该显示“已记录”，
  而不是“一致”。
