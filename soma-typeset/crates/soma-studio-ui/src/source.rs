//! Source editor and DocIR mapping.
//!
//! A `<textarea>` keeps the editor dependency-free and IME-correct. The gutter
//! is a synced scroll container. Node selection is exact at block level: the
//! canonical DocIR carries byte spans, and the atom stream has no source
//! offsets yet (D103), so a line falls back to its block.

use gloo_timers::future::TimeoutFuture;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::state::{self, use_studio, Mode, Studio};

#[component]
pub fn SourcePane() -> impl IntoView {
    let studio = use_studio();
    let textarea = NodeRef::<html::Textarea>::new();
    let gutter = NodeRef::<html::Div>::new();
    let cursor = RwSignal::new(String::from("1:1"));
    let input_seq = RwSignal::new(0u64);
    let composing = RwSignal::new(false);

    let numbers = Memo::new(move |_| {
        let source = studio.source.get();
        let count = source.lines().count().max(1);
        let mut text = String::with_capacity(count * 4);
        for line in 1..=count {
            if line > 1 {
                text.push('\n');
            }
            text.push_str(&line.to_string());
        }
        text
    });

    let on_scroll = move |_| {
        if let (Some(input), Some(gutter)) = (textarea.get(), gutter.get()) {
            gutter.set_scroll_top(input.scroll_top());
        }
    };

    // Keep the DOM value in sync with programmatic changes (tab switch, file
    // open) without resetting the caret on every keystroke: on input the two
    // strings are already equal.
    Effect::new(move |_| {
        let value = studio.source.get();
        if let Some(input) = textarea.get() {
            if input.value() != value {
                input.set_value(&value);
                let _ = input.set_selection_range(value.len() as u32, value.len() as u32);
            }
        }
    });

    let update_cursor = move || {
        let Some(input) = textarea.get() else {
            return;
        };
        let source = studio.source.get_untracked();
        let pos = input.selection_start().ok().flatten().unwrap_or(0) as usize;
        let byte = utf16_to_byte(&source, pos);
        let before = &source[..byte.min(source.len())];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        cursor.set(format!("{line}:{column}"));
        let node = studio
            .build
            .with(|build| {
                build
                    .as_ref()
                    .and_then(|build| find_node(&build.docir, byte))
            })
            .unwrap_or(-1);
        studio.cursor_node.set(node);
    };

    // Rebuild 220 ms after the last keystroke. IME composition cancels any
    // pending build and schedules a single one on `compositionend`, so the
    // proof does not flicker through intermediate candidates.
    let schedule_rebuild = move || {
        let seq = input_seq.get_untracked() + 1;
        input_seq.set(seq);
        spawn_local(async move {
            TimeoutFuture::new(220).await;
            if input_seq.get_untracked() != seq || composing.get_untracked() {
                return;
            }
            state::rebuild_now(studio);
        });
    };

    let on_input = move |event: leptos::ev::Event| {
        let value = event_target_value(&event);
        let active = studio.active.get_untracked();
        studio.source.set(value.clone());
        studio.tabs.update(|tabs| {
            if let Some(tab) = tabs.get_mut(active) {
                tab.source = value;
                tab.dirty = true;
            }
        });
        update_cursor();
        if !composing.get_untracked() {
            schedule_rebuild();
        }
    };

    // When an inspect row points at a node, select that block after switching
    // back to the source view.
    Effect::new(move |_| {
        if studio.mode.get() != Mode::Source {
            return;
        }
        let node = studio.selection.get().node;
        if node < 0 {
            return;
        }
        if let Some(input) = textarea.get() {
            if let Some((start, end)) = studio.build.with(|build| {
                build
                    .as_ref()
                    .and_then(|build| find_span(&build.docir, node))
            }) {
                let source = studio.source.get_untracked();
                let start = byte_to_utf16(&source, start);
                let end = byte_to_utf16(&source, end);
                let _ = input.set_selection_range(start as u32, end as u32);
                input.focus().ok();
            }
        }
    });

    let docx = move || {
        studio.project.with(|project| {
            project
                .as_ref()
                .map(|project| project.format == soma_studio_core::SourceFormat::Docx)
                .unwrap_or(false)
        })
    };

    let visible =
        move || studio.source_open.get() && matches!(studio.mode.get(), Mode::Split | Mode::Source);
    let width_style = move || {
        if studio.mode.get() == Mode::Split && studio.proof_open.get() {
            format!("flex:0 0 {:.3}%;", studio.split.get() * 100.0)
        } else {
            String::from("flex:1 1 0;")
        }
    };
    view! {
        <section class="pane source-pane" id="source-pane" hidden=move || { !visible() }
            style=width_style>
            <div class="pane-head">
                <span class="badge" id="source-cursor">{move || cursor.get()}</span>
                <div class="fill"></div>
                <button class="mini" on:click=move |_| state::save_active(studio)>"保存"</button>
                <button class="icon-btn small" title="关闭源文面板" aria-label="关闭源文面板"
                    on:click=move |_| { studio.source_open.set(false); state::persist(studio); }>"×"</button>
            </div>
            <div class="pane-body" style="overflow:hidden">
                {move || if docx() {
                    view! {
                        <div class="empty-state">
                            <div class="mark">"D"</div>
                            <strong>"DOCX 是二进制源"</strong>
                            <span>"可以在校样与检视中查看；编辑请在 Word 中完成。"</span>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="editor-wrap">
                            <div class="gutter" node_ref=gutter aria-hidden="true">{move || numbers.get()}</div>
                            <textarea
                                class="source-input"
                                id="source-editor"
                                node_ref=textarea
                                spellcheck="false"
                                autocapitalize="off"
                                autocomplete="off"
                                wrap="off"
                                aria-label="源文编辑器"
                                on:input=on_input
                                on:scroll=on_scroll
                                on:keyup=move |_| update_cursor()
                                on:click=move |_| update_cursor()
                                on:select=move |_| update_cursor()
                                on:compositionstart=move |_| {
                                    composing.set(true);
                                    input_seq.update(|seq| *seq += 1);
                                }
                                on:compositionend=move |_| {
                                    composing.set(false);
                                    schedule_rebuild();
                                }
                            ></textarea>
                        </div>
                    }.into_any()
                }}
            </div>
        </section>
    }
}

