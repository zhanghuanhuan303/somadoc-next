//! Shared UI state and the two actions that talk to the engine/server:
//! `rebuild_now` (wasm) and `verify_now` (native).

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use soma_studio_core::{BuildRequest, NativeSummary, ProjectPayload, SourceFormat};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::probe::ProbeReport;
use crate::{api, engine, engine::SourceInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Source on the left, proof on the right. The default.
    Split,
    Source,
    Proof,
    Inspect,
    Verify,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Split,
        Mode::Source,
        Mode::Proof,
        Mode::Inspect,
        Mode::Verify,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Source => "source",
            Self::Proof => "proof",
            Self::Inspect => "inspect",
            Self::Verify => "verify",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Split => "分栏",
            Self::Source => "源文",
            Self::Proof => "校样",
            Self::Inspect => "检视",
            Self::Verify => "校验",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aux {
    None,
    Template,
    Engine,
    Capabilities,
    Fonts,
    About,
}

impl Aux {
    pub const fn title(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Template => "模板",
            Self::Engine => "引擎参数",
            Self::Capabilities => "能力矩阵",
            Self::Fonts => "字体",
            Self::About => "关于 Soma Studio",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub page: usize,
    pub block: usize,
    pub line: usize,
    pub node: i64,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            page: 0,
            block: usize::MAX,
            line: usize::MAX,
            node: -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocTab {
    pub id: u64,
    pub path: String,
    pub title: String,
    pub source: String,
    pub format: SourceFormat,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AiMessage {
    pub user: bool,
    pub text: String,
    pub badge: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub kind: &'static str,
}

#[derive(Clone, Copy)]
pub struct Studio {
    pub project: RwSignal<Option<ProjectPayload>>,
    pub tree: RwSignal<Vec<soma_studio_core::FileEntry>>,
    pub tabs: RwSignal<Vec<DocTab>>,
    pub active: RwSignal<usize>,
    pub source: RwSignal<String>,
    pub template: RwSignal<String>,
    pub engine_text: RwSignal<Option<String>>,
    pub docx_bytes: RwSignal<Option<Vec<u8>>>,
    pub metrics_bytes: RwSignal<Option<Vec<u8>>>,
    pub build: RwSignal<Option<engine::AppBuild>>,
    pub build_error: RwSignal<Option<String>>,
    pub building: RwSignal<bool>,
    pub build_seq: RwSignal<u64>,
    pub verify: RwSignal<Option<NativeSummary>>,
    pub verify_error: RwSignal<Option<String>>,
    pub verify_seq: RwSignal<u64>,
    pub mode: RwSignal<Mode>,
    pub page: RwSignal<usize>,
    pub zoom: RwSignal<f64>,
    pub fit: RwSignal<bool>,
    pub overlay: RwSignal<bool>,
    pub selection: RwSignal<Selection>,
    pub cursor_node: RwSignal<i64>,
    pub source_open: RwSignal<bool>,
    pub proof_open: RwSignal<bool>,
    pub theme: RwSignal<String>,
    pub sidebar_open: RwSignal<bool>,
    pub sidebar_width: RwSignal<i32>,
    pub ai_open: RwSignal<bool>,
    pub ai_width: RwSignal<i32>,
    pub ai_messages: RwSignal<Vec<AiMessage>>,
    pub ai_busy: RwSignal<bool>,
    pub palette_open: RwSignal<bool>,
    pub palette_query: RwSignal<String>,
    pub aux: RwSignal<Option<Aux>>,
    pub toast: RwSignal<Option<Toast>>,
    pub split: RwSignal<f64>,
    pub font_probe: RwSignal<Option<ProbeReport>>,
    pub probe_seq: RwSignal<u64>,
}

impl Default for Studio {
    fn default() -> Self {
        Self::new()
    }
}

impl Studio {
    pub fn new() -> Self {
        Self {
            project: RwSignal::new(None),
            tree: RwSignal::new(Vec::new()),
            tabs: RwSignal::new(Vec::new()),
            active: RwSignal::new(0),
            source: RwSignal::new(String::new()),
            template: RwSignal::new(String::new()),
            engine_text: RwSignal::new(None),
            docx_bytes: RwSignal::new(None),
            metrics_bytes: RwSignal::new(None),
            build: RwSignal::new(None),
            build_error: RwSignal::new(None),
            building: RwSignal::new(false),
            build_seq: RwSignal::new(0),
            verify: RwSignal::new(None),
            verify_error: RwSignal::new(None),
            verify_seq: RwSignal::new(0),
            mode: RwSignal::new(Mode::Split),
            page: RwSignal::new(0),
            zoom: RwSignal::new(1.0),
            fit: RwSignal::new(true),
            overlay: RwSignal::new(false),
            selection: RwSignal::new(Selection::default()),
            cursor_node: RwSignal::new(-1),
            source_open: RwSignal::new(true),
            proof_open: RwSignal::new(true),
            theme: RwSignal::new(String::from("dark")),
            sidebar_open: RwSignal::new(true),
            sidebar_width: RwSignal::new(216),
            ai_open: RwSignal::new(false),
            ai_width: RwSignal::new(420),
            ai_messages: RwSignal::new(Vec::new()),
            ai_busy: RwSignal::new(false),
            palette_open: RwSignal::new(false),
            palette_query: RwSignal::new(String::new()),
            aux: RwSignal::new(None),
            toast: RwSignal::new(None),
            split: RwSignal::new(0.46),
            font_probe: RwSignal::new(None),
            probe_seq: RwSignal::new(0),
        }
    }

    pub fn provide(self) {
        provide_context(self);
    }
}

pub fn use_studio() -> Studio {
    use_context::<Studio>().expect("Studio context")
}

/* ------------------------------------------------------------------ project */

pub fn apply_project(studio: Studio, project: ProjectPayload) {
    let source = project.source_text.clone().unwrap_or_default();
    let title = file_name(&project.input_relative);
    let tab = DocTab {
        id: 1,
        path: project.input_relative.clone(),
        title,
        source: source.clone(),
        format: project.format,
        dirty: false,
    };
    studio.tabs.set(vec![tab]);
    studio.active.set(0);
    studio.source.set(source);
    studio.template.set(project.template.clone());
    studio.engine_text.set(project.engine.clone());
    studio.docx_bytes.set(None);
    studio.metrics_bytes.set(None);
    studio.aux.set(None);
    studio.page.set(0);
    studio.selection.set(Selection::default());
    install_fonts(&project);
    studio.project.set(Some(project.clone()));
    spawn_local(async move {
        if project.custom_metrics {
            match api::metrics().await {
                Ok(bytes) => studio.metrics_bytes.set(Some(bytes)),
                Err(error) => studio.build_error.set(Some(error)),
            }
        }
        if project.format == SourceFormat::Docx {
            if let Ok(bytes) = api::source_bytes().await {
                studio.docx_bytes.set(Some(bytes));
                rebuild_now(studio);
            } else {
                studio
                    .build_error
                    .set(Some(String::from("无法读取 DOCX 源文件")));
            }
        } else {
            rebuild_now(studio);
        }
    });
}

pub fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Subscribe to the server's file watcher. A successful reload refreshes the
/// open tab only when it is clean; unsaved local edits always win. A failed
/// reload keeps the previous build and raises a diagnostic.
pub fn install_events(studio: Studio) {
    // The self-test runs headless with `--dump-dom`; a live SSE connection
    // keeps the page from reaching the idle state that command waits for.
    if web_sys::window()
        .and_then(|window| window.location().search().ok())
        .map(|search| search.contains("selftest") || search.contains("events=off"))
        .unwrap_or(false)
    {
        return;
    }
    let Ok(source) = web_sys::EventSource::new("/api/events") else {
        return;
    };
    let on_message =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                return;
            };
            match value.get("type").and_then(|kind| kind.as_str()) {
                Some("built") => reload_server_build(studio),
                Some("error") => {
                    let message = value
                        .get("message")
                        .and_then(|message| message.as_str())
                        .unwrap_or("服务端构建失败")
                        .to_string();
                    studio.build_error.set(Some(message.clone()));
                    toast(studio, message, "bad");
                }
                _ => {}
            }
        });
    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    // The connection lives for the page lifetime; dropping it would close it.
    std::mem::forget(source);
}

fn reload_server_build(studio: Studio) {
    spawn_local(async move {
        let Ok(project) = api::project().await else {
            return;
        };
        if let Ok(tree) = api::tree().await {
            studio.tree.set(tree.entries);
        }
        let active = studio.active.get_untracked();
        let current = studio.tabs.with_untracked(|tabs| tabs.get(active).cloned());
        match current {
            None => apply_project(studio, project),
            Some(tab) if tab.path == project.input_relative && !tab.dirty => {
                let source = project.source_text.clone().unwrap_or_default();
                studio.source.set(source.clone());
                studio.tabs.update(|tabs| {
                    if let Some(tab) = tabs.get_mut(active) {
                        tab.source = source;
                        tab.dirty = false;
                    }
                });
                studio.template.set(project.template.clone());
                studio.engine_text.set(project.engine.clone());
                install_fonts(&project);
                let docx = project.format == SourceFormat::Docx;
                if project.custom_metrics {
                    if let Ok(bytes) = api::metrics().await {
                        studio.metrics_bytes.set(Some(bytes));
                    }
                } else {
                    studio.metrics_bytes.set(None);
                }
                studio.project.set(Some(project));
                if docx {
                    if let Ok(bytes) = api::source_bytes().await {
                        studio.docx_bytes.set(Some(bytes));
                    }
                }
                rebuild_now(studio);
                toast(studio, "已重新载入服务端改动", "ok");
            }
            Some(_) => {
                studio.project.set(Some(project));
                toast(studio, "服务端文件已更新；保留本地草稿", "warn");
            }
        }
    });
}

pub fn set_mode(studio: Studio, mode: Mode) {
    studio.mode.set(mode);
    studio.aux.set(None);
    match mode {
        Mode::Split => {
            studio.source_open.set(true);
            studio.proof_open.set(true);
        }
        Mode::Source => studio.source_open.set(true),
        Mode::Proof => studio.proof_open.set(true),
        Mode::Inspect => {
            studio.proof_open.set(true);
            studio.overlay.set(true);
        }
        Mode::Verify => {}
    }
}

/// Set the page and scroll the proof canvas to it. The page controls live in
/// the breadcrumb, so this is shared by the shell and the inspect table.
pub fn go_to_page(studio: Studio, page: usize) {
    let count = studio.page_count();
    if count == 0 {
        return;
    }
    let page = page.min(count - 1);
    studio.page.set(page);
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Ok(Some(element)) =
        document.query_selector(&format!(".page-slot[data-page='{}']", page + 1))
    else {
        return;
    };
    let options = web_sys::ScrollIntoViewOptions::new();
    options.set_behavior(web_sys::ScrollBehavior::Auto);
    options.set_block(web_sys::ScrollLogicalPosition::Start);
    element.scroll_into_view_with_scroll_into_view_options(&options);
}

/* ------------------------------------------------------------------ rebuild */

/// The frontend of the tab the user is looking at. A tab carries its own
/// format: opening `notes.md` inside a LaTeX project must parse Markdown, not
/// reuse the project input's format. Falls back to the project input when no
/// tab is active yet.
fn active_format(studio: Studio) -> Option<SourceFormat> {
    let active = studio.active.get_untracked();
    studio
        .tabs
        .with_untracked(|tabs| tabs.get(active).map(|tab| tab.format))
        .or_else(|| {
            studio
                .project
                .with_untracked(|project| project.as_ref().map(|project| project.format))
        })
}

pub fn rebuild_now(studio: Studio) {
    if studio.tabs.with_untracked(|tabs| tabs.is_empty()) {
        studio.build_seq.update(|seq| *seq += 1);
        studio.verify_seq.update(|seq| *seq += 1);
        studio.building.set(false);
        studio.build.set(None);
        studio.build_error.set(None);
        return;
    }
    let seq = studio.build_seq.get_untracked() + 1;
    studio.build_seq.set(seq);
    let Some(format) = active_format(studio) else {
        return;
    };
    let source = studio.source.get_untracked();
    let template = studio.template.get_untracked();
    let engine = studio.engine_text.get_untracked();
    let metrics = studio.metrics_bytes.get_untracked();
    studio.building.set(true);
    studio.build_error.set(None);

    spawn_local(async move {
        if studio.build_seq.get_untracked() != seq {
            return;
        }
        let result = if format == SourceFormat::Docx {
            let Some(bytes) = studio.docx_bytes.get_untracked() else {
                studio.building.set(false);
                return;
            };
            engine::build(
                format,
                SourceInput::Bytes(&bytes),
                &template,
                engine.as_deref(),
                metrics.as_deref(),
            )
            .await
        } else {
            engine::build(
                format,
                SourceInput::Text(&source),
                &template,
                engine.as_deref(),
                metrics.as_deref(),
            )
            .await
        };
        if studio.build_seq.get_untracked() != seq {
            return;
        }
        studio.building.set(false);
        match result {
            Ok(build) => {
                studio.build.set(Some(build));
                studio.build_error.set(None);
                let pages = studio.build.with(|build| {
                    build
                        .as_ref()
                        .map(|build| build.build.pages.page_count())
                        .unwrap_or(0)
                });
                if studio.page.get_untracked() >= pages as usize {
                    studio.page.set(0);
                }
                verify_now(studio);
            }
            Err(error) => {
                studio.build_error.set(Some(error));
            }
        }
    });
}

/* ------------------------------------------------------------------- verify */

pub fn verify_now(studio: Studio) {
    let Some(format) = active_format(studio) else {
        return;
    };
    let seq = studio.verify_seq.get_untracked() + 1;
    studio.verify_seq.set(seq);
    let request = BuildRequest {
        source: if format == SourceFormat::Docx {
            None
        } else {
            Some(studio.source.get_untracked())
        },
        template: Some(studio.template.get_untracked()),
        engine: studio.engine_text.get_untracked(),
        format: Some(format),
    };
    spawn_local(async move {
        let result = api::verify(&request).await;
        if studio.verify_seq.get_untracked() != seq {
            return;
        }
        match result {
            Ok(summary) => {
                studio.verify.set(Some(summary));
                studio.verify_error.set(None);
            }
            Err(error) => {
                studio.verify.set(None);
                studio.verify_error.set(Some(error));
            }
        }
    });
}

/* --------------------------------------------------------------------- files */

pub fn open_file(studio: Studio, path: String) {
    if let Some(index) = studio
        .tabs
        .with(|tabs| tabs.iter().position(|tab| tab.path == path))
    {
        activate_tab(studio, index);
        return;
    }
    let Some(format) = SourceFormat::from_path(&path) else {
        toast(studio, "不支持的文件类型", "bad");
        return;
    };
    let name = file_name(&path);
    spawn_local(async move {
        if format == SourceFormat::Docx {
            let Ok(bytes) = api::asset(&path).await else {
                toast(studio, "无法读取 DOCX", "bad");
                return;
            };
            studio.docx_bytes.set(Some(bytes));
            let index = studio.tabs.with_untracked(|tabs| tabs.len());
            studio.tabs.update(|tabs| {
                tabs.push(DocTab {
                    id: (tabs.len() as u64) + 1,
                    path: path.clone(),
                    title: name,
                    source: String::new(),
                    format,
                    dirty: false,
                });
            });
            studio.active.set(index);
            studio.source.set(String::new());
            rebuild_now(studio);
            return;
        }
        let Ok(text) = api::file(&path).await else {
            toast(studio, "无法读取文件", "bad");
            return;
        };
        let index = studio.tabs.with_untracked(|tabs| tabs.len());
        studio.tabs.update(|tabs| {
            tabs.push(DocTab {
                id: (tabs.len() as u64) + 1,
                path: path.clone(),
                title: name,
                source: text.clone(),
                format,
                dirty: false,
            });
        });
        studio.active.set(index);
        studio.source.set(text);
        studio.docx_bytes.set(None);
        rebuild_now(studio);
    });
}

pub fn activate_tab(studio: Studio, index: usize) {
    let current = studio.active.get_untracked();
    if current == index {
        return;
    }
    let source = studio.source.get_untracked();
    studio.tabs.update(|tabs| {
        if let Some(tab) = tabs.get_mut(current) {
            tab.source = source;
        }
    });
    let next = studio.tabs.with_untracked(|tabs| tabs.get(index).cloned());
    if let Some(tab) = next {
        studio.active.set(index);
        studio.source.set(tab.source);
        studio.page.set(0);
        studio.selection.set(Selection::default());
        if tab.format == SourceFormat::Docx {
            let path = tab.path.clone();
            spawn_local(async move {
                if let Ok(bytes) = api::asset(&path).await {
                    studio.docx_bytes.set(Some(bytes));
                    rebuild_now(studio);
                }
            });
        } else {
            rebuild_now(studio);
        }
    }
}

pub fn close_tab(studio: Studio, index: usize) {
    let len = studio.tabs.with_untracked(|tabs| tabs.len());
    if index >= len {
        return;
    }
    studio.tabs.update(|tabs| {
        tabs.remove(index);
    });
    let remaining = len - 1;
    if remaining == 0 {
        // Invalidate builds that were already in flight for the closed tab.
        studio.build_seq.update(|seq| *seq += 1);
        studio.verify_seq.update(|seq| *seq += 1);
        studio.building.set(false);
        studio.active.set(0);
        studio.source.set(String::new());
        studio.build.set(None);
        studio.build_error.set(None);
        studio.verify.set(None);
        studio.page.set(0);
        studio.selection.set(Selection::default());
        studio.aux.set(None);
        return;
    }
    let active = studio.active.get_untracked();
    let next = if index < active {
        active - 1
    } else if index == active {
        active.min(remaining - 1)
    } else {
        active
    };
    studio.active.set(next);
    if let Some(tab) = studio.tabs.with_untracked(|tabs| tabs.get(next).cloned()) {
        studio.source.set(tab.source);
    }
    rebuild_now(studio);
}

pub fn save_active(studio: Studio) {
    let index = studio.active.get_untracked();
    let tab = studio.tabs.with_untracked(|tabs| tabs.get(index).cloned());
    let Some(tab) = tab else {
        return;
    };
    let text = studio.source.get_untracked();
    let path = tab.path.clone();
    spawn_local(async move {
        match api::save(&path, &text).await {
            Ok(()) => {
                studio.tabs.update(|tabs| {
                    if let Some(tab) = tabs.get_mut(index) {
                        tab.source = text;
                        tab.dirty = false;
                    }
                });
                toast(studio, "已保存", "ok");
                // The project input may have changed on disk; refresh the
                // native summary without replacing the in-browser source.
                if let Ok(project) = api::rebuild().await {
                    studio.project.set(Some(project));
                }
                verify_now(studio);
            }
            Err(error) => toast(studio, error, "bad"),
        }
    });
}

pub fn export_pdf(studio: Studio) {
    let Some(project) = studio.project.get_untracked() else {
        return;
    };
    let Some(format) = active_format(studio) else {
        return;
    };
    if studio.build.with_untracked(|build| build.is_none()) {
        toast(studio, "还没有可导出的构建", "bad");
        return;
    }
    if !project.pdf {
        toast(studio, "服务端没有配置 PDF 字体", "bad");
        return;
    }
    let request = BuildRequest {
        source: if format == SourceFormat::Docx {
            None
        } else {
            Some(studio.source.get_untracked())
        },
        template: Some(studio.template.get_untracked()),
        engine: studio.engine_text.get_untracked(),
        format: Some(format),
    };
    spawn_local(async move {
        match api::pdf(&request).await {
            Ok(bytes) if bytes.is_empty() => {
                toast(studio, "服务端返回了空 PDF", "bad");
            }
            Ok(bytes) => {
                let pages = studio.summary().map(|summary| summary.pages).unwrap_or(0);
                let kb = bytes.len().div_ceil(1024);
                match api::download("soma-studio.pdf", &bytes) {
                    Ok(()) => toast(studio, format!("PDF 已导出 · {pages} 页 · {kb} KB"), "ok"),
                    Err(error) => toast(studio, error, "bad"),
                }
            }
            Err(error) => toast(studio, error, "bad"),
        }
    });
}

/* --------------------------------------------------------------------- theme */

pub fn init_preferences(studio: Studio) {
    let Some(storage) = storage() else {
        return;
    };
    let theme = storage
        .get_item("soma.theme")
        .ok()
        .flatten()
        .filter(|value| value == "light" || value == "dark")
        .unwrap_or_else(|| {
            if web_sys::window()
                .and_then(|window| {
                    window
                        .match_media("(prefers-color-scheme: light)")
                        .ok()
                        .flatten()
                })
                .map(|query| query.matches())
                .unwrap_or(false)
            {
                String::from("light")
            } else {
                String::from("dark")
            }
        });
    set_theme(studio, &theme);
    if let Some(width) = storage
        .get_item("soma.sidebar_width")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<i32>().ok())
    {
        studio.sidebar_width.set(width.clamp(180, 380));
    }
    if let Some(width) = storage
        .get_item("soma.ai_width")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<i32>().ok())
    {
        studio.ai_width.set(width.clamp(320, 900));
    }
    if let Some(open) = storage.get_item("soma.sidebar_open").ok().flatten() {
        studio.sidebar_open.set(open != "0");
    }
    if let Some(open) = storage.get_item("soma.ai_open").ok().flatten() {
        studio.ai_open.set(open == "1");
    }
    if let Some(open) = storage.get_item("soma.source_open").ok().flatten() {
        studio.source_open.set(open != "0");
    }
    if let Some(open) = storage.get_item("soma.proof_open").ok().flatten() {
        studio.proof_open.set(open != "0");
    }
    if let Some(split) = storage
        .get_item("soma.split")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<f64>().ok())
    {
        studio.split.set(split.clamp(0.18, 0.82));
    }
}

