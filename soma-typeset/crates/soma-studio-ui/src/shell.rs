//! Desktop shell: titlebar, sidebar, tabs, breadcrumb, status bar, palette.
//! The 38/34/29/24 baselines live in `style/studio.css`.

use leptos::html;
use leptos::prelude::*;
use web_sys::PointerEvent;

use crate::state::{
    self, activate_tab, close_tab, export_pdf, export_tikz, go_to_page, open_file, persist,
    rebuild_now, save_active, toggle_theme, use_studio, Aux, Mode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    Sidebar,
    Ai,
    Split,
}

#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub kind: DragKind,
    pub start_x: f64,
    pub start_w: i32,
    /// Container width for a split drag; zero for the side panels.
    pub span: f64,
}

/// Shared between the resizer handles and the window-level drag listener.
#[derive(Clone, Copy)]
pub struct DragCtx {
    pub drag: RwSignal<Option<Drag>>,
}

pub fn use_drag() -> DragCtx {
    use_context::<DragCtx>().expect("DragCtx")
}

#[component]
pub fn Titlebar() -> impl IntoView {
    let studio = use_studio();
    let title = move || {
        studio.tabs.with(|tabs| {
            tabs.get(studio.active.get())
                .map(|tab| tab.title.clone())
                .unwrap_or_else(|| String::from("未打开文档"))
        })
    };
    let verify = move || {
        let Some(wasm) = studio.summary() else {
            return (String::from("—"), "idle", true);
        };
        match studio.verify.get() {
            None => (String::from("校验中"), "idle", false),
            Some(native) if native.hash == wasm.hash && native.params_hash == wasm.params_hash => {
                (String::from("wasm = native"), "ok", false)
            }
            Some(_) => (String::from("不同哈希"), "bad", false),
        }
    };

    view! {
        <header class="titlebar">
            <div class="title-left">
                <button class="icon-btn sidebar-toggle" title="切换侧栏" aria-label="切换侧栏"
                    on:click=move |_| { studio.sidebar_open.update(|open| *open = !*open); persist(studio); }>"◧"</button>
                <span class="brand" aria-hidden="true">"S"</span>
                <strong>"Soma Studio"</strong>
                <span class="workspace-name" id="doc-title">{title}</span>
            </div>
            <button class="command" on:click=move |_| open_palette(studio) aria-label="搜索或运行命令">
                <span>"搜索或运行命令"</span><kbd>"⌘ K"</kbd>
            </button>
            <div class="title-right">
                <span class=move || format!("verify-chip {}", verify().1)
                    id="verify-chip"
                    hidden=move || verify().2
                    title="wasm 与 native 的 canonical 哈希一致"
                    aria-label="校验状态">{move || verify().0}</span>
                <button class="mini title-action" title="并排显示源文与校样" aria-label="并排显示源文与校样"
                    on:click=move |_| state::set_mode(studio, Mode::Split)>"◫ 分栏"</button>
                <button class="mini title-action" title="导出当前文档第一张流程图为 TikZ（服务端）" aria-label="导出 TikZ"
                    on:click=move |_| export_tikz(studio)>"⤴ TikZ"</button>
                <button class="mini title-action" title="导出 PDF（服务端）" aria-label="导出 PDF（服务端）"
                    on:click=move |_| export_pdf(studio)>"⤓ PDF"</button>
                <button class="icon-btn" title="切换主题" aria-label="切换主题"
                    on:click=move |_| toggle_theme(studio)>{move || if studio.theme.get() == "dark" { "☀" } else { "☾" }}</button>
                <button class="icon-btn" title="排版助手（⌘J）" aria-label="排版助手"
                    on:click=move |_| { studio.ai_open.update(|open| *open = !*open); persist(studio); }>"✦"</button>
            </div>
        </header>
    }
}

