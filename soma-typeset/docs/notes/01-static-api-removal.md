# 开发笔记 01：把 `/api` 从公网删掉

- 日期：2026-11-27
- 范围：`soma-studio-ui` 的主机判定、Apache 静态部署、S 门禁
- 结论：公网 `somadoc.art` 只服务静态文件；`/api` 从“可达”变成“404”；
  排版、PDF、TikZ 与文件打开都在浏览器里完成。对应的决策记录是
  [ADR-0121](../adr/0121-static-browser-local-studio.md)。

---

## 1. 问题不是“慢”，是“可达”

迁移前，公网 vhost 把 `/` 全量反代到 `127.0.0.1:7070`。浏览器本地模式已经
能完成排版，但同一台 Apache 仍然把整个 Axum API 暴露在公网：

| 路由 | 方法 | 迁移前的实际能力 |
|---|---|---|
| `/api/health` | GET | 公网可达，实测 200 |
| `/api/pdf` | POST | 任意请求可触发一次完整排版 + PDF 生成 |
| `/api/file?path=` | GET/POST | 在项目根内读/写文本文件 |
| `/api/verify` | POST | 服务端 native 构建 |
| `/api/events` | GET | SSE 文件监听 |

`check_origin` 只检查 `Origin` 和 `Sec-Fetch-Site`。这不是鉴权：**不带 Origin
的 curl 可以直接通过**。只要端口或反代还在，`/api/pdf` 就是一个可被刷 CPU 的
入口，`/api/file` 就是一个有写能力的入口。

而浏览器本地模式已经具备全部产品能力：

- 四个前端（Markdown / HTML / LaTeX / DOCX）都在 wasm 里；
- K-P、分页、SVG 校样、PDF、TikZ 都在 wasm 里；
- 默认模板、引擎参数、度量表、断词规则、`welcome.md` 都是 `include_str!` /
  `include_bytes!`；
- 字体是同源静态文件，不是 API。

所以这次不是“优化 API”，而是**把公网的应用服务器整个拿掉**。

---

## 2. 只加 `Require all denied` 为什么不够

最小改动当然可以只在 Apache 里拒绝 `/api`：

```apache
<Location "/api">
    Require all denied
</Location>
```

它能挡住请求，但保留了三个问题：

1. Axum 还在公网主机上，未来任何一次 vhost 编辑都可能把 `/api` 重新放出来；
2. 静态站旁边还有一个需要维护、需要打补丁、需要监控的进程；
3. `is_local()` 仍然是一个域名白名单。

第 3 点是最隐蔽的。迁移前的判定是：

```rust
if search.contains("server=1") { return false; }
if search.contains("local=1")  { return true; }
let host = window.location().hostname().unwrap_or_default();
host == "somadoc.art" || host.ends_with(".somadoc.art")
```

这意味着：把 `dist/` 放到 CDN 预览域名、自定义域名或 IP 上，**默认是 server
模式**，页面会去请求 `/api`。如果那个域名旁边正好还留着 Axum，静态部署就会
悄悄调用一个开发服务器。

---

## 3. 默认值反转：非 loopback 一律 local

新的 `is_local()` 把“本地”变成默认，把“服务端”变成显式选择：

```rust
pub fn is_local() -> bool {
    let Some(window) = web_sys::window() else {
        return true;
    };
    let search = window.location().search().unwrap_or_default();
    let host = window.location().hostname().unwrap_or_default();
    resolve_local(&search, &host)
}

fn resolve_local(search: &str, host: &str) -> bool {
    let flag = |name: &str| {
        search
            .trim_start_matches('?')
            .split('&')
            .any(|pair| pair == format!("{name}=1"))
    };
    if flag("server") { return false; }
    if flag("local")  { return true; }
    !is_loopback(host)
}
```

`is_loopback` 覆盖 `localhost`、`127.0.0.1`、`::1`、`[::1]` 和
`*.localhost`。行为表：

| URL | 模式 |
|---|---|
| `https://somadoc.art/` | local |
| `https://preview.example.net/` | local（修复点） |
| `http://127.0.0.1:7070/` | server（开发工作流） |
| `http://127.0.0.1:7070/?local=1` | local（静态门禁） |
| 任意主机 `?server=1` | server（显式逃生口） |

这三个分支有单元测试，不依赖浏览器窗口：

```rust
assert!(resolve_local("", "preview.example.net"));
assert!(!resolve_local("", "127.0.0.1"));
assert!(!resolve_local("?server=1", "somadoc.art"));
assert!(resolve_local("?local=1", "127.0.0.1"));
```

---

## 4. 静态 vhost：`/api` 由“路由”变成“不存在的文件”

公网 Apache 的核心变化只有一句：`ProxyPass` 消失，`DocumentRoot` 指向
`/opt/somadoc/dist`。

```apache
DocumentRoot /opt/somadoc/dist
<Directory /opt/somadoc/dist>
    Require all granted
    Options -Indexes +FollowSymLinks
    AllowOverride None
</Directory>
```

