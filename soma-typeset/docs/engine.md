# 引擎配置（`soma.engine/1`）

状态：**已实现**（`crates/soma-engine`，ADR-0016）。
代码是权威，本文解释形状、默认值、错误与调用方式。

引擎配置和模板是两件事：

- **模板**（`soma.template/2`）描述呈现——页面、字体、缩进、母版。
- **引擎配置**（`soma.engine/1`）描述算法——preset、搜索窗口、回退阶梯、垂直代价。

模板不能写引擎字段（Gate A 静态检查）；引擎配置不能写呈现字段。

## 1. 形状

```yaml
schema: soma.engine/1          # 必需
preset: default-v1             # 可选；目前唯一 preset
window: paragraph              # 可选；目前只实现 paragraph
max_kp_atoms: null             # null 或 1..=10000
fallback:                      # 可选；覆盖 default-v1
  allow_emergency: true
  allow_forbidden: true
  allow_overfull: true
  emergency_stretch: 3em
vertical:                      # 可选；覆盖 default_v1
  page_weight: 0
  badness_scale: 100
  overflow_penalty: 1000000
  club_penalty: 10000
  widow_penalty: 10000
```

规则：

- 只有两层：顶层键，以及 `fallback` / `vertical` 下面的键。出现序列、流式集合、
  更深缩进都是错误。
- 未知键、未知段、重复键、用 tab 缩进、值越界都报错，
  格式 `line:column:path:reason`。
- 标量可裸写或加引号；引号外的 `#` 起始注释。
- 长度只接受整数 `em`（`3em`）；这是引擎几何，不是排版单位系统。
- `fallback` / `vertical` 的值覆盖 `default-v1`，然后交给内核的
  `ParamSet::validate` 与垂直模型自身的范围检查。

## 2. 默认值

`fallback` 的默认来自 `ParamSet::default_v1()`：

| 键 | 默认 |
|---|---|
| `allow_emergency` | `true` |
| `allow_forbidden` | `true` |
| `allow_overfull` | `true` |
| `emergency_stretch` | `3em` |

`vertical` 的默认来自 `VerticalParams::default_v1()`：

| 键 | 默认 | 含义 |
|---|---:|---|
| `page_weight` | 0 | 每页固定代价；0 表示只优化 badness |
| `badness_scale` | 100 | 缺一行 = 1000‰ × scale |
| `overflow_penalty` | 1,000,000 | 单块高于版心 |
| `club_penalty` | 10,000 | 段首孤行（页尾只剩第一行） |
| `widow_penalty` | 10,000 | 段末孤行（页首只剩最后一行） |

`max_kp_atoms: null` 表示不限制；设置后由内核的确定性窗口策略决定何时降级为
`Greedy-C`，页面结果里的 `breakerUsed` / `lengthLimited` 会记录。

## 3. 用法

```rust
use soma_engine::EngineConfig;
use soma_layout::{layout_with, PaginationStrategy};

let engine = EngineConfig::parse(source)?;
let pages = layout_with(
    &doc,
    &styled,
    &metrics,
    &engine.break_config(),
    PaginationStrategy::VerticalKp,
    engine.vertical(),
)?;

// 配置身份（用于缓存/复核）
let hash = engine.hash();
let json = engine.canonical_json();
```

因为配置解析成与手写参数**相同的** `ParamSet` / `VerticalParams`，
页面 canonical 不需要新字段：`engineHash` 与 `verticalHash` 已经标识实际生效的模型。
一份等于 `default-v1` 的配置会产出与 `layout(...)` 逐字节相同的页面
（`crates/soma-engine/tests/engine.rs` 锁住这条）。

## 4. 错误示例

```text
3:3: fallback: unknown key
2:1: window: only the paragraph window is implemented
3:1: vertical.widow_penalty: expected a non-negative integer
1:1: fallback.allow_forbidden: requires allow_emergency
```

## 5. 当前边界

- 只有 `default-v1`；新 preset 需要新 ADR 与代码。
- 只有 `paragraph` 窗口；`page` / `chapter` 在配置层就被拒绝。
- 不暴露单项权重：preset 的意义就是让文档不能漂移到未复核的代价模型。
- 解析器是固定形状实现，不复用模板的 YAML 子集（D65）。

## 6. 门禁

- `Gate A`：`soma-engine` 只依赖 `soma-core`、保持 `no_std`、无浮点、声明
  `soma.engine/1`。
- `cargo test -p soma-engine`：15 个测试，覆盖解析、重复键、缩进、窗口、
  preset、越界、canonical 哈希，以及「配置布局 = 手写参数布局」的逐字节等价。
