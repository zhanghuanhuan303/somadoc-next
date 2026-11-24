# 代码块样张

这份样张用同一份 Plex 模板渲染语言围栏。面板背景、内边距和等宽族都来自
`examples/templates/plex.yaml`，token 颜色来自它的 `syntax:` 表。

## Rust

```rust
use std::collections::HashMap;

/// 统计词频；中文注释与标识符仍用回退字体。
fn word_count(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}

fn main() {
    let counts = word_count("to be or not to be");
    println!("to = {}", counts["to"]);
}
```

## C

```c
#include <stdio.h>
#include <stddef.h>

int main(void) {
    const char *words[] = {"to", "be", "or", "not", "to", "be"};
    size_t count = sizeof(words) / sizeof(words[0]);
    for (size_t i = 0; i < count; i++) {
        printf("%s\n", words[i]);
    }
    return 0;
}
```

## C++

```cpp
#include <iostream>
#include <string>
#include <vector>

int main() {
    std::vector<std::string> words{"to", "be", "or", "not"};
    for (const auto &word : words) {
        std::cout << word << '\n';
    }
    return 0;
}
```

## Shell

```bash
#!/bin/sh
# 生成一份文件清单
set -eu
out=${1:-listing.txt}
for file in *.rs; do
    printf '%s\n' "$file"
done > "$out"
echo "wrote $out"
```

## Markdown

```markdown
# 标题

一段带 **强调**、`行内代码` 和 [链接](https://example.com) 的正文。

- 列表项
- 第二项
```

## LaTeX

```latex
\section{标题}
中文正文，公式 $x^2 + y^2 = z^2$，以及行间公式：
\[
  \frac{a}{b} = \sqrt{c}
\]
```

## 其他常用语言

```python
def word_count(text: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for word in text.split():
        counts[word] = counts.get(word, 0) + 1
    return counts
```

```javascript
const words = "to be or not to be".split(" ");
const counts = new Map();
for (const word of words) {
  counts.set(word, (counts.get(word) ?? 0) + 1);
}
console.log(counts.get("to"));
```

```json
{
  "name": "soma-typeset",
  "versions": [1, 7],
  "stable": true
}
```

```html
<p class="lead">一段 <strong>HTML</strong> 文本。</p>
```