这里有一个容易写错的细节：**不要加 `FallbackResource /index.html`**。
`/api/health` 会被 fallback 成 `index.html`，HTTP 状态是 200，门禁里
“`/api` 必须 404”就永远过不了，而且会掩盖“API 又回来了”的事实。当前 App
只使用 query/hash 参数，没有客户端路径路由，不需要 history fallback。将来
如果加路径路由，fallback 必须显式排除 `/api`。

缓存与压缩由静态层负责：

```apache
<FilesMatch "\.(js|css|wasm|ttf|otf)$">
    Header set Cache-Control "public, max-age=31536000, immutable"
</FilesMatch>
<Files "index.html">
    Header set Cache-Control "no-cache"
</Files>
```

- hashed JS/CSS/wasm 可以 immutable；
- `index.html` 必须 revalidate，否则新构建会被旧 HTML 挡住；
- `/fonts/v2/` 是版本化路径，字体字节不变就可以长期缓存；
- 服务器没有 `mod_brotli`，所以使用 `mod_deflate`：

```apache
AddOutputFilterByType DEFLATE application/wasm application/javascript \
    text/css text/html text/markdown image/svg+xml
```

CSP 也必须从 Axum 搬到 Apache。迁移前 CSP 由 `check_origin` 的 middleware 加；
去掉 Axum 后如果不补，页面就失去了 `wasm-unsafe-eval`、`frame-ancestors` 等
约束。新 vhost 显式设置了与原来完全一致的 CSP、HSTS、`nosniff` 和
`Referrer-Policy`。

---

## 5. 实测数字

同一版静态 UI 的测量：

| 项目 | 数值 |
|---|---:|
| wasm 原始大小 | 4,136,582 B（约 3.94 MiB） |
| wasm gzip 后 | 1,595,499 B（约 1.52 MiB） |
| `dist/` 总计 | 8.5 MB |
| `/fonts/v2/` 五个字体 | 4.4 MB |
| `index.html` | 1,900 B |
| `welcome.md` | 2.8 KB |
| wasm 内 PDF（3 页示例） | 224 KB / 约 50 ms |
| `/api/health` | 404 |
| `www.somadoc.art` | 200 |
| `http://somadoc.art/` | 301 → HTTPS |

`somadoc.service` 已停止并禁用。公网没有 Axum 进程，也没有 `/api` 日志——
静态文件服务器只在访问日志里留下字体、wasm 和 favicon 的请求。

---

## 6. 门禁：怎么证明“真的没有 API”

新增 `scripts/static.sh`。它不是“再跑一次 S1”，而是换一个宿主：

1. 用 `trunk build --release` 构建 `dist/`；
2. 确认 `/fonts/v2/` 和 `welcome.md` 存在；
3. 用一个**没有 Axum** 的 Python 文件服务器起 `dist/`；
4. 跑 `?selftest=1&local=1` 的 S1/S3–S10/S12/S13；
5. 断言访问日志里没有 `/api`；
6. 最后才请求 `/api/health`，断言 404。

两个顺序陷阱：

- Python 的 `http.server` 默认**不发送 CSP**。S8 和浏览器安全断言需要 CSP，
  所以脚本内嵌了一个自定义 handler，显式补 CSP / `nosniff` / referrer；
- `/api` 日志检查必须放在故意请求 `/api/health` **之前**，否则门禁会被自己的
  404 探测污染。

S13 是这次新增的浏览器检查：构造一个 `File`，打开 HTML，用 HTML 前端重建，
再关闭 tab。它不经过文件选择器（浏览器原生对话框无法在 headless 里自动化），
但覆盖了“File → bytes → wasm 解析 → tab → 构建”这段真正容易出错的路径。

---

## 7. 回滚

切换前保留了三个备份：

```text
/opt/somadoc/backup-20261127/
├── somadoc-art-ssl.conf     # 迁移前的反向代理 vhost
├── dist/                    # 迁移前的服务端 dist
└── dist-static-e609/        # 第一版静态 dist（wasm e609…）
```

回滚路径是：恢复 vhost → `systemctl start somadoc` → `systemctl enable somadoc`
→ `apache2ctl configtest && systemctl reload apache2`。当前静态部署不依赖
systemd，所以即使 `somadoc.service` 被误启动，也不会改变公网行为；真正的边界
是 Apache 的 `DocumentRoot` 和“没有 ProxyPass”。

---

## 8. 这次没有做的事

- **字体没有塞进 wasm。** 字体 4.4 MB，塞进去会让首屏先下完所有字重，且字体
  版本变化会连累引擎包；`@font-face` 还需要 Blob URL。字体继续作为
  `/fonts/v2/` 的版本化静态文件。
- **没有编译期 `public` feature。** `/api` 已经 404，cfg 掉整条服务端分支的
  收益主要是“字符串不在 wasm 里”，维护成本却要摊到 `state/app/verify/shell`。
  留作后续硬化项。
- **没有单文件离线包。** 当前是 `index.html` + 内容哈希资源 + 字体目录；
  单文件需要 `embed-fonts`，见 D105。
- **没有服务端日志。** 公网只付静态流量；如果以后需要统计，应该单独做静态友好
  的方案，而不是把应用服务器请回来。