#[component]
pub fn Sidebar() -> impl IntoView {
    let studio = use_studio();
    let drag = use_drag();
    let files = move || {
        let entries = studio.tree.get();
        if entries.is_empty() {
            return studio.tabs.with(|tabs| {
                tabs.iter()
                    .map(|tab| soma_studio_core::FileEntry {
                        path: tab.path.clone(),
                        name: tab.title.clone(),
                        kind: String::from("file"),
                        size: 0,
                    })
                    .collect::<Vec<_>>()
            });
        }
        entries
    };
    let outline = Memo::new(move |_| {
        studio.build.with(|build| {
            build
                .as_ref()
                .map(|build| build.build.pages.outline.clone())
                .unwrap_or_default()
        })
    });
    let active_path = move || {
        studio.tabs.with(|tabs| {
            tabs.get(studio.active.get())
                .map(|tab| tab.path.clone())
                .unwrap_or_default()
        })
    };
    let width = move || studio.sidebar_width.get();

    view! {
        <aside class="sidebar" class:collapsed=move || { !studio.sidebar_open.get() }
            style=move || { format!("width:{}px;flex-basis:{}px", width(), width()) }>
            <div class="side-head">
                <span>"工作区"</span>
                <div class="fill"></div>
                <span class="faint" id="side-engine">{move || studio.project.with(|project| project.as_ref().map(|project| format!("soma {}", project.engine_version)).unwrap_or_default())}</span>
                <button class="icon-btn small" title="收起侧栏" aria-label="收起侧栏"
                    on:click=move |_| { studio.sidebar_open.set(false); persist(studio); }>"◧"</button>
            </div>
            <nav class="nav" aria-label="工作区导航">
                <div class="nav-group">
                    <span class="nav-title">"稿件"</span>
                    {move || {
                        let current = active_path();
                        files().into_iter().map(|file| {
                            let path = file.path.clone();
                            let active = path == current;
                            view! {
                                <button class="nav-item" class:active=active
                                    title=file.path.clone()
                                    on:click=move |_| open_file(studio, path.clone())>
                                    <span class="nav-label">{file.name}</span>
                                </button>
                            }
                        }).collect_view()
                    }}
                </div>
                <div class="nav-group">
                    <span class="nav-title">"大纲"</span>
                    {move || {
                        let outline = outline.get();
                        if outline.is_empty() {
                            return view! { <div class="nav-empty">"当前文档没有标题"</div> }.into_any();
                        }
                        outline.into_iter().map(|entry| {
                            let page = entry.page.saturating_sub(1) as usize;
                            view! {
                                <button class="nav-item" style=format!("padding-left:{}px", 8 + (entry.level.saturating_sub(1) as i32) * 9)
                                    title=format!("第 {} 页", entry.page)
                                    on:click=move |_| { go_to_page(studio, page); studio.mode.set(Mode::Proof); studio.aux.set(None); }>
                                    <span class="nav-label">{entry.text}</span>
                                    <span class="nav-meta">{entry.printed}</span>
                                </button>
                            }
                        }).collect_view().into_any()
                    }}
                </div>
                <div class="nav-group">
                    <span class="nav-title">"视图"</span>
                    {Mode::ALL.into_iter().map(|mode| view! {
                        <button class="nav-item" class:active=move || { studio.mode.get() == mode && studio.aux.get().is_none() }
                            on:click=move |_| state::set_mode(studio, mode)>
                            <span class="nav-label">{mode.label()}</span>
                        </button>
                    }).collect_view()}
                </div>
                <div class="nav-group">
                    <span class="nav-title">"模板与引擎"</span>
                    <button class="nav-item" class:active=move || { studio.aux.get() == Some(Aux::Template) }
                        on:click=move |_| studio.aux.set(Some(Aux::Template))>
                        <span class="nav-label">"模板 YAML"</span>
                        <span class="nav-meta">{move || studio.project.with(|project| project.as_ref().and_then(|project| project.template_path.as_ref().map(|path| state::file_name(path))).unwrap_or_else(|| String::from("built-in")))}</span>
                    </button>
                    <button class="nav-item" class:active=move || { studio.aux.get() == Some(Aux::Engine) }
                        on:click=move |_| studio.aux.set(Some(Aux::Engine))>
                        <span class="nav-label">"引擎参数"</span>
                        <span class="nav-meta">"read-only"</span>
                    </button>
                </div>
                <div class="nav-group">
                    <span class="nav-title">"诊断"</span>
                    <button class="nav-item" class:active=move || { studio.mode.get() == Mode::Verify && studio.aux.get().is_none() }
                        on:click=move |_| state::set_mode(studio, Mode::Verify)>
                        <span class="nav-label">"wasm vs native"</span>
                    </button>
                    <button class="nav-item" class:active=move || { studio.aux.get() == Some(Aux::Capabilities) }
                        on:click=move |_| studio.aux.set(Some(Aux::Capabilities))>
                        <span class="nav-label">"能力矩阵"</span>
                    </button>
                    <button class="nav-item" class:active=move || { studio.aux.get() == Some(Aux::Fonts) }
                        on:click=move |_| studio.aux.set(Some(Aux::Fonts))>
                        <span class="nav-label">"字体"</span>
                    </button>
                    <button class="nav-item" class:active=move || { studio.aux.get() == Some(Aux::About) }
                        on:click=move |_| studio.aux.set(Some(Aux::About))>
                        <span class="nav-label">"关于"</span>
                    </button>
                </div>
            </nav>
            <div class="side-foot">
                <span id="side-doc">{move || studio.project.with(|project| project.as_ref().map(|project| project.input_relative.clone()).unwrap_or_default())}</span>
                <span class="mono" id="side-hash">{move || studio.summary().map(|summary| format!("p:{}", &summary.params_hash[..8])).unwrap_or_else(|| String::from("—"))}</span>
            </div>
            <div class="resizer" title="拖动调整侧栏宽度，双击恢复默认"
                on:pointerdown=move |event: PointerEvent| begin_drag(event, DragKind::Sidebar, studio, drag)
                on:dblclick=move |_| { studio.sidebar_width.set(default_sidebar_width()); persist(studio); }></div>
        </aside>
    }
}

