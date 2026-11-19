# A1 report（`soma-doc`）

状态：**A1 complete.** `soma-doc` 落地，legacy `semantic`/`text` 删除，Gate A 建好并
接入 `gates.sh`。工作区 **159 tests** 全绿，`scripts/gates.sh` 与
`scripts/kp-verify.sh` 全绿。

对应债务：关闭 **D14**（legacy 导出）、**D20**（DocIR）、**D28**（Gate A）、
**D30** 的 `docHash` 部分。

---

## 1. 交付物

| 文件 | 内容 |
|---|---|
| `crates/soma-doc/Cargo.toml` | 新 crate，**零依赖** |
| `crates/soma-doc/src/lib.rs` | crate 契约、`no_std`、再导出 |
| `crates/soma-doc/src/ids.rs` | `NodeId`、`SourceId`、`SourceSpan`、`SourceInfo`、`SourceMap`、`SourceKind` |
| `crates/soma-doc/src/attrs.rs` | `Attrs`（role / classes / anchor / lang）+ 上限常量 |
| `crates/soma-doc/src/node.rs` | `NodeMeta`（id + span + attrs） |
| `crates/soma-doc/src/meta.rs` | `DocumentMeta`、`Author`、`Date`、`TemplateRef`、`CoverData`、`MetaField`、`ImageRef`、`ImageIntrinsic` |
| `crates/soma-doc/src/inline.rs` | 16 个行内变体 + 无歧义 `From` 转换 |
| `crates/soma-doc/src/block.rs` | 13 个块变体 + 表格部件 + 无歧义 `From` 转换 |
| `crates/soma-doc/src/raw.rs` | `RawContent`、`RawCapability` |
| `crates/soma-doc/src/doc.rs` | `Document`、`FootnoteDef`、`renumber`、前序遍历、hash/text 入口 |
| `crates/soma-doc/src/validate.rs` | `DocError`、`MAX_DEPTH`、全量校验 |
| `crates/soma-doc/src/canonical.rs` | 手写 canonical JSON writer、`Full`/`Content` 两模式 |
| `crates/soma-doc/src/hash.rs` | `DocHash`、FNV-1a sink、`doc_hash` / `content_hash` |
| `crates/soma-doc/src/text.rs` | `plain_text` 投影（标题/TOC/alt/诊断用） |
| `crates/soma-doc/tests/*` | 44 个集成测试 + 1 个 doc-test |
| `crates/soma-doc/tests/golden/a1_small.json` | 字节级 golden |
| `scripts/gate-a.sh` | Gate A（架构门禁），已接入 `gates.sh` |
| `docs/docir.md` | DocIR 规范 |

删除：

- `crates/soma-core/src/semantic.rs`（106 行）
- `crates/soma-core/src/text.rs`（264 行）
- `soma-core` 对 `BlockNode` / `ColumnAlignment` / `InlineNode` / `extract_text` /
  `ir_to_markdown` 的公开再导出

## 2. 关键决定

1. **零依赖**：DocIR 不含物理单位，所以 `soma-doc` 不依赖 `soma-core`。Gate A 用
   `cargo tree` 的**行数 == 1** 锁死这一点，比白名单更强。
2. **`NodeMeta` 统一头部**：id/span/attrs 在一个结构里。加节点类型不增加簿记字段，
   renumber/validate/canonical 只需处理节点本身。
3. **Section 是块级标记，不是嵌套树**。分页、running header、增量重排都按文档顺序
   扫描；标记避免「section 树 + block 列表」两套真相。
4. **`anchor` 不叫 `id`**：`id` 已被 `NodeId` 占用，JSON 里两者同时出现会误读。
5. **两种 canonical 模式**：`doc_hash`（含 span/id/source/template）用于精确复现，
   `content_hash`（归零）用于「内容变没变」的缓存键。两者语义不同，容易混淆，所以
   名字和文档都分开写。
6. **`RawCapability` 只是路由提示**：`Preserved` / `Degraded` / `Dropped` 不影响数据
   保留，`value` 永远不丢。
7. **表格列宽不进 IR**：`Column` 只有 `align`。宽度由布局的列算法结合模板决定
   （ADR-0009）。
8. **拒绝空 `Text`**：空文本节点永远是前端 bug，尽早失败比静默 merge 好。

## 3. Gate A1 清单

| # | 项目 | 验证 | 状态 |
|---|---|---|---|
| A1.1 | 每个节点有 id/span/attrs | `maximal_document`（覆盖所有 kind）经 `renumber` + `validate` | ✅ |
| A1.2 | id 唯一且连续 | `renumber_is_deterministic_and_contiguous`、`duplicate_node_ids_are_rejected` | ✅ |
| A1.3 | 未访问节点会被发现 | `renumber_leaves_no_unassigned_ids`（canonical 里不得出现 `"id":0`） | ✅ |
| A1.4 | 未知构造不丢 | `a1_raw.rs` 4 个测试 + canonical 转义断言 | ✅ |
| A1.5 | canonical 字节稳定 | golden `a1_small.json`、`canonical_json_is_stable_across_runs` | ✅ |
| A1.6 | 无浮点 | `canonical_json_has_no_float_tokens`、Gate A grep | ✅ |
| A1.7 | 校验覆盖结构/引用/边界 | `a1_validate.rs` 23 个测试 | ✅ |
| A1.8 | 深度受限 | `nesting_depth_is_bounded`（`MAX_DEPTH = 32`） | ✅ |
| A1.9 | 跨端字节一致 | `soma-wasm` fixture 现含 `docIr` + `layout`，native vs `wasm32-wasip1` 一致（6,782 bytes） | ✅ |
| A1.10 | 层依赖 | `gate-a.sh`：`soma-doc` 零依赖、`soma-core` 不依赖 `soma-doc` | ✅ |
| A1.11 | legacy 已删 | `gate-a.sh` 检查文件与再导出 | ✅ |
| A1.12 | 物理样式不进 IR | Gate A 关键字检查（width/fontSize/margin/color/lineHeight） | ✅ |
| A1.13 | 现有测试不退化 | K1–K6/K8 全通过；仅删除 2 个 legacy 单测 | ✅ |

## 4. 测试

| suite | 数量 |
|---|---:|
| `a1_canonical.rs` | 8 |
| `a1_model.rs` | 9 |
| `a1_raw.rs` | 4 |
| `a1_validate.rs` | 23 |
| doc-test（`lib.rs` 示例） | 1 |
| **`soma-doc` 小计** | **45** |

工作区总数 **159**（原 116 − 2 legacy + 45）。`soma-core` 单测从 25 降为 23。

## 5. 已知限制（已记账）

- 没有 canonical JSON **reader**（当前 write-only）；需要回读时再加（`docs/debt.md` D31）。
- canonical writer 与 `soma-core::result` 的 writer 是两份实现；第三个消费者出现时
  再抽公共 crate（D32）。
- `rowspan > 1` 的网格占位一致性未校验（只校验每行 `colspan` 之和）（D33）。
- `Date` 不校验月份天数（接受 2 月 30 日）；来源已经是日期文本，IR 不发明日历（D34）。
- `Document::for_each_meta` 与 `_mut` 是两条平行遍历，靠测试锁同步（D35）。
- Raw 内容不做安全白名单；这是 Gate C（A5）的职责（D36）。

## 6. 复现

```sh
cargo test -p soma-doc
cargo test --workspace
bash scripts/gate-a.sh
bash scripts/gates.sh
```
