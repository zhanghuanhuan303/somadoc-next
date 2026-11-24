//! The assistant pane. Local-first and deterministic: it reads the selected
//! line from the build, explains the exported facts, and proposes template
//! YAML. It has no path into the breaker or the renderer.

use gloo_timers::future::TimeoutFuture;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use pulldown_cmark::{Options, Parser};
use web_sys::PointerEvent;

use crate::shell::{default_ai_width, use_drag, Drag, DragKind};
use crate::state::{self, use_studio, AiMessage, Studio};

#[derive(Clone, Debug, Default)]
struct Facts {
    page: usize,
    block: usize,
    line: usize,
    start: u32,
    end: u32,
    ratio: i32,
    adjustment: i32,
    trim_left: i32,
    trim_right: i32,
    boundary: i64,
    boundary_count: usize,
    cost: i64,
    overfull: bool,
    emergency: bool,
    forced: bool,
    hyphenated: bool,
    text: String,
}

#[component]
pub fn AiDock() -> impl IntoView {
    let studio = use_studio();
    let input = NodeRef::<html::Textarea>::new();
    let stream = NodeRef::<html::Div>::new();
    let drag = use_drag();

    Effect::new(move |_| {
        let _ = studio.ai_messages.get();
        if let Some(stream) = stream.get() {
            stream.set_scroll_top(stream.scroll_height());
        }
    });

    let send = move || {
        let Some(input_element) = input.get() else {
            return;
        };
        let question = input_element.value().trim().to_string();
        if question.is_empty() {
            return;
        }
        input_element.set_value("");
        studio.ai_messages.update(|messages| {
            messages.push(AiMessage {
                user: true,
                text: question.clone(),
                badge: "",
            });
        });
        studio.ai_busy.set(true);
        spawn_local(async move {
            TimeoutFuture::new(140).await;
            let reply = local_reply(studio, &question);
            studio.ai_messages.update(|messages| {
                messages.push(AiMessage {
                    user: false,
                    text: reply,
                    badge: "本地 · 规则",
                });
            });
            studio.ai_busy.set(false);
        });
    };

    view! {
        <aside class="ai-dock" class:open=move || { studio.ai_open.get() } id="ai-dock"
            style=move || { if studio.ai_open.get() { format!("width:{}px;flex-basis:{}px", studio.ai_width.get(), studio.ai_width.get()) } else { String::new() } }>
            <div class="resizer" id="ai-resizer" title="拖动调整助手宽度，双击恢复默认"
                on:pointerdown=move |event: PointerEvent| {
                    event.prevent_default();
                    drag.drag.set(Some(Drag {
                        kind: DragKind::Ai,
                        start_x: event.client_x() as f64,
                        start_w: studio.ai_width.get_untracked(),
                    }));
                }
                on:dblclick=move |_| {
                    studio.ai_width.set(default_ai_width());
                    state::persist(studio);
                }></div>
            <div class="ai-header">
                <span class="dot warn" id="ai-status-dot" title="本地规则 · 不联网" aria-label="本地规则 · 不联网"></span>
                <strong>"排版助手"</strong>
                <select id="ai-model" aria-label="AI 模型">
                    <option value="local" selected>"本地 · 规则"</option>
                    <option value="remote" disabled>"远端 · 未配置"</option>
                </select>
                <div class="ai-fill"></div>
                <button class="icon-btn small" title="关闭助手" aria-label="关闭助手"
                    on:click=move |_| studio.ai_open.set(false)>"×"</button>
            </div>
            <div class="ai-stream" id="ai-stream" node_ref=stream>
                {move || {
                    let messages = studio.ai_messages.get();
                    if messages.is_empty() {
                        return view! {
                            <div class="ai-empty">
                                <div class="mark">"✦"</div>
                                <strong>"选中一行，问它为什么这样断"</strong>
                                <p>"本地规则读取 layout 里的比例、裁剪、边界偏差与代价；不联网。"</p>
                                <div class="suggestions">
                                    <button class="suggestion" on:click=move |_| ask_suggestion(studio, "这一行为什么这样断？")>"解释这一行"</button>
                                    <button class="suggestion" on:click=move |_| ask_suggestion(studio, "本页最差的三行在哪？")>"本页最差三行"</button>
                                    <button class="suggestion" on:click=move |_| ask_suggestion(studio, "模板可以怎么调？")>"模板建议"</button>
                                </div>
                            </div>
                        }.into_any();
                    }
                    messages.into_iter().map(|message| {
                        let class = if message.user { "msg user" } else { "msg assistant" };
                        let body = if message.user {
                            view! { <div class="msg-body">{message.text.clone()}</div> }.into_any()
                        } else {
                            view! { <div class="msg-body md" inner_html=markdown(&message.text)></div> }.into_any()
                        };
                        view! {
                            <div class=class>
                                <div class="msg-head">
                                    {if message.user { "你".to_string() } else { "排版助手".to_string() }}
                                    {(!message.badge.is_empty()).then(|| view! { <span class="badge">{message.badge}</span> })}
                                </div>
                                {body}
                            </div>
                        }
                    }).collect_view().into_any()
                }}
            </div>
            <form class="ai-composer" on:submit=move |event| { event.prevent_default(); send(); }>
                <textarea id="ai-input" node_ref=input rows="4"
                    placeholder="例如：这一行为什么被压缩？本页最差的三行在哪？"
                    on:keydown=move |event: leptos::ev::KeyboardEvent| {
                        if event.key() == "Enter" && !event.shift_key() && !event.is_composing() {
                            event.prevent_default();
                            send();
                        }
                    }></textarea>
                <div class="ai-composer-row">
                    <span class="hint">"Enter 发送 · Shift+Enter 换行 · 本地规则不联网"</span>
                    <button class="primary" type="submit" disabled=move || { studio.ai_busy.get() }>"发送"</button>
                </div>
            </form>
        </aside>
    }
}