#[component]
pub fn Tabs() -> impl IntoView {
    let studio = use_studio();
    let tabs = move || studio.tabs.get();
    view! {
        <div class="tabs" id="tabs">
            <div class="tab-scroll" id="tab-scroll">
                <For
                    each={move || tabs()}
                    key={|tab| tab.id}
                    children={move |tab| {
                        let id = tab.id;
                        // A keyed `For` keeps the child closure of a surviving
                        // tab; an index captured here would go stale as soon as
                        // a tab before it is closed. Resolve the index by id at
                        // click time.
                        let active = move || {
                            studio.tabs.with(|tabs| {
                                tabs.get(studio.active.get())
                                    .map(|tab| tab.id == id)
                                    .unwrap_or(false)
                            })
                        };
                        let title = tab.title.clone();
                        view! {
                            <div class="tab" class:active=active data-page=tab.path.clone()
                                on:click=move |_| {
                                    if let Some(index) = studio
                                        .tabs
                                        .with_untracked(|tabs| tabs.iter().position(|tab| tab.id == id))
                                    {
                                        activate_tab(studio, index);
                                    }
                                }>
                                <span class="tab-label">{title}</span>
                                {tab.dirty.then(|| view! { <span class="tab-dirty" title="未保存"></span> })}
                                <button class="tab-close" title="关闭" aria-label="关闭标签"
                                    on:click=move |event| {
                                        event.stop_propagation();
                                        if let Some(index) = studio
                                            .tabs
                                            .with_untracked(|tabs| tabs.iter().position(|tab| tab.id == id))
                                        {
                                            close_tab(studio, index);
                                        }
                                    }>"×"</button>
                            </div>
                        }
                    }}
                />
                <button class="tab-add" title="打开文件（⌘K）" aria-label="打开文件"
                    on:click=move |_| open_palette(studio)>"+"</button>
            </div>
            <div class="tabs-fill"></div>
            <button class="icon-btn" title="重新排版（⌘↵）" aria-label="重新排版"
                on:click=move |_| rebuild_now(studio)>"↻"</button>
        </div>
    }
}