pub fn set_theme(studio: Studio, theme: &str) {
    studio.theme.set(theme.to_string());
    if let Some(document) = web_sys::window().and_then(|window| window.document()) {
        if let Some(root) = document.document_element() {
            let _ = root.set_attribute("data-theme", theme);
        }
    }
    if let Some(storage) = storage() {
        let _ = storage.set_item("soma.theme", theme);
    }
}

pub fn toggle_theme(studio: Studio) {
    let next = if studio.theme.get_untracked() == "dark" {
        "light"
    } else {
        "dark"
    };
    set_theme(studio, next);
}

pub fn persist(studio: Studio) {
    let Some(storage) = storage() else {
        return;
    };
    let _ = storage.set_item(
        "soma.sidebar_width",
        &studio.sidebar_width.get_untracked().to_string(),
    );
    let _ = storage.set_item(
        "soma.ai_width",
        &studio.ai_width.get_untracked().to_string(),
    );
    let _ = storage.set_item(
        "soma.sidebar_open",
        if studio.sidebar_open.get_untracked() {
            "1"
        } else {
            "0"
        },
    );
    let _ = storage.set_item(
        "soma.ai_open",
        if studio.ai_open.get_untracked() {
            "1"
        } else {
            "0"
        },
    );
    let _ = storage.set_item(
        "soma.source_open",
        if studio.source_open.get_untracked() {
            "1"
        } else {
            "0"
        },
    );
    let _ = storage.set_item(
        "soma.proof_open",
        if studio.proof_open.get_untracked() {
            "1"
        } else {
            "0"
        },
    );
    let _ = storage.set_item(
        "soma.split",
        &format!("{:.4}", studio.split.get_untracked()),
    );
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/* --------------------------------------------------------------------- toast */

pub fn toast(studio: Studio, text: impl Into<String>, kind: &'static str) {
    let id = studio
        .toast
        .get_untracked()
        .map(|toast| toast.id + 1)
        .unwrap_or(1);
    studio.toast.set(Some(Toast {
        id,
        text: text.into(),
        kind,
    }));
    spawn_local(async move {
        TimeoutFuture::new(4200).await;
        if studio.toast.get_untracked().map(|toast| toast.id) == Some(id) {
            studio.toast.set(None);
        }
    });
}

/* --------------------------------------------------------------------- fonts */

fn install_fonts(project: &ProjectPayload) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };
    if let Some(old) = document.get_element_by_id("soma-fonts") {
        old.remove();
    }
    let style = document
        .create_element("style")
        .ok()
        .and_then(|element| element.dyn_into::<web_sys::HtmlStyleElement>().ok());
    let Some(style) = style else {
        return;
    };
    style.set_id("soma-fonts");
    let mut css = String::new();
    let mut roles: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    for font in &project.fonts {
        // System faces discovered by the server have no URL; reference them by
        // name so the proof uses the same family as the PDF.
        roles.insert(font.role.as_str(), font.family.clone());
        if font.url.is_empty() {
            continue;
        }
        let format = if font.role == "math" || font.url.ends_with(".otf") {
            "opentype"
        } else if font.url.ends_with(".ttf") {
            "truetype"
        } else if font.url.ends_with(".woff2") {
            "woff2"
        } else {
            "woff"
        };
        css.push_str(&format!(
            "@font-face{{font-family:\"{}\";src:url(\"{}\") format(\"{}\");font-weight:{};font-style:{};font-display:swap;}}",
            css_string(&font.family),
            font.url,
            format,
            font.weight,
            font.style
        ));
    }
    if !roles.is_empty() {
        // `tokens.css` defines the fallback families on `html[data-theme=…]`,
        // whose specificity beats a plain `:root`. Match that specificity and
        // come later in the cascade so the installed faces win.
        css.push_str("html[data-theme],:root{");
        for (role, family) in &roles {
            let fallback = match *role {
                "mono" => "monospace",
                "sans" => "sans-serif",
                _ => "serif",
            };
            css.push_str(&format!(
                "--soma-{}:\"{}\", {};",
                role,
                css_string(family),
                fallback
            ));
        }
        css.push('}');
    }
    style.set_text_content(Some(&css));
    let _ = head.append_child(&style);
}

fn css_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/* ------------------------------------------------------------------- derive */

impl Studio {
    pub fn summary(&self) -> Option<NativeSummary> {
        self.build.with(|build| {
            build
                .as_ref()
                .map(|build| build.build.summary(build.elapsed_ms))
        })
    }

    pub fn page_count(&self) -> usize {
        self.build.with(|build| {
            build
                .as_ref()
                .map(|build| build.build.pages.page_count() as usize)
                .unwrap_or(0)
        })
    }

    pub fn current_selection(&self) -> Option<Selection> {
        let selection = self.selection.get_untracked();
        if selection.block < usize::MAX {
            Some(selection)
        } else {
            None
        }
    }
}
