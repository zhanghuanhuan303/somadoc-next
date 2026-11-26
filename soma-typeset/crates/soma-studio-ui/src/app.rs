//! Application shell and workspace composition.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{KeyboardEvent, PointerEvent};

use crate::ai::AiDock;
use crate::api;
use crate::inspect::InspectPane;
use crate::proof::ProofPane;
use crate::shell::{self, Drag, DragCtx, DragKind, Palette, Sidebar, Statusbar, Tabs, Titlebar};
use crate::source::SourcePane;
use crate::state::{self, persist, use_studio, Aux, Mode, Studio};
use crate::verify::VerifyPane;

#[component]
pub fn App() -> impl IntoView {
    let studio = Studio::new();
    studio.provide();
    let drag = DragCtx {
        drag: RwSignal::new(None),
    };
    provide_context(drag);
    state::init_preferences(studio);
    install_global(studio, drag);
    crate::selftest::install(studio);

    let loaded = StoredValue::new(false);
    Effect::new(move |_| {
        if loaded.get_value() {
            return;
        }
        loaded.set_value(true);
        spawn_local(async move {
            if studio.local {
                state::apply_local_project(studio);
                apply_url_params(studio);
                return;
            }
            match api::project().await {
                Ok(project) => {
                    state::apply_project(studio, project);
                    if let Ok(tree) = api::tree().await {
                        studio.tree.set(tree.entries);
                    }
                    apply_url_params(studio);
                }
                Err(error) => studio.build_error.set(Some(error)),
            }
        });
    });
    // The server's watcher reloads a clean tab and keeps unsaved drafts.
    state::install_events(studio);
    // The font probe is a diagnostic; it never feeds layout.
    Effect::new(move |_| {
        if studio.build.with(|build| build.is_none()) {
            return;
        }
        let seq = studio.probe_seq.get_untracked() + 1;
        studio.probe_seq.set(seq);
        spawn_local(async move {
            if let Some(report) = crate::probe::measure(studio).await {
                if studio.probe_seq.get_untracked() == seq {
                    studio.font_probe.set(Some(report));
                }
            }
        });
    });

    view! {
        <div class="app">
            <Titlebar/>
            <div class="body">
                <Sidebar/>
                <main class="main">
                    <Tabs/>
                    <div class="content">
                        <Loading/>
                        <EmptyWorkspace/>
                        <EmptyPanes/>
                        <Workspace/>
                        <VerifyPane/>
                        <AuxPage/>
                    </div>
                </main>
                <AiDock/>
            </div>
            <Statusbar/>
        </div>
        <Palette/>
        <Toasts/>
    }
}