fn ask_suggestion(studio: Studio, question: &str) {
    studio.ai_messages.update(|messages| {
        messages.push(AiMessage {
            user: true,
            text: question.to_string(),
            badge: "",
        });
    });
    let question = question.to_string();
    studio.ai_busy.set(true);
    spawn_local(async move {
        TimeoutFuture::new(120).await;
        let reply = local_reply(studio, &question);
        studio.ai_messages.update(|messages| {
            messages.push(AiMessage {
                user: false,
                text: reply,
                badge: "本地 · 规则",
            });
        });
        studio.ai_busy.set(false);
    });
}

fn local_reply(studio: Studio, question: &str) -> String {
    let facts = current_facts(studio);
    if question.contains("最差") || question.contains("worst") {
        return worst_reply(studio);
    }
    if question.contains("模板") {
        return template_reply(facts.as_ref());
    }
    match facts {
        Some(facts) => line_reply(&facts),
        None => String::from(
            "先在校样里点一行，或在源文里把光标放进一个段落；我才能读取那一行的 ratio、裁剪与边界偏差。\n\n本地规则不猜布局。",
        ),
    }
}

fn line_reply(facts: &Facts) -> String {
    let mut out = String::new();
    out.push_str("## 这一行\n\n");
    out.push_str("| 指标 | 值 |\n|---|---|\n");
    out.push_str(&format!(
        "| 位置 | 第 {} 页 · 块 {} · 行 {} |\n",
        facts.page + 1,
        facts.block + 1,
        facts.line + 1
    ));
    out.push_str(&format!("| 原子 | {}–{} |\n", facts.start, facts.end));
    out.push_str(&format!("| ratio | **{}‰** |\n", facts.ratio));
    out.push_str(&format!("| 调整 | {} (1/4096 em) |\n", facts.adjustment));
    out.push_str(&format!(
        "| 裁剪 | 左 {} / 右 {} |\n",
        facts.trim_left, facts.trim_right
    ));
    out.push_str(&format!(
        "| 中英边界偏差 | {}（{} 个边界） |\n",
        facts.boundary, facts.boundary_count
    ));
    out.push_str(&format!("| 代价 | {} |\n", facts.cost));
    out.push_str(&format!("| 文本 | `{}` |\n", preview(&facts.text)));
    out.push_str(&format!("| 标记 | {} |\n", flags(facts)));
    out.push_str("\n**判断**：");
    if facts.overfull {
        out.push_str("这一行在容差内仍然超宽，走了 overfull。先看它是不是长英文词/URL，再看模板的 measure 与字号；不要把 `tolerance` 写进模板。");
    } else if facts.emergency {
        out.push_str("这一行触发了 emergency。它通常意味着栏宽、字号与标点禁则一起把可行断点挤没了；先看前后两行，再考虑放宽栏宽或字号。");
    } else if facts.ratio > 500 {
        out.push_str("这一行被明显拉开。中英混排时常见的原因是相邻可行断点太远，或者行末标点被裁掉后剩余空间只能靠拉伸填满。");
    } else if facts.ratio < -500 {
        out.push_str("这一行被明显压缩。先看 `trimLeft/trimRight` 与连续标点；压缩量超过 shrink 上限时会走 overfull。");
    } else {
        out.push_str("比例在正常范围内；没有 overfull/emergency。");
    }
    out.push_str("\n\n");
    out.push_str("**可以试的模板键**（只列模板 YAML 能表达的）：\n\n```yaml\n");
    if facts.trim_left > 0 || facts.trim_right > 0 {
        out.push_str("# 行边标点挤压由规则层给上限；模板侧可检查：\n");
        out.push_str("# punctuation-trim 的上限是否过松\n");
    }
    if facts.ratio.abs() > 500 {
        out.push_str("# 如果整页普遍偏松/偏紧，先调版心而不是单个参数：\n");
        out.push_str("# page.margin-left / page.margin-right\n");
        out.push_str("# body font-size / line-height\n");
    }
    if facts.overfull || facts.emergency {
        out.push_str("# 先确认本段是否有长英文词；再检查：\n");
        out.push_str("# paragraph.measure（或容器的 indent-left/right）\n");
    }
    out.push_str("```\n\n> 本地规则 · 不联网 · 不生成断点");
    out
}