/// UTF-16 selection offset → UTF-8 byte offset.
pub fn utf16_to_byte(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= utf16 {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text.char_indices()
        .take_while(|(offset, _)| *offset < byte)
        .map(|(_, character)| character.len_utf16())
        .sum()
}

/// Deepest DocIR node whose source span contains `byte`.
pub fn find_node(value: &serde_json::Value, byte: usize) -> Option<i64> {
    let mut best = None;
    walk_node(value, byte, &mut best);
    best
}

fn walk_node(value: &serde_json::Value, byte: usize, best: &mut Option<i64>) {
    if let Some(object) = value.as_object() {
        if let Some(span) = object.get("span").and_then(|span| span.as_object()) {
            let start = span
                .get("start")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            let end = span
                .get("end")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize;
            if byte >= start && byte < end {
                if let Some(id) = object.get("id").and_then(|value| value.as_i64()) {
                    *best = Some(id);
                }
            }
        }
        for (_, child) in object {
            walk_node(child, byte, best);
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            walk_node(child, byte, best);
        }
    }
}

/// The source span of a node id, for source selection.
pub fn find_span(value: &serde_json::Value, id: i64) -> Option<(usize, usize)> {
    if let Some(object) = value.as_object() {
        if object.get("id").and_then(|value| value.as_i64()) == Some(id) {
            if let Some(span) = object.get("span").and_then(|span| span.as_object()) {
                let start = span.get("start")?.as_u64()? as usize;
                let end = span.get("end")?.as_u64()? as usize;
                return Some((start, end));
            }
        }
        for (_, child) in object {
            if let Some(found) = find_span(child, id) {
                return Some(found);
            }
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            if let Some(found) = find_span(child, id) {
                return Some(found);
            }
        }
    }
    None
}

/// Select the block of the current inspect row when the user asks for it.
pub fn reveal_node(studio: Studio, node: i64) {
    studio.selection.update(|selection| {
        let mut next = selection.clone();
        next.node = node;
        *selection = next;
    });
    studio.mode.set(Mode::Source);
}