#[component]
pub fn Breadcrumb() -> impl IntoView {
    let studio = use_studio();
    let pages = move || studio.page_count();
    let show_pages = move || {
        matches!(studio.mode.get(), Mode::Split | Mode::Proof | Mode::Inspect)
            && studio.proof_open.get()
            && studio.build.with(|build| build.is_some())
    };
    let zoom_value = move || {
        if studio.fit.get() {
            String::from("fit")
        } else {
            format!("{}", studio.zoom.get())
        }
    };
    let chip = move || {
        let Some(wasm) = studio.summary() else {
            return ("chip", String::from("—"), true);
        };
        match studio.verify.get() {
            None => ("chip", String::from("校验中"), false),
            Some(native) if native.hash == wasm.hash => {
                ("chip ok", String::from("wasm = native"), false)
            }
            Some(_) => ("chip bad", String::from("哈希不一致"), false),
        }
    };

    view! {
        <div class="breadcrumb" id="breadcrumb">
            <div class="page-controls" hidden=move || { !show_pages() }>
                <button class="page-btn" title="上一页" aria-label="上一页"
                    on:click=move |_| { let page = studio.page.get().saturating_sub(1); go_to_page(studio, page); }>"‹"</button>
                <input class="page-input" inputmode="numeric" aria-label="页码" title="页码"
                    prop:value=move || { (studio.page.get() + 1).to_string() }
                    on:change=move |event| {
                        if let Ok(value) = event_target_value(&event).parse::<usize>() {
                            go_to_page(studio, value.saturating_sub(1));
                        }
                    }/>
                <span class="faint" id="page-total">{move || format!("/ {}", pages())}</span>
                <button class="page-btn" title="下一页" aria-label="下一页"
                    on:click=move |_| { let page = (studio.page.get() + 1).min(pages().saturating_sub(1)); go_to_page(studio, page); }>"›"</button>
                <select class="zoom-select" aria-label="缩放" title="缩放"
                    prop:value=zoom_value
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        if value == "fit" {
                            studio.fit.set(true);
                        } else if let Ok(zoom) = value.parse::<f64>() {
                            studio.fit.set(false);
                            studio.zoom.set(zoom);
                        }
                    }>
                    <option value="fit">"适应宽度"</option>
                    <option value="0.5">"50%"</option>
                    <option value="0.75">"75%"</option>
                    <option value="1">"100%"</option>
                    <option value="1.25">"125%"</option>
                    <option value="1.5">"150%"</option>
                    <option value="2">"200%"</option>
                </select>
                <button class="page-btn" title="适应宽度" aria-label="适应宽度"
                    on:click=move |_| studio.fit.set(true)>"⤢"</button>
                <button class="page-btn" title="显示/隐藏检视覆盖层（⌘I）" aria-label="显示/隐藏检视覆盖层"
                    on:click=move |_| studio.overlay.update(|value| *value = !*value)>"▦"</button>
            </div>
            <div class="crumb-fill"></div>
            <span class=move || { chip().0 } hidden=move || { chip().2 }>{move || chip().1}</span>
        </div>
    }
}

#[component]
pub fn Statusbar() -> impl IntoView {
    let studio = use_studio();
    let summary = move || studio.summary();
    view! {
        <footer class="statusbar">
            <span class="status-item" id="status-engine">{move || studio.project.with(|project| project.as_ref().map(|project| format!("soma {}", project.engine_version)).unwrap_or_else(|| String::from("soma —")))}</span>
            <span class="status-item mono" id="status-hash">{move || summary().map(|summary| format!("p:{}", &summary.params_hash[..8])).unwrap_or_else(|| String::from("—"))}</span>
            <span class="status-item optional" id="status-doc">{move || summary().map(|summary| format!("{} 页 · {} 行", summary.pages, summary.lines)).unwrap_or_default()}</span>
            <span class="status-item optional" id="status-layout">{move || summary().map(|summary| format!("{} ms", summary.elapsed_ms)).unwrap_or_default()}</span>
            <span class=move || format!("font-probe status-item {}", studio.font_probe.get().map(|probe| probe.class).unwrap_or("idle"))
                id="font-probe"
                title=move || studio.font_probe.get().map(|probe| probe.detail).unwrap_or_else(|| String::from("字体探针等待构建"))
                aria-label="字体一致性诊断">
                {move || studio.font_probe.get().map(|probe| probe.label).unwrap_or("字体 —")}
            </span>
            <div class="status-fill"></div>
            <span class="status-item status-mode" id="status-mode">{move || studio.mode.get().label()}</span>
            <span class="status-item optional" id="status-tabs">{move || format!("{} 个标签", studio.tabs.with(|tabs| tabs.len()))}</span>
            <span class="status-item optional" id="status-zoom">{move || if studio.fit.get() { format!("适应 · {:.0}%", studio.zoom.get() * 100.0) } else { format!("{:.0}%", studio.zoom.get() * 100.0) }}</span>
            <span class="status-item"><span class="mono">"⌘K"</span>" 命令"</span>
            <span class="status-item"><span class="mono">"⌘J"</span>" 助手"</span>
        </footer>
    }
}

