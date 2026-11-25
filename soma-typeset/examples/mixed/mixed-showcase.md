# 混杂文档示例

这是一个包含 **Markdown**、$\LaTeX$ 和 <strong>HTML</strong> 的混合文档。

## 1. Markdown 部分

- 列表项一
- 列表项二
- **加粗文字**
- *斜体文字*

> 这是一段引用文字。

## 2. LaTeX 数学公式

行内公式：质能方程 $E = mc^2$，以及欧拉恒等式 $e^{i\pi} + 1 = 0$。

独立公式：

$$
\int_{-\infty}^{\infty} e^{-x^2} \, dx = \sqrt{\pi}
$$

矩阵示例：

$$
\mathbf{A} = \begin{pmatrix}
1 & 2 & 3 \\
4 & 5 & 6 \\
7 & 8 & 9
\end{pmatrix}
$$

## 3. HTML 部分

<div style="border: 1px solid #ccc; padding: 10px; border-radius: 5px;">
  <p>这是一个 <strong>HTML</strong> 块级元素。</p>
  <ul>
    <li>HTML 列表项</li>
    <li>支持内联样式</li>
  </ul>
</div>

<table>
  <thead>
    <tr><th>名称</th><th>数值</th></tr>
  </thead>
  <tbody>
    <tr><td>$\pi$</td><td>3.14159</td></tr>
    <tr><td>$e$</td><td>2.71828</td></tr>
  </tbody>
</table>

## 4. 混合使用

下面是一个同时包含三种语法的段落：

在 Markdown 中，我们可以写 **加粗**、$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$，以及 <span style="color: red;">红色 HTML 文字</span>。

<details>
<summary>点击展开（HTML 折叠块）</summary>

折叠内容中也可以包含 LaTeX：$\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}$。

</details>
