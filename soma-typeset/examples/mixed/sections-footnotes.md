# 注音、断词与结构

[[ruby:汉|hàn]][[ruby:字|zì]]是中文排版的根基，注音应落在基字正上方，不得与上一行相撞。这一段同时提到[[index:汉字]]、[[index:汉字!字形]]与[[index:排版]]，用来检查索引的层级与页码。

## 列表与表格测试

- 第一层：断行是全局优化
  - 第二层甲：标点禁则
  - 第二层乙：行边裁剪
- 另一个第一层：注音与索引

| 项目 | 说明 | 数值 |
| :--- | :--- | ---: |
| 断行 | 全局最优 | 1 |
| 注音 | 基字单原子 | 2 |
| 索引 | 拼音排序 | 3 |

\section{脚注与索引}

脚注\footnote{脚注的高度反过来影响页面可用空间，因此必须与正文一起求解；这里的脚注里也有一个词条\index{脚注词条}，索引会把它记在引用所在页。}改变页面剩余空间：同一页里多一条脚注，正文就少一行，分页点也随之移动。所以脚注不是“排完正文再补”，而是和正文一起解的未知量；过高的一条脚注会把正文挤到下一页，而不是自己从中间断开。

索引在文末汇总，标记写在正文里。宋体\index{宋体}、表格\index{表格}、汉字\index{汉字}与字形\index{汉字!字形}都做了标记；排版\index{排版|seealso{汉字}}既有正文页码，也指向相关词条；旧称\index{旧称|see{汉字}}只有一条交叉引用，不再单独列出页码。索引的排序是模板的决定：这里用拼音，换一份模板可以改成码位。

拼音排序不是逐字查字典：每个汉字映射到一个无声调的音节排名，词条按“是不是汉字、音节排名、码位”逐字比较，拉丁字母与数字排在汉字之前。这样“八”不会排在“阿”前面，同音词也有稳定的先后。

索引词条支持两级以上：\texttt{汉字!字形} 是“汉字”下面的子条目，父条目没有单独标记时也会自动补一行；页码合并时连续三页以上写成起止页，两页仍然分开列，因为两页的区间省不下多少空间。

## 断词

A line of deliberately long English is placed here to exercise hyphenationhyphenationhyphenationhyphenationhyphenationhyphenationhyphenationhyphenation, and the word hyphenation itself should break at one of its pattern points when the measure is narrow. 参见[[index:排版|see{汉字}]]与[[index:阿]]、[[index:八]]、[[index:中]]，它们在索引里应按拼音排列。