/* ------------------------------------------------------------------ palette */

#[derive(Clone, Debug, PartialEq)]
struct PaletteItem {
    id: String,
    label: String,
    hint: String,
    group: &'static str,
}

#[component]
pub fn Palette() -> impl IntoView {
    let studio = use_studio();
    let input = NodeRef::<html::Input>::new();
    let selected = RwSignal::new(0usize);
    let items = Memo::new(move |_| palette_items(&studio));
    let filtered = Memo::new(move |_| {
        let query = studio.palette_query.get().to_lowercase();
        items
            .get()
            .into_iter()
            .filter(|item| {
                query.is_empty()
                    || item.label.to_lowercase().contains(&query)
                    || item.hint.to_lowercase().contains(&query)
                    || item.group.to_lowercase().contains(&query)
                    || item.id.to_lowercase().contains(&query)
            })
            .collect::<Vec<_>>()
    });

    Effect::new(move |_| {
        let _ = studio.palette_query.get();
        selected.set(0);
    });
    Effect::new(move |_| {
        if studio.palette_open.get() {
            if let Some(input) = input.get() {
                input.focus().ok();
                input.select();
            }
        }
    });

    let run = move |index: usize| {
        if let Some(item) = filtered.get().get(index).cloned() {
            run_item(studio, &item.id);
            studio.palette_open.set(false);
        }
    };

    view! {
        <div class="palette-overlay" hidden=move || { !studio.palette_open.get() }
            on:click=move |_| studio.palette_open.set(false)>
            <div class="palette" role="dialog" aria-modal="true" aria-label="命令面板"
                on:click=move |event| event.stop_propagation()>
                <div class="palette-input">
                    <span class="faint">"⌘"</span>
                    <input node_ref=input placeholder="搜索命令、文件或大纲…" autocomplete="off" spellcheck="false"
                        on:input=move |event| studio.palette_query.set(event_target_value(&event))
                        on:keydown=move |event: leptos::ev::KeyboardEvent| {
                            let count = filtered.with(|items| items.len());
                            match event.key().as_str() {
                                "ArrowDown" => { event.prevent_default(); if count > 0 { selected.update(|value| *value = (*value + 1) % count); } }
                                "ArrowUp" => { event.prevent_default(); if count > 0 { selected.update(|value| *value = (*value + count - 1) % count); } }
                                "Enter" => { event.prevent_default(); run(selected.get_untracked()); }
                                "Escape" => { event.prevent_default(); studio.palette_open.set(false); }
                                _ => {}
                            }
                        }/>
                    <kbd>"ESC"</kbd>
                </div>
                <div class="palette-list" id="palette-list">
                    {move || {
                        let items = filtered.get();
                        if items.is_empty() {
                            return view! { <div class="nav-empty">"没有匹配的命令"</div> }.into_any();
                        }
                        let mut last_group = "";
                        items.into_iter().enumerate().map(|(index, item)| {
                            let group = if item.group != last_group {
                                last_group = item.group;
                                Some(view! { <div class="palette-group">{item.group}</div> })
                            } else {
                                None
                            };
                            let active = move || selected.get() == index;
                            view! {
                                {group}
                                <button class="palette-row" class:active=active data-id=item.id.clone()
                                    on:mouseenter=move |_| selected.set(index)
                                    on:click=move |_| run(index)>
                                    <span class="icon">{item_kind(&item.id)}</span>
                                    <span class="label">{item.label}</span>
                                    <span class="hint">{item.hint}</span>
                                </button>
                            }
                        }).collect_view().into_any()
                    }}
                </div>
                <div class="palette-foot">
                    <span>"↑↓ 选择"</span><span>"↵ 执行"</span><span>"ESC 关闭"</span>
                    <div class="crumb-fill"></div>
                    <span id="palette-count">{move || filtered.with(|items| items.len()).to_string()}</span>
                </div>
            </div>
        </div>
    }
}