fn worst_reply(studio: Studio) -> String {
    let rows = current_page_rows(studio);
    if rows.is_empty() {
        return String::from("当前页没有可读的行。先在校样里翻到一页。");
    }
    let mut rows = rows;
    rows.sort_by_key(|row| -(row.ratio.unsigned_abs() as i64));
    let mut out = String::from(
        "## 本页最松/最紧的三行\n\n| 行 | ratio | 裁剪 | 代价 | 标记 |\n|---|---:|---:|---:|---|\n",
    );
    for row in rows.iter().take(3) {
        out.push_str(&format!(
            "| {} | {}‰ | {}/{} | {} | {} |\n",
            row.line + 1,
            row.ratio,
            row.trim_left,
            row.trim_right,
            row.cost,
            flags(row)
        ));
    }
    out.push_str("\n点行表里的对应行可以定位覆盖层；`ratio` 的绝对值越大，行越松或越紧。\n\n> 本地规则 · 不联网");
    out
}

fn template_reply(facts: Option<&Facts>) -> String {
    let mut out = String::from("## 模板建议\n\n模板只表达设计，不表达引擎旋钮。\n\n");
    if let Some(facts) = facts {
        out.push_str(&format!(
            "当前选中的行：第 {} 页，ratio {}‰，裁剪 {}/{}，边界偏差 {}。\n\n",
            facts.page + 1,
            facts.ratio,
            facts.trim_left,
            facts.trim_right,
            facts.boundary
        ));
    }
    out.push_str("```yaml\n");
    out.push_str("# 版心：中文长文优先保证每行 28–45 字\n");
    out.push_str("page:\n  margin-left: 25mm\n  margin-right: 25mm\n\n");
    out.push_str("# 正文：行距与首行缩进是模板语言\n");
    out.push_str("body:\n  font-size: 10.5pt\n  line-height: 1.55\n  first-indent: 2em\n\n");
    out.push_str("# 标点：只在行边挤压，不在行中压扁\n");
    out.push_str("punctuation:\n  trim: true\n");
    out.push_str("```\n\n");
    out.push_str("不要写 `tolerance`、`badness_scale`、`max_kp_atoms`；它们是引擎配置，改动会换 `paramHash`。\n\n> 本地规则 · 不联网 · 不生成断点");
    out
}

fn preview(text: &str) -> String {
    let mut out: String = text.chars().take(36).collect();
    if text.chars().count() > 36 {
        out.push('…');
    }
    out.replace('|', "\\|").replace('`', "'")
}

fn flags(facts: &Facts) -> String {
    let mut flags = Vec::new();
    if facts.overfull {
        flags.push("overfull");
    }
    if facts.emergency {
        flags.push("emergency");
    }
    if facts.forced {
        flags.push("forced");
    }
    if facts.hyphenated {
        flags.push("hyphenated");
    }
    if flags.is_empty() {
        String::from("—")
    } else {
        flags.join(", ")
    }
}