#[component]
fn Loading() -> impl IntoView {
    let studio = use_studio();
    view! {
        <div class="empty-state" hidden=move || { studio.project.get().is_some() }>
            <div class="mark">"S"</div>
            {move || {
                if let Some(error) = studio.build_error.get() {
                    view! {
                        <strong>"无法加载项目"</strong>
                        <span class="mono" style="max-width:720px;white-space:pre-wrap">{error}</span>
                    }.into_any()
                } else {
                    view! {
                        <strong>"正在加载"</strong>
                        <span>"读取项目、模板与引擎参数；排版会在浏览器里完成。"</span>
                    }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn EmptyWorkspace() -> impl IntoView {
    let studio = use_studio();
    let visible = move || {
        studio.project.get().is_some()
            && studio.tabs.with(|tabs| tabs.is_empty())
            && studio.aux.get().is_none()
            && studio.mode.get() != Mode::Verify
    };
    view! {
        <div class="empty-state" id="empty-workspace" hidden=move || { !visible() }>
            <div class="mark">"S"</div>
            <strong>"没有打开的文档"</strong>
            <span>"从左侧稿件栏选择文件，或按 ⌘K 打开命令面板。"</span>
            <button class="primary" on:click=move |_| {
                studio.palette_query.set(String::new());
                studio.palette_open.set(true);
            }>"打开文件"</button>
        </div>
    }
}

#[component]
fn EmptyPanes() -> impl IntoView {
    let studio = use_studio();
    let source_visible =
        move || studio.source_open.get() && matches!(studio.mode.get(), Mode::Split | Mode::Source);
    let proof_visible = move || {
        studio.proof_open.get()
            && matches!(studio.mode.get(), Mode::Split | Mode::Proof)
            && studio.aux.get().is_none()
    };
    let visible = move || {
        studio.project.get().is_some()
            && studio.tabs.with(|tabs| !tabs.is_empty())
            && studio.aux.get().is_none()
            && studio.mode.get() != Mode::Verify
            && studio.mode.get() != Mode::Inspect
            && !source_visible()
            && !proof_visible()
    };
    view! {
        <div class="empty-state" id="empty-panes" hidden=move || { !visible() }>
            <div class="mark">"S"</div>
            <strong>"面板都已关闭"</strong>
            <span>"重新打开源文或校样；也可以用左侧「视图」或 ⌘K。"</span>
            <div class="empty-actions">
                <button class="primary" on:click=move |_| state::set_mode(studio, Mode::Split)>"并排显示"</button>
                <button class="ghost" on:click=move |_| state::set_mode(studio, Mode::Source)>"只要源文"</button>
                <button class="ghost" on:click=move |_| state::set_mode(studio, Mode::Proof)>"只要校样"</button>
            </div>
        </div>
    }
}

#[component]
fn Workspace() -> impl IntoView {
    let studio = use_studio();
    let drag = crate::shell::use_drag();
    let source_visible =
        move || studio.source_open.get() && matches!(studio.mode.get(), Mode::Split | Mode::Source);
    let proof_visible = move || {
        studio.proof_open.get()
            && matches!(studio.mode.get(), Mode::Split | Mode::Proof | Mode::Inspect)
            && studio.aux.get().is_none()
    };
    let has_tab = move || studio.tabs.with(|tabs| !tabs.is_empty());
    let panes_empty = move || {
        studio.mode.get() != Mode::Verify
            && studio.mode.get() != Mode::Inspect
            && !source_visible()
            && !proof_visible()
    };
    let splitter_visible = move || source_visible() && proof_visible();
    let split_start = move |event: PointerEvent| {
        event.prevent_default();
        let Some(target) = event
            .current_target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let Some(parent) = target.parent_element() else {
            return;
        };
        let Some(source) = parent.query_selector(".source-pane").ok().flatten() else {
            return;
        };
        drag.drag.set(Some(Drag {
            kind: DragKind::Split,
            start_x: event.client_x() as f64,
            start_w: source.get_bounding_client_rect().width() as i32,
            span: parent.get_bounding_client_rect().width(),
        }));
    };
    view! {
        <div class="workspace"
            hidden=move || {
                studio.project.get().is_none()
                    || !has_tab()
                    || studio.aux.get().is_some()
                    || studio.mode.get() == Mode::Verify
                    || panes_empty()
            }>
            <SourcePane/>
            <div class="split-resizer" id="split-resizer"
                hidden=move || !splitter_visible()
                on:pointerdown=split_start
                on:dblclick=move |_| { studio.split.set(0.46); persist(studio); }></div>
            <section class="pane" id="proof-pane" hidden=move || !proof_visible()>
                <div class="pane-head">
                    <span>"校样"</span>
                    <div class="proof-controls" hidden=move || studio.build.with(|build| build.is_none())>
                        <button class="page-btn" title="上一页" aria-label="上一页"
                            on:click=move |_| { let page = studio.page.get().saturating_sub(1); state::go_to_page(studio, page); }>"‹"</button>
                        <input class="page-input" inputmode="numeric" aria-label="页码" title="页码"
                            prop:value=move || { (studio.page.get() + 1).to_string() }
                            on:change=move |event| {
                                if let Ok(value) = event_target_value(&event).parse::<usize>() {
                                    state::go_to_page(studio, value.saturating_sub(1));
                                }
                            }/>
                        <span class="faint">{move || format!("/ {}", studio.page_count())}</span>
                        <button class="page-btn" title="下一页" aria-label="下一页"
                            on:click=move |_| { let page = (studio.page.get() + 1).min(studio.page_count().saturating_sub(1)); state::go_to_page(studio, page); }>"›"</button>
                        <select class="zoom-select" aria-label="缩放" title="缩放"
                            prop:value=move || if studio.fit.get() { String::from("fit") } else { format!("{}", studio.zoom.get()) }
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
                    <div class="fill"></div>
                    <span class="badge">
                        {move || studio.build.with(|build| {
                            build.as_ref().map(|build| {
                                format!("{} 页 · {} ms", build.build.pages.page_count(), build.elapsed_ms)
                            }).unwrap_or_else(|| String::from("—"))
                        })}
                    </span>
                    <button class="icon-btn small" title="关闭校样面板" aria-label="关闭校样面板"
                        on:click=move |_| { studio.proof_open.set(false); persist(studio); }>"×"</button>
                </div>
                <div class="pane-body" style="overflow:hidden">
                    <ProofPane/>
                </div>
            </section>
            <InspectPane/>
        </div>
    }
}

#[component]
fn AuxPage() -> impl IntoView {
    let studio = use_studio();
    let title = move || studio.aux.get().map(Aux::title).unwrap_or("").to_string();
    view! {
        <section class="doc-page" hidden=move || { studio.aux.get().is_none() }>
            <h1>{title}</h1>
            <p class="lead">
                <button class="ghost" style="height:26px;margin-right:8px"
                    on:click=move |_| studio.aux.set(None)>"← 返回"</button>
                {move || match studio.aux.get() {
                    Some(Aux::Template) => "模板是纯 YAML；这里只读展示当前生效的文件。",
                    Some(Aux::Engine) => "引擎配置是参数层，不属于模板。改动会换 paramHash。",
                    Some(Aux::Capabilities) => "前端对每种源文构造的声明；与 docs/capabilities.md 同源。",
                    Some(Aux::Fonts) => "服务端为这次会话提供的字体；浏览器用同一族名注入 @font-face。",
                    Some(Aux::About) => "全 Rust wasm 前端 + Axum 后端。",
                    _ => "",
                }}
            </p>
            {move || match studio.aux.get() {
                Some(Aux::Template) => view! {
                    <div class="panel">
                        <div class="panel-head">
                            <div>
                                <h2>"模板 YAML"</h2>
                                <p>{move || studio.project.with(|project| project.as_ref().and_then(|project| project.template_path.clone()).unwrap_or_else(|| String::from("built-in plex"))) }</p>
                            </div>
                        </div>
                        <pre class="mono" style="margin:0;border:0;border-radius:0;max-height:64vh;white-space:pre-wrap">{move || studio.template.get()}</pre>
                    </div>
                }.into_any(),
                Some(Aux::Engine) => view! {
                    <div class="panel">
                        <div class="panel-head">
                            <div>
                                <h2>"引擎配置"</h2>
                                <p>"只读；改这里需要换 preset / 新 ADR。"</p>
                            </div>
                        </div>
                        <pre class="mono" style="margin:0;border:0;border-radius:0;max-height:64vh;white-space:pre-wrap">{move || studio.engine_text.get().unwrap_or_else(|| String::from("（内置 proposal-v1）"))}</pre>
                    </div>
                }.into_any(),
                Some(Aux::Capabilities) => view! {
                    <div class="panel">
                        <div class="panel-head"><h2>"能力矩阵"</h2></div>
                        <div class="table-wrap">
                            <table class="compact">
                                <thead><tr><th>"构造"</th><th>"状态"</th><th>"说明"</th></tr></thead>
                                <tbody>
                                    {move || studio.project.with(|project| {
                                        project.as_ref().map(|project| {
                                            project.capabilities.iter().map(|capability| {
                                                let class = match capability.status.as_str() {
                                                    "supported" => "pill ok",
                                                    "degraded" => "pill warn",
                                                    _ => "pill bad",
                                                };
                                                view! {
                                                    <tr>
                                                        <td class="mono">{capability.syntax.clone()}</td>
                                                        <td><span class=class>{capability.status.clone()}</span></td>
                                                        <td>{capability.note.clone()}</td>
                                                    </tr>
                                                }
                                            }).collect_view()
                                        })
                                    })}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }.into_any(),
                Some(Aux::Fonts) => view! {
                    <div class="panel">
                        <div class="panel-head"><h2>"字体"</h2></div>
                        <div class="table-wrap">
                            <table class="compact">
                                <thead><tr><th>"角色"</th><th>"族名"</th><th>"来源"</th></tr></thead>
                                <tbody>
                                    {move || studio.project.with(|project| {
                                        project.as_ref().map(|project| {
                                            if project.fonts.is_empty() {
                                                return view! { <tr><td colspan="3" class="muted">"没有通过 CLI 配置字体；浏览器回退到系统族。"</td></tr> }.into_any();
                                            }
                                            project.fonts.iter().map(|font| view! {
                                                <tr>
                                                    <td class="mono">{font.role.clone()}</td>
                                                    <td>{font.family.clone()}</td>
                                                    <td class="mono">{font.url.clone()}</td>
                                                </tr>
                                            }).collect_view().into_any()
                                        })
                                    })}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }.into_any(),
                Some(Aux::About) => view! {
                    <div class="panel">
                        <div class="panel-body">
                            <p>"Soma Studio 把整个排版引擎编进 wasm：源文解析、模板解析、断行、分页、HTML 页面都在浏览器里完成。Axum 只负责文件、图片/字体、PDF 与 native 校验。"</p>
                            <p>"canonical 哈希在两边必须相等；不一致是红状态。"</p>
                            <div class="kv-grid">
                                <div class="kv"><div class="k">"前端"</div><div class="v">"Leptos CSR → wasm"</div></div>
                                <div class="kv"><div class="k">"后端"</div><div class="v">"Axum"</div></div>
                                <div class="kv"><div class="k">"引擎"</div><div class="v">{move || studio.project.with(|project| project.as_ref().map(|project| format!("soma {}", project.engine_version)).unwrap_or_default())}</div></div>
                                <div class="kv"><div class="k">"快捷键"</div><div class="v">"⌘K / ⌘J / ⌘↵ / ⌘S"</div></div>
                            </div>
                        </div>
                    </div>
                }.into_any(),
                _ => view! { <div></div> }.into_any(),
            }}
        </section>
    }
}

#[component]
fn Toasts() -> impl IntoView {
    let studio = use_studio();
    view! {
        <div class="toasts" aria-live="polite">
            {move || studio.toast.get().map(|toast| view! {
                <div class=format!("toast {}", toast.kind)>{toast.text}</div>
            })}
        </div>
    }
}

fn apply_url_params(studio: Studio) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(location) = window.location().search() else {
        return;
    };
    let search = location;
    let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) else {
        return;
    };
    if let Some(theme) = params.get("theme") {
        if theme == "dark" || theme == "light" {
            state::set_theme(studio, &theme);
        }
    }
    if let Some(mode) = params.get("mode") {
        match mode.as_str() {
            "split" => state::set_mode(studio, Mode::Split),
            "source" => state::set_mode(studio, Mode::Source),
            "proof" => state::set_mode(studio, Mode::Proof),
            "inspect" => state::set_mode(studio, Mode::Inspect),
            "verify" => state::set_mode(studio, Mode::Verify),
            _ => {}
        }
    }
    if let Some(zoom) = params
        .get("zoom")
        .and_then(|value| value.parse::<f64>().ok())
    {
        studio.fit.set(false);
        studio.zoom.set(zoom.clamp(0.25, 3.0));
    }
    if let Some(page) = params
        .get("page")
        .and_then(|value| value.parse::<usize>().ok())
    {
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(250).await;
            state::go_to_page(studio, page.saturating_sub(1));
        });
    }
}

/* ------------------------------------------------------------------- global */

fn install_global(studio: Studio, drag: DragCtx) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let key = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        let meta = event.meta_key() || event.ctrl_key();
        let key = event.key();
        if meta && key.eq_ignore_ascii_case("k") {
            event.prevent_default();
            studio.palette_query.set(String::new());
            studio.palette_open.update(|open| *open = !*open);
            return;
        }
        if meta && key.eq_ignore_ascii_case("j") {
            event.prevent_default();
            studio.ai_open.update(|open| *open = !*open);
            persist(studio);
            return;
        }
        if meta && key == "Enter" {
            event.prevent_default();
            state::rebuild_now(studio);
            return;
        }
        if meta && key.eq_ignore_ascii_case("s") {
            event.prevent_default();
            state::save_active(studio);
            return;
        }
        if meta && key.eq_ignore_ascii_case("i") {
            event.prevent_default();
            studio.overlay.update(|value| *value = !*value);
            return;
        }
        if meta {
            match key.as_str() {
                "1" => {
                    event.prevent_default();
                    state::set_mode(studio, Mode::Source);
                }
                "2" => {
                    event.prevent_default();
                    state::set_mode(studio, Mode::Proof);
                }
                "3" => {
                    event.prevent_default();
                    state::set_mode(studio, Mode::Inspect);
                }
                "4" => {
                    event.prevent_default();
                    state::set_mode(studio, Mode::Verify);
                }
                _ => {}
            }
        }
        if key == "Escape" {
            studio.palette_open.set(false);
        }
    });
    let _ = window.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref());
    key.forget();

    let pointer = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
        let current = drag.drag.get_untracked();
        if current.is_some() {
            shell::apply_drag(studio, current, &event);
        }
    });
    let _ =
        window.add_event_listener_with_callback("pointermove", pointer.as_ref().unchecked_ref());
    pointer.forget();

    let up = Closure::<dyn FnMut(PointerEvent)>::new(move |_event: PointerEvent| {
        if drag.drag.get_untracked().is_some() {
            drag.drag.set(None);
            persist(studio);
        }
    });
    let _ = window.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
    up.forget();
}