fn palette_items(studio: &crate::state::Studio) -> Vec<PaletteItem> {
    let mode = |mode: Mode| PaletteItem {
        id: format!("mode:{}", mode.as_str()),
        label: format!("视图：{}", mode.label()),
        hint: String::new(),
        group: "视图",
    };
    let mut items = vec![
        mode(Mode::Source),
        mode(Mode::Proof),
        mode(Mode::Inspect),
        mode(Mode::Verify),
        PaletteItem {
            id: String::from("rebuild"),
            label: String::from("重新排版"),
            hint: String::from("⌘↵"),
            group: "文件",
        },
        PaletteItem {
            id: String::from("save"),
            label: String::from("保存当前源文"),
            hint: String::from("⌘S"),
            group: "文件",
        },
        PaletteItem {
            id: String::from("tikz"),
            label: String::from("导出 TikZ（第一张流程图）"),
            hint: String::new(),
            group: "文件",
        },
        PaletteItem {
            id: String::from("pdf"),
            label: String::from("导出 PDF（服务端）"),
            hint: String::new(),
            group: "文件",
        },
        PaletteItem {
            id: String::from("fit"),
            label: String::from("适应宽度"),
            hint: String::new(),
            group: "视图",
        },
        PaletteItem {
            id: String::from("zoom:1"),
            label: String::from("缩放 100%"),
            hint: String::new(),
            group: "视图",
        },
        PaletteItem {
            id: String::from("zoom:1.5"),
            label: String::from("缩放 150%"),
            hint: String::new(),
            group: "视图",
        },
        PaletteItem {
            id: String::from("theme"),
            label: String::from("切换深色 / 浅色主题"),
            hint: String::new(),
            group: "界面",
        },
        PaletteItem {
            id: String::from("sidebar"),
            label: String::from("折叠 / 展开侧栏"),
            hint: String::new(),
            group: "界面",
        },
        PaletteItem {
            id: String::from("ai"),
            label: String::from("显示 / 隐藏排版助手"),
            hint: String::from("⌘J"),
            group: "界面",
        },
        PaletteItem {
            id: String::from("aux:template"),
            label: String::from("打开模板 YAML"),
            hint: String::new(),
            group: "模板",
        },
        PaletteItem {
            id: String::from("aux:engine"),
            label: String::from("打开引擎参数"),
            hint: String::from("read-only"),
            group: "模板",
        },
        PaletteItem {
            id: String::from("aux:capabilities"),
            label: String::from("打开能力矩阵"),
            hint: String::new(),
            group: "诊断",
        },
        PaletteItem {
            id: String::from("aux:fonts"),
            label: String::from("打开字体"),
            hint: String::new(),
            group: "诊断",
        },
        PaletteItem {
            id: String::from("aux:about"),
            label: String::from("关于 Soma Studio"),
            hint: String::new(),
            group: "诊断",
        },
    ];
    for file in studio.tree.get() {
        items.push(PaletteItem {
            id: format!("file:{}", file.path),
            label: format!("打开：{}", file.name),
            hint: file.path,
            group: "稿件",
        });
    }
    studio.build.with(|build| {
        if let Some(build) = build {
            for entry in &build.build.pages.outline {
                items.push(PaletteItem {
                    id: format!("page:{}", entry.page.saturating_sub(1)),
                    label: entry.text.clone(),
                    hint: format!("第 {} 页", entry.page),
                    group: "大纲",
                });
            }
        }
    });
    items
}

