# 开发笔记

三篇围绕 2026-11-27 静态化迁移的工程笔记。它们不是规范，也不是 ADR；写的是
“为什么这样做、哪里会坏、怎么证明它没坏”。规范冲突时，以 ADR、代码和
`scripts/gates.sh` 为准。

| # | 笔记 | 主题 |
|---|---|---|
| 01 | [把 `/api` 从公网删掉](01-static-api-removal.md) | 攻击面、`is_local` 默认值、Apache 静态宿主、缓存/压缩、静态门禁、回滚 |
| 02 | [浏览器里打开 DOCX](02-browser-file-open.md) | File API、`Arc<Vec<u8>>`、tab 生命周期、拖放、S13 |
| 03 | [让 wasm 校验不说谎](03-wasm-verify-truth.md) | 自比的假绿标、canonical/参数/产物三种哈希、CI 契约、构建纪律 |

## 先读哪篇

- 关心部署与安全：01。
- 关心前端/文件/内存模型：02。
- 关心确定性、哈希与门禁：03。

## 三篇共享的结论

1. **默认值就是安全边界。** `is_local` 的 fallback 必须是 local；域名白名单
   会在 CDN/预览域名上悄悄回到服务端。
2. **静态不等于“少一个服务”。** 它还意味着字体版本、缓存头、CSP、`/api`
   404 和回滚路径都要自己负责。
3. **验证要能指向证据。** 一个绿色徽标必须对应一次可复现的比较；本地 wasm
   不能证明 native 相等，CI 才能。
4. **产物哈希不是行为哈希。** wasm 文件哈希会因行号/元数据变化；布局行为看
   canonical 哈希与测试。

## 复核命令

```sh
cd soma-typeset
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p soma-studio-ui --target wasm32-unknown-unknown
bash scripts/wasm-hash.sh
SOMA_STATIC_SKIP_TRUNK=1 bash scripts/static.sh
bash scripts/studio.sh
```

线上：

```sh
curl -s -o /dev/null -w '%{http_code}\n' https://somadoc.art/api/health   # 404
curl -sI https://somadoc.art/ | grep -i content-security-policy
```