fn current_facts(studio: Studio) -> Option<Facts> {
    let selection = studio.selection.get_untracked();
    let target = if selection.block != usize::MAX {
        Some((selection.page, selection.block, selection.line))
    } else {
        let node = studio.cursor_node.get_untracked();
        if node < 0 {
            None
        } else {
            find_first_by_node(studio, node)
        }
    };
    let (page, block, line) = target?;
    studio.build.with(|build| {
        let build = build.as_ref()?;
        let layout_page = build.build.pages.pages.get(page)?;
        let (placed_block, block_index) = if let Some(found) = layout_page.blocks.get(block) {
            (found, block)
        } else {
            let footnote = block.checked_sub(layout_page.blocks.len())?;
            (layout_page.footnotes.get(footnote)?, block)
        };
        let placed = placed_block.lines.get(line)?;
        let line_data = &placed.line;
        let text = (line_data.start..line_data.end)
            .map(|atom| {
                soma_layout::TextSource::atom_text(&build.build.pages.text, placed_block.node, atom)
            })
            .collect::<String>();
        Some(Facts {
            page,
            block: block_index,
            line,
            start: line_data.start,
            end: line_data.end,
            ratio: line_data.ratio_milli,
            adjustment: line_data.adjustment,
            trim_left: line_data.trim_left.0,
            trim_right: line_data.trim_right.0,
            boundary: line_data.boundary_deviation,
            boundary_count: line_data.cjk_latin_adjustments.len(),
            cost: line_data.cost.0,
            overfull: line_data.overfull,
            emergency: line_data.emergency,
            forced: line_data.forced_end,
            hyphenated: line_data.hyphenated,
            text,
        })
    })
}

fn current_page_rows(studio: Studio) -> Vec<Facts> {
    let page = studio.page.get_untracked();
    studio.build.with(|build| {
        let Some(build) = build.as_ref() else {
            return Vec::new();
        };
        let Some(layout_page) = build.build.pages.pages.get(page) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (block_index, block) in layout_page.blocks.iter().enumerate() {
            append_facts(&mut rows, page, build, block_index, block);
        }
        let offset = layout_page.blocks.len();
        for (block_index, block) in layout_page.footnotes.iter().enumerate() {
            append_facts(&mut rows, page, build, offset + block_index, block);
        }
        rows
    })
}

fn append_facts(
    rows: &mut Vec<Facts>,
    page: usize,
    build: &crate::engine::AppBuild,
    block_index: usize,
    block: &soma_layout::PlacedBlock,
) {
    for (line_index, placed) in block.lines.iter().enumerate() {
        let line = &placed.line;
        rows.push(Facts {
            page,
            block: block_index,
            line: line_index,
            start: line.start,
            end: line.end,
            ratio: line.ratio_milli,
            adjustment: line.adjustment,
            trim_left: line.trim_left.0,
            trim_right: line.trim_right.0,
            boundary: line.boundary_deviation,
            boundary_count: line.cjk_latin_adjustments.len(),
            cost: line.cost.0,
            overfull: line.overfull,
            emergency: line.emergency,
            forced: line.forced_end,
            hyphenated: line.hyphenated,
            text: (line.start..line.end)
                .map(|atom| {
                    soma_layout::TextSource::atom_text(&build.build.pages.text, block.node, atom)
                })
                .collect(),
        });
    }
}

fn find_first_by_node(studio: Studio, node: i64) -> Option<(usize, usize, usize)> {
    studio.build.with(|build| {
        let build = build.as_ref()?;
        for (page_index, page) in build.build.pages.pages.iter().enumerate() {
            for (block_index, block) in page.blocks.iter().enumerate() {
                if block.node.0 as i64 == node && !block.lines.is_empty() {
                    return Some((page_index, block_index, 0));
                }
            }
        }
        None
    })
}

/// Markdown for the assistant pane. Raw HTML is escaped and unsafe link or
/// image schemes are replaced, so a document's text can never become markup in
/// the UI (S8).
fn markdown(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(text, options).map(|event| match event {
        pulldown_cmark::Event::Html(html) | pulldown_cmark::Event::InlineHtml(html) => {
            pulldown_cmark::Event::Text(html)
        }
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link {
            link_type,
            dest_url: safe_url(&dest_url).into(),
            title,
            id,
        }),
        pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => pulldown_cmark::Event::Start(pulldown_cmark::Tag::Image {
            link_type,
            dest_url: safe_url(&dest_url).into(),
            title,
            id,
        }),
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

fn safe_url(url: &str) -> String {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        String::from("#")
    } else {
        url.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::markdown;

    #[test]
    fn assistant_html_is_escaped() {
        let html = markdown("<img src=x onerror=alert(1)>");
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains("&lt;img"), "{html}");
    }

    #[test]
    fn assistant_links_reject_script_schemes() {
        let html = markdown("[x](javascript:alert(1)) ![y](data:text/html,boom)");
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("data:text"), "{html}");
    }
}