fn item_kind(id: &str) -> &'static str {
    if id.starts_with("mode:") {
        "◫"
    } else if id.starts_with("file:") {
        "▤"
    } else if id.starts_with("page:") {
        "§"
    } else if id.starts_with("aux:") {
        "⚙"
    } else {
        "›"
    }
}

fn run_item(studio: crate::state::Studio, id: &str) {
    if let Some(mode) = id.strip_prefix("mode:") {
        match mode {
            "source" => state::set_mode(studio, Mode::Source),
            "proof" => state::set_mode(studio, Mode::Proof),
            "inspect" => state::set_mode(studio, Mode::Inspect),
            "verify" => state::set_mode(studio, Mode::Verify),
            _ => {}
        }
        return;
    }
    if let Some(file) = id.strip_prefix("file:") {
        open_file(studio, file.to_string());
        return;
    }
    if let Some(page) = id.strip_prefix("page:") {
        if let Ok(page) = page.parse::<usize>() {
            go_to_page(studio, page);
            state::set_mode(studio, Mode::Proof);
        }
        return;
    }
    if let Some(aux) = id.strip_prefix("aux:") {
        studio.aux.set(match aux {
            "template" => Some(Aux::Template),
            "engine" => Some(Aux::Engine),
            "capabilities" => Some(Aux::Capabilities),
            "fonts" => Some(Aux::Fonts),
            _ => Some(Aux::About),
        });
        return;
    }
    match id {
        "rebuild" => rebuild_now(studio),
        "save" => save_active(studio),
        "tikz" => export_tikz(studio),
        "pdf" => export_pdf(studio),
        "fit" => studio.fit.set(true),
        "zoom:1" => {
            studio.fit.set(false);
            studio.zoom.set(1.0);
        }
        "zoom:1.5" => {
            studio.fit.set(false);
            studio.zoom.set(1.5);
        }
        "theme" => toggle_theme(studio),
        "sidebar" => {
            studio.sidebar_open.update(|open| *open = !*open);
            persist(studio);
        }
        "ai" => {
            studio.ai_open.update(|open| *open = !*open);
            persist(studio);
        }
        _ => {}
    }
}

fn open_palette(studio: crate::state::Studio) {
    studio.palette_query.set(String::new());
    studio.palette_open.set(true);
}

fn begin_drag(event: PointerEvent, kind: DragKind, studio: crate::state::Studio, drag: DragCtx) {
    event.prevent_default();
    let start_w = match kind {
        DragKind::Sidebar => studio.sidebar_width.get_untracked(),
        DragKind::Ai => studio.ai_width.get_untracked(),
        DragKind::Split => 0,
    };
    drag.drag.set(Some(Drag {
        kind,
        start_x: event.client_x() as f64,
        start_w,
        span: 0.0,
    }));
}

pub fn default_sidebar_width() -> i32 {
    if web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|value| value.as_f64())
        .map(|width| width >= 2560.0)
        .unwrap_or(false)
    {
        240
    } else {
        216
    }
}

pub fn default_ai_width() -> i32 {
    if web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|value| value.as_f64())
        .map(|width| width >= 2560.0)
        .unwrap_or(false)
    {
        520
    } else {
        420
    }
}

pub fn apply_drag(studio: crate::state::Studio, drag: Option<Drag>, event: &PointerEvent) {
    let Some(drag) = drag else {
        return;
    };
    let delta = event.client_x() as f64 - drag.start_x;
    match drag.kind {
        DragKind::Sidebar => {
            studio
                .sidebar_width
                .set((drag.start_w as f64 + delta).clamp(180.0, 380.0) as i32);
        }
        DragKind::Ai => {
            studio
                .ai_width
                .set((drag.start_w as f64 - delta).clamp(320.0, 900.0) as i32);
        }
        DragKind::Split => {
            if drag.span > 0.0 {
                studio
                    .split
                    .set(((drag.start_w as f64 + delta) / drag.span).clamp(0.18, 0.82));
            }
        }
    }
}
