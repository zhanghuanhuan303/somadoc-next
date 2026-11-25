//! Browser self-test for the Studio shell (gates S1–S11).
//!
//! It runs only when the URL carries `?selftest=1`. Every assertion writes a
//! line into `<pre id="selftest">` (`S1|ok|…` / `S1|fail|…`), so a headless
//! Chromium run can read the result with `--dump-dom`. The module never touches
//! the engine; it reads the live DOM and the exported layout summaries.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use soma_studio_core::SourceFormat;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, Event, EventInit, HtmlElement, PointerEvent, PointerEventInit};

use crate::state::{self, Mode, Studio};

pub fn install(studio: Studio) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(search) = window.location().search() else {
        return;
    };
    let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) else {
        return;
    };
    if !params.has("selftest") {
        return;
    }
    let only = params.get("only");
    spawn_local(async move {
        run(studio, only).await;
    });
}

async fn run(studio: Studio, only: Option<String>) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let mut lines: Vec<String> = Vec::new();
    let only = only.unwrap_or_default();

    if only.is_empty() {
        if !wait_for(|| studio.build.with(|build| build.is_some()), 120).await {
            lines.push(result("S0", false, "构建未完成"));
            emit(&document, &lines);
            return;
        }
        let _ = wait_for(|| studio.verify.with(|verify| verify.is_some()), 80).await;
        if !studio.ai_open.get_untracked() {
            click_selector(&document, "[aria-label='排版助手']");
            TimeoutFuture::new(120).await;
        }
        lines.push(s1(&document));
        emit(&document, &lines);
        lines.push(s3(&document, studio).await);
        emit(&document, &lines);
        lines.push(s4(&document, studio).await);
        emit(&document, &lines);
        lines.push(s5(&document, studio).await);
        emit(&document, &lines);
        lines.push(s6(&document, studio).await);
        emit(&document, &lines);
        lines.push(s7(&document, studio).await);
        emit(&document, &lines);
        lines.push(s8(&document));
        emit(&document, &lines);
        lines.push(s9(&document, studio).await);
        emit(&document, &lines);
        lines.push(s10(&document, studio).await);
        emit(&document, &lines);
        lines.push(s2(&document, studio).await);
        emit(&document, &lines);
        lines.push(s11(&document, studio).await);
    } else if only == "s9font" {
        if !wait_for(|| studio.build.with(|build| build.is_some()), 160).await {
            lines.push(result("S0", false, "构建未完成"));
            emit(&document, &lines);
            return;
        }
        lines.push(s9(&document, studio).await);
    }

    emit(&document, &lines);
    document.set_title("SOMA-SELFTEST-DONE");
    if let Some(root) = document.document_element() {
        let _ = root.set_attribute("data-selftest", "done");
    }
}

fn emit(document: &Document, lines: &[String]) {
    let pre = document
        .get_element_by_id("selftest")
        .or_else(|| {
            let pre = document.create_element("pre").ok()?;
            let _ = pre.set_attribute("id", "selftest");
            let _ = pre.set_attribute(
                "style",
                "position:fixed;left:0;bottom:0;max-height:40vh;overflow:auto;z-index:999;background:var(--pre-bg);color:var(--ink);font:11px/1.5 monospace;padding:8px;margin:0;white-space:pre-wrap;",
            );
            document.body()?.append_child(&pre).ok()?;
            Some(pre)
        });
    if let Some(pre) = pre {
        pre.set_text_content(Some(&lines.join("\n")));
        let _ = pre.set_attribute("data-done", "1");
    }
}

fn result(id: &str, ok: bool, detail: impl Into<String>) -> String {
    format!("{id}|{}|{}", if ok { "ok" } else { "fail" }, detail.into())
}

async fn wait_for(mut predicate: impl FnMut() -> bool, tries: usize) -> bool {
    for _ in 0..tries {
        if predicate() {
            return true;
        }
        TimeoutFuture::new(100).await;
    }
    predicate()
}

/* ------------------------------------------------------------------- helpers */

fn query(document: &Document, selector: &str) -> Option<Element> {
    document.query_selector(selector).ok().flatten()
}

fn query_all(document: &Document, selector: &str) -> Vec<Element> {
    let Ok(nodes) = document.query_selector_all(selector) else {
        return Vec::new();
    };
    (0..nodes.length())
        .filter_map(|index| nodes.item(index))
        .filter_map(|node| node.dyn_into::<Element>().ok())
        .collect()
}

fn height(element: &Element) -> f64 {
    element.get_bounding_client_rect().height()
}

fn width(element: &Element) -> f64 {
    element.get_bounding_client_rect().width()
}

fn bottom(element: &Element) -> f64 {
    let rect = element.get_bounding_client_rect();
    rect.top() + rect.height()
}

fn text(element: &Element) -> String {
    element.text_content().unwrap_or_default()
}

fn approx(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

fn click(element: &Element) {
    if let Some(element) = element.dyn_ref::<HtmlElement>() {
        element.click();
    }
}

fn click_selector(document: &Document, selector: &str) -> bool {
    match query(document, selector) {
        Some(element) => {
            click(&element);
            true
        }
        None => false,
    }
}

fn pointer(element: &Element, kind: &str, x: f64) {
    let init = PointerEventInit::new();
    init.set_client_x(x as i32);
    init.set_client_y(40);
    init.set_bubbles(true);
    init.set_pointer_id(1);
    if let Ok(event) = PointerEvent::new_with_event_init_dict(kind, &init) {
        let _ = element.dispatch_event(&event);
    }
}

fn pointer_window(kind: &str, x: f64) {
    if let Some(window) = web_sys::window() {
        let init = PointerEventInit::new();
        init.set_client_x(x as i32);
        init.set_client_y(40);
        init.set_bubbles(true);
        init.set_pointer_id(1);
        if let Ok(event) = PointerEvent::new_with_event_init_dict(kind, &init) {
            let _ = window.dispatch_event(&event);
        }
    }
}

fn fire(element: &Element, kind: &str) {
    let init = EventInit::new();
    init.set_bubbles(true);
    if let Ok(event) = Event::new_with_event_init_dict(kind, &init) {
        let _ = element.dispatch_event(&event);
    }
}

fn shadow_host(document: &Document, page: usize) -> Option<web_sys::ShadowRoot> {
    query(
        document,
        &format!(".page-slot[data-page='{page}'] .page-host"),
    )?
    .shadow_root()
}

/* ---------------------------------------------------------------------- S1 */

fn s1(document: &Document) -> String {
    let Some(titlebar) = query(document, ".titlebar") else {
        return result("S1", false, "缺少 titlebar");
    };
    let Some(tabs) = query(document, ".tabs") else {
        return result("S1", false, "缺少 tabs");
    };
    let Some(statusbar) = query(document, ".statusbar") else {
        return result("S1", false, "缺少 statusbar");
    };
    let heights = (height(&titlebar), height(&tabs), height(&statusbar));
    let ok_heights = approx(heights.0, 38.0, 0.75)
        && approx(heights.1, 30.0, 0.75)
        && approx(heights.2, 24.0, 0.75);

    let side_head = query(document, ".side-head");
    let ai_head = query(document, ".ai-header");
    let chrome_ok = match (side_head, ai_head) {
        (Some(side), Some(ai)) => {
            approx(height(&side), 30.0, 0.75)
                && approx(height(&ai), 30.0, 0.75)
                && approx(bottom(&side), bottom(&tabs), 0.75)
                && approx(bottom(&ai), bottom(&tabs), 0.75)
        }
        _ => false,
    };

    let full_width = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|value| value.as_f64())
        .map(|inner| approx(width(&statusbar), inner, 1.0))
        .unwrap_or(false);
    let navs = query_all(document, ".nav").len();
    let split_ok = query(document, "#source-pane")
        .map(|element| !element.has_attribute("hidden"))
        .unwrap_or(false)
        && query(document, "#proof-pane")
            .map(|element| !element.has_attribute("hidden"))
            .unwrap_or(false);
    let verify_ok = query(document, "#verify-chip")
        .map(|element| !element.has_attribute("hidden") && text(&element).contains("wasm = native"))
        .unwrap_or(false);

    let detail = format!(
        "chrome {:.0}/{:.0}/{:.0} · 30px 同底 {chrome_ok} · 状态栏全宽 {full_width} · 导航树 {navs} · 默认分栏 {split_ok} · 校验徽标 {verify_ok}",
        heights.0, heights.1, heights.2
    );
    result(
        "S1",
        ok_heights && chrome_ok && full_width && navs == 1 && split_ok && verify_ok,
        detail,
    )
}

/* ---------------------------------------------------------------------- S2 */

async fn s2(document: &Document, studio: Studio) -> String {
    click_selector(document, ".nav-item[title='b.md']");
    let two = wait_for(|| studio.tabs.with(|tabs| tabs.len() == 2), 40).await;
    click_selector(document, ".nav-item[title='c.md']");
    let three = wait_for(|| studio.tabs.with(|tabs| tabs.len() == 3), 40).await;
    click_selector(document, ".nav-item[title='b.md']");
    TimeoutFuture::new(200).await;
    let no_duplicate = studio.tabs.with(|tabs| tabs.len() == 3);
    let opened = two && three && no_duplicate;

    click_selector(document, ".tab[data-page='c.md']");
    wait_for(
        || query(document, ".tab.active[data-page='c.md']").is_some(),
        20,
    )
    .await;
    let active_before = studio.tabs.with_untracked(|tabs| {
        tabs.get(studio.active.get_untracked())
            .map(|tab| tab.path.clone())
    });
    let right = if active_before.as_deref() == Some("c.md") {
        click_selector(document, ".tab.active .tab-close");
        wait_for(
            || {
                studio.tabs.with(|tabs| {
                    tabs.get(studio.active.get_untracked())
                        .map(|tab| tab.path == "b.md")
                        .unwrap_or(false)
                })
            },
            20,
        )
        .await
    } else {
        false
    };

    let mut closed_all = false;
    for _ in 0..4 {
        let before = studio.tabs.with_untracked(|tabs| tabs.len());
        if before == 0 {
            closed_all = true;
            break;
        }
        wait_for(|| query(document, ".tab.active").is_some(), 10).await;
        click_selector(document, ".tab.active .tab-close");
        wait_for(
            || {
                studio
                    .tabs
                    .with(|tabs| tabs.len() < before || tabs.is_empty())
            },
            20,
        )
        .await;
    }
    if studio.tabs.with(|tabs| tabs.is_empty()) {
        closed_all = true;
    }
    TimeoutFuture::new(160).await;
    let empty_visible = query(document, "#empty-workspace")
        .map(|element| !element.has_attribute("hidden"))
        .unwrap_or(false);
    click_selector(document, ".tab-add");
    TimeoutFuture::new(160).await;
    let palette_open = query(document, ".palette-overlay")
        .map(|element| !element.has_attribute("hidden"))
        .unwrap_or(false);
    if let Some(overlay) = query(document, ".palette-overlay") {
        click(&overlay);
    }
    TimeoutFuture::new(120).await;
    let palette_closed = query(document, ".palette-overlay")
        .map(|element| element.has_attribute("hidden"))
        .unwrap_or(false);

    let detail = format!(
        "开 3 个 {opened} · 活动 {} · 右邻 {right} · 关完 {closed_all} · 空工作区 {empty_visible} · + 打开面板 {palette_open}/{palette_closed}",
        active_before.unwrap_or_default()
    );
    result(
        "S2",
        opened && right && closed_all && empty_visible && palette_open && palette_closed,
        detail,
    )
}

/* ---------------------------------------------------------------------- S3 */

async fn s3(document: &Document, _studio: Studio) -> String {
    let Some(sidebar) = query(document, ".sidebar") else {
        return result("S3", false, "缺少侧栏");
    };
    let default_ok = approx(width(&sidebar), 216.0, 1.0);
    let Some(side_resizer) = query(document, ".sidebar > .resizer") else {
        return result("S3", false, "缺少侧栏 resizer");
    };
    let rect = side_resizer.get_bounding_client_rect();
    let x = rect.left() + rect.width() / 2.0;
    pointer(&side_resizer, "pointerdown", x);
    pointer_window("pointermove", x + 60.0);
    pointer_window("pointerup", x + 60.0);
    TimeoutFuture::new(80).await;
    let dragged_ok = approx(width(&sidebar), 276.0, 2.0);
    let stored = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("soma.sidebar_width").ok().flatten())
        .unwrap_or_default();
    let persisted = stored == "276";

    pointer(&side_resizer, "pointerdown", x);
    pointer_window("pointermove", x + 2000.0);
    pointer_window("pointerup", x + 2000.0);
    TimeoutFuture::new(60).await;
    let max_ok = approx(width(&sidebar), 380.0, 1.0);
    pointer(&side_resizer, "pointerdown", x);
    pointer_window("pointermove", x - 2000.0);
    pointer_window("pointerup", x - 2000.0);
    TimeoutFuture::new(60).await;
    let min_ok = approx(width(&sidebar), 180.0, 1.0);
    fire(&side_resizer, "dblclick");
    TimeoutFuture::new(60).await;
    let reset_ok = approx(width(&sidebar), 216.0, 1.0);

    click_selector(document, ".side-head button[aria-label='收起侧栏']");
    TimeoutFuture::new(80).await;
    let collapsed = query(document, ".sidebar")
        .map(|element| element.class_list().contains("collapsed"))
        .unwrap_or(false);
    click_selector(document, "[aria-label='切换侧栏']");
    TimeoutFuture::new(80).await;
    let expanded = query(document, ".sidebar")
        .map(|element| !element.class_list().contains("collapsed"))
        .unwrap_or(false);

    let Some(ai) = query(document, "#ai-dock") else {
        return result("S3", false, "缺少助手");
    };
    let ai_open = !ai.class_list().contains("open");
    if ai_open {
        click_selector(document, "[aria-label='排版助手']");
        TimeoutFuture::new(80).await;
    }
    let ai_default = approx(width(&ai), 420.0, 2.0);
    let Some(ai_resizer) = query(document, "#ai-resizer") else {
        return result("S3", false, "缺少助手 resizer");
    };
    let rect = ai_resizer.get_bounding_client_rect();
    let ax = rect.left() + rect.width() / 2.0;
    pointer(&ai_resizer, "pointerdown", ax);
    pointer_window("pointermove", ax - 80.0);
    pointer_window("pointerup", ax - 80.0);
    TimeoutFuture::new(80).await;
    let ai_drag_ok = approx(width(&ai), 500.0, 2.0);
    pointer(&ai_resizer, "pointerdown", ax);
    pointer_window("pointermove", ax - 2000.0);
    pointer_window("pointerup", ax - 2000.0);
    TimeoutFuture::new(60).await;
    let ai_max_ok = approx(width(&ai), 900.0, 1.0);
    pointer(&ai_resizer, "pointerdown", ax);
    pointer_window("pointermove", ax + 2000.0);
    pointer_window("pointerup", ax + 2000.0);
    TimeoutFuture::new(60).await;
    let ai_min_ok = approx(width(&ai), 320.0, 1.0);
    fire(&ai_resizer, "dblclick");
    TimeoutFuture::new(60).await;
    let ai_reset_ok = approx(width(&ai), 420.0, 2.0);
    click_selector(
        document,
        "#ai-dock .ai-header button[aria-label='关闭助手']",
    );
    let closed = wait_for(
        || {
            query(document, "#ai-dock")
                .map(|element| !element.class_list().contains("open") && width(&element) <= 1.0)
                .unwrap_or(false)
        },
        20,
    )
    .await;
    let ai_closed = closed;

    let source_before = query(document, "#source-pane")
        .map(|element| width(&element))
        .unwrap_or(0.0);
    let split_dragged = match query(document, "#split-resizer") {
        Some(resizer) => {
            let rect = resizer.get_bounding_client_rect();
            let x = rect.left() + rect.width() / 2.0;
            pointer(&resizer, "pointerdown", x);
            pointer_window("pointermove", x + 80.0);
            pointer_window("pointerup", x + 80.0);
            TimeoutFuture::new(80).await;
            let dragged = query(document, "#source-pane")
                .map(|element| width(&element))
                .unwrap_or(0.0)
                > source_before + 40.0;
            fire(&resizer, "dblclick");
            TimeoutFuture::new(80).await;
            let reset = query(document, "#source-pane")
                .map(|element| approx(width(&element), source_before, 3.0))
                .unwrap_or(false);
            (dragged, reset)
        }
        None => (false, false),
    };

    click_selector(document, "#source-pane [aria-label='关闭源文面板']");
    let source_closed = wait_for(
        || {
            query(document, "#source-pane")
                .map(|element| element.has_attribute("hidden"))
                .unwrap_or(false)
                && query(document, "#proof-pane")
                    .map(|element| !element.has_attribute("hidden"))
                    .unwrap_or(false)
        },
        20,
    )
    .await;
    let reopened = click_selector(document, "[aria-label='并排显示源文与校样']")
        && wait_for(
            || {
                query(document, "#source-pane")
                    .map(|element| !element.has_attribute("hidden"))
                    .unwrap_or(false)
                    && query(document, "#proof-pane")
                        .map(|element| !element.has_attribute("hidden"))
                        .unwrap_or(false)
            },
            20,
        )
        .await;
    click_selector(document, "#proof-pane [aria-label='关闭校样面板']");
    let proof_closed = wait_for(
        || {
            query(document, "#proof-pane")
                .map(|element| element.has_attribute("hidden"))
                .unwrap_or(false)
                && query(document, "#source-pane")
                    .map(|element| !element.has_attribute("hidden"))
                    .unwrap_or(false)
        },
        20,
    )
    .await;
    click_selector(document, "[aria-label='并排显示源文与校样']");
    wait_for(
        || {
            query(document, "#proof-pane")
                .map(|element| !element.has_attribute("hidden"))
                .unwrap_or(false)
        },
        20,
    )
    .await;

    let ai_state = query(document, "#ai-dock")
        .map(|element| {
            format!(
                "{} {:.1}px",
                element.class_list().contains("open"),
                width(&element)
            )
        })
        .unwrap_or_default();

    let detail = format!(
        "侧栏 216 {default_ok} · 拖 {dragged_ok}/存 {persisted} · 夹 180..380 {min_ok}/{max_ok} · 复位 {reset_ok} · 折叠 {collapsed}/{expanded} · AI 420 {ai_default} · 拖 {ai_drag_ok} · 夹 320..900 {ai_min_ok}/{ai_max_ok} · 复位 {ai_reset_ok} · 关 0 {ai_closed} ({ai_state}) · 分栏 拖 {:?} 复位 {:?} · 面板 关源文 {source_closed}/重开 {reopened}/关校样 {proof_closed}",
        split_dragged.0, split_dragged.1
    );
    result(
        "S3",
        default_ok
            && dragged_ok
            && persisted
            && min_ok
            && max_ok
            && reset_ok
            && collapsed
            && expanded
            && ai_default
            && ai_drag_ok
            && ai_min_ok
            && ai_max_ok
            && ai_reset_ok
            && ai_closed
            && split_dragged.0
            && split_dragged.1
            && source_closed
            && reopened
            && proof_closed,
        detail,
    )
}

/* ---------------------------------------------------------------------- S4 */

async fn s4(document: &Document, studio: Studio) -> String {
    let Some(root) = document.document_element() else {
        return result("S4", false, "缺少 html");
    };
    let initial = root
        .get_attribute("data-theme")
        .unwrap_or_else(|| String::from("—"));
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
    let stored_before = storage
        .as_ref()
        .and_then(|storage| storage.get_item("soma.theme").ok().flatten())
        .unwrap_or_default();
    let background = |name: &str| {
        web_sys::window()
            .and_then(|window| window.get_computed_style(&root).ok().flatten())
            .and_then(|style| style.get_property_value(name).ok())
            .unwrap_or_default()
    };
    let bg_before = background("--bg");
    click_selector(document, ".titlebar button[aria-label='切换主题']");
    TimeoutFuture::new(80).await;
    let after = root
        .get_attribute("data-theme")
        .unwrap_or_else(|| String::from("—"));
    let stored_after = storage
        .as_ref()
        .and_then(|storage| storage.get_item("soma.theme").ok().flatten())
        .unwrap_or_default();
    let bg_after = background("--bg");
    let toggled = after != initial && stored_after == after && bg_after != bg_before;
    click_selector(document, ".titlebar button[aria-label='切换主题']");
    TimeoutFuture::new(80).await;
    let back = root
        .get_attribute("data-theme")
        .unwrap_or_else(|| String::from("—"));
    let back_ok = back == initial;
    let _ = studio;
    let detail = format!(
        "初始 {initial}/{stored_before} · 切换 {toggled} · 存储 {stored_after} · 回 {back_ok}"
    );
    result("S4", toggled && back_ok, detail)
}

/* ---------------------------------------------------------------------- S5 */

async fn s5(document: &Document, studio: Studio) -> String {
    let slots = query_all(document, ".page-slot");
    let pages = studio.page_count();
    let fragments = studio.build.with(|build| {
        build
            .as_ref()
            .map(|build| build.svg_fragments.len())
            .unwrap_or(0)
    });
    let contiguous = slots.iter().enumerate().all(|(index, element)| {
        element
            .get_attribute("data-page")
            .and_then(|value| value.parse::<usize>().ok())
            == Some(index + 1)
    });
    let shadow_ok = slots.iter().enumerate().all(|(index, slot)| {
        let Some(host) = slot.query_selector(".page-host").ok().flatten() else {
            return false;
        };
        let Some(shadow) = host.shadow_root() else {
            return false;
        };
        shadow
            .query_selector(&format!(".page[data-page='{}']", index + 1))
            .ok()
            .flatten()
            .is_some()
    });
    let verified = wait_for(
        || {
            studio.verify.with(|verify| {
                verify
                    .as_ref()
                    .map(|verify| {
                        verify.hash
                            == studio
                                .summary()
                                .map(|summary| summary.hash)
                                .unwrap_or_default()
                    })
                    .unwrap_or(false)
            })
        },
        80,
    )
    .await;
    let same_pages = studio.verify.with(|verify| {
        verify
            .as_ref()
            .map(|verify| verify.pages as usize == pages)
            .unwrap_or(false)
    });
    let status_ok = query(document, "#status-doc")
        .map(|element| text(&element).contains(&format!("{pages} 页")))
        .unwrap_or(false);

    let slots = slots.len();
    let detail = format!(
        "槽 {slots}/页 {pages}/fragment {fragments} · 连号 {contiguous} · Shadow 盒 {shadow_ok} · 校验 {verified} · 同页数 {same_pages} · 状态栏 {status_ok}"
    );
    result(
        "S5",
        slots > 0
            && slots == pages
            && pages == fragments
            && contiguous
            && shadow_ok
            && verified
            && same_pages
            && status_ok,
        detail,
    )
}

/* ---------------------------------------------------------------------- S6 */

async fn s6(document: &Document, studio: Studio) -> String {
    let Some(editor) = query(document, "#source-editor") else {
        return result("S6", false, "缺少编辑器");
    };
    let Some(editor) = editor.dyn_ref::<web_sys::HtmlTextAreaElement>() else {
        return result("S6", false, "编辑器类型");
    };
    let before_seq = studio.build_seq.get_untracked();
    let before_hash = studio
        .summary()
        .map(|summary| summary.hash)
        .unwrap_or_default();
    let mut value = editor.value();
    value.push_str("\n\nLive probe paragraph with 中英 mixed 文本。\n");
    value.push_str("自动换行 ");
    value.push_str(&"长".repeat(180));
    editor.set_value(&value);
    fire(editor.as_ref(), "input");
    let rebuilt = wait_for(
        || {
            studio.build_seq.get_untracked() > before_seq
                && !studio.building.get_untracked()
                && studio.build.with(|build| build.is_some())
        },
        80,
    )
    .await;
    let after_hash = studio
        .summary()
        .map(|summary| summary.hash)
        .unwrap_or_default();
    let elapsed = studio
        .build
        .with(|build| build.as_ref().map(|build| build.elapsed_ms).unwrap_or(0));
    let changed = !before_hash.is_empty() && before_hash != after_hash;
    TimeoutFuture::new(80).await;
    let wrap_ok = editor.scroll_width() <= editor.client_width() + 1;

    let comp_seq = studio.build_seq.get_untracked();
    fire(editor.as_ref(), "compositionstart");
    let mut composing_value = editor.value();
    composing_value.push('拼');
    editor.set_value(&composing_value);
    fire(editor.as_ref(), "input");
    TimeoutFuture::new(700).await;
    let quiet = studio.build_seq.get_untracked() == comp_seq;
    fire(editor.as_ref(), "compositionend");
    let rebuilt_after = wait_for(|| studio.build_seq.get_untracked() > comp_seq, 60).await;

    let detail = format!(
        "输入重建 {rebuilt} · 哈希变 {changed} · {elapsed} ms · 自动换行 {wrap_ok} · 组合静默 {quiet} · 组合后重建 {rebuilt_after}"
    );
    result(
        "S6",
        rebuilt && changed && wrap_ok && quiet && rebuilt_after,
        detail,
    )
}

/* ---------------------------------------------------------------------- S7 */

async fn s7(document: &Document, studio: Studio) -> String {
    state::set_mode(studio, Mode::Inspect);
    TimeoutFuture::new(320).await;
    let rows = query_all(document, "tr[data-line-row]");
    let overlay = shadow_host(document, 1)
        .map(|shadow| {
            shadow
                .query_selector_all(".ov-line")
                .map(|nodes| nodes.length() as usize)
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let first = rows.first().cloned();
    if let Some(first) = &first {
        click(first);
        TimeoutFuture::new(160).await;
    }
    let selected = shadow_host(document, 1)
        .and_then(|shadow| shadow.query_selector(".ov-line.selected").ok().flatten())
        .map(|element| {
            let block = first
                .as_ref()
                .and_then(|row| row.get_attribute("data-b"))
                .unwrap_or_default();
            let line = first
                .as_ref()
                .and_then(|row| row.get_attribute("data-l"))
                .unwrap_or_default();
            element.get_attribute("data-b").as_deref() == Some(block.as_str())
                && element.get_attribute("data-l").as_deref() == Some(line.as_str())
        })
        .unwrap_or(false);
    let detail_text = query(document, "#inspect-detail")
        .map(|element| text(&element))
        .unwrap_or_default();
    let dashes = detail_text.contains('—') && detail_text.contains("D102");

    let detail = format!(
        "行 {} · 覆盖层 {overlay} · 点选 {selected} · 不可得字段 {dashes}",
        rows.len()
    );
    result(
        "S7",
        !rows.is_empty() && rows.len() == overlay && selected && dashes,
        detail,
    )
}

/* ---------------------------------------------------------------------- S8 */

fn s8(document: &Document) -> String {
    let external = query_all(
        document,
        "script[src^='http'], link[href^='http'], img[src^='http'], iframe[src]",
    )
    .into_iter()
    .filter(|element| {
        let url = element
            .get_attribute("src")
            .or_else(|| element.get_attribute("href"))
            .unwrap_or_default();
        !url.starts_with("http://127.0.0.1") && !url.starts_with("http://localhost")
    })
    .count();
    let mut scripts = 0;
    let mut anchors = 0;
    let mut pages = 0;
    for slot in query_all(document, ".page-slot") {
        let Some(host) = slot.query_selector(".page-host").ok().flatten() else {
            continue;
        };
        let Some(shadow) = host.shadow_root() else {
            continue;
        };
        pages += 1;
        scripts += shadow
            .query_selector_all("script")
            .map(|nodes| nodes.length())
            .unwrap_or(0);
        anchors += shadow
            .query_selector_all("a[href]")
            .map(|nodes| nodes.length())
            .unwrap_or(0);
    }
    let remote_disabled = query(document, "#ai-model option[value='remote']")
        .map(|element| element.has_attribute("disabled"))
        .unwrap_or(false);
    let detail = format!(
        "外部资源 {external} · Shadow 页 {pages} · script {scripts} · a[href] {anchors} · 远端关闭 {remote_disabled}"
    );
    result(
        "S8",
        external == 0 && pages > 0 && scripts == 0 && anchors == 0 && remote_disabled,
        detail,
    )
}

/* ---------------------------------------------------------------------- S9 */

async fn s9(document: &Document, studio: Studio) -> String {
    let ready = wait_for(|| studio.font_probe.with(|probe| probe.is_some()), 60).await;
    let Some(badge) = query(document, "#font-probe") else {
        return result("S9", false, "缺少字体徽标");
    };
    let label = text(&badge);
    let class = badge.get_attribute("class").unwrap_or_default();
    let title = badge.get_attribute("title").unwrap_or_default();
    let report = studio.font_probe.get_untracked();
    let fonts = studio.project.with(|project| {
        project
            .as_ref()
            .map(|project| {
                project
                    .fonts
                    .iter()
                    .filter(|font| {
                        matches!(font.role.as_str(), "serif" | "sans" | "mono" | "fallback")
                    })
                    .count()
            })
            .unwrap_or(0)
    });
    let expected_bad = fonts == 0;
    let verdict_ok = if expected_bad {
        class.contains("bad") && label.contains("不一致")
    } else {
        !class.contains("bad") && (label.contains("一致") || label.contains("接近"))
    };
    let computed_family = shadow_host(document, 1)
        .and_then(|shadow| shadow.query_selector(".block").ok().flatten())
        .and_then(|element| {
            web_sys::window().and_then(|window| window.get_computed_style(&element).ok().flatten())
        })
        .map(|style| style.get_property_value("font-family").unwrap_or_default())
        .unwrap_or_default();
    let expected_family = studio
        .project
        .with(|project| {
            project.as_ref().and_then(|project| {
                project
                    .fonts
                    .iter()
                    .find(|font| font.role == "sans")
                    .or_else(|| project.fonts.iter().find(|font| font.role == "serif"))
                    .map(|font| font.family.clone())
            })
        })
        .unwrap_or_default();
    let proof_font_ok = expected_family.is_empty()
        || computed_family
            .to_lowercase()
            .contains(&expected_family.to_lowercase());
    // The proof lives on white paper in a Shadow DOM that resets all properties.
    // If the page CSS does not pin a dark ink colour, `all:initial` falls back
    // to `canvastext`, which is white under a dark browser preference.
    let ink_ok = shadow_host(document, 1)
        .and_then(|shadow| shadow.query_selector(".block").ok().flatten())
        .and_then(|element| {
            web_sys::window().and_then(|window| window.get_computed_style(&element).ok().flatten())
        })
        .map(|style| {
            let color = style.get_property_value("color").unwrap_or_default();
            let mut parts = color
                .trim_start_matches("rgb(")
                .trim_end_matches(')')
                .split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(red), Some(green), Some(blue)) => {
                    (red * 299 + green * 587 + blue * 114) / 1000 < 128
                }
                _ => false,
            }
        })
        .unwrap_or(false);
    let var_of = |element: &Element| {
        web_sys::window()
            .and_then(|window| window.get_computed_style(element).ok().flatten())
            .map(|style| style.get_property_value("--soma-sans").unwrap_or_default())
            .unwrap_or_default()
    };
    let root_var = document
        .document_element()
        .map(|element| var_of(&element))
        .unwrap_or_default();
    let host_var = shadow_host(document, 1)
        .map(|shadow| var_of(&shadow.host()))
        .unwrap_or_default();
    let pages = query_all(document, ".page-slot").len();
    let detail = format!(
        "字体 {fonts} · {label} · {class} · {title} · 页 {pages} · 探针 {ready} · 校样字体 {computed_family}（期望 {expected_family}） · 墨色 {ink_ok} · var root={root_var} host={host_var}"
    );
    result(
        "S9",
        ready
            && verdict_ok
            && proof_font_ok
            && ink_ok
            && pages > 0
            && report.map(|report| report.samples > 0).unwrap_or(false),
        detail,
    )
}

/* --------------------------------------------------------------------- S10 */

async fn s10(document: &Document, studio: Studio) -> String {
    if !studio.ai_open.get_untracked() {
        click_selector(document, "[aria-label='排版助手']");
        TimeoutFuture::new(120).await;
    }
    let Some(input) = query(document, "#ai-input") else {
        return result("S10", false, "缺少助手输入");
    };
    let Some(input) = input.dyn_ref::<web_sys::HtmlTextAreaElement>() else {
        return result("S10", false, "助手输入类型");
    };
    input.set_value("这一行为什么这样断？");
    click_selector(document, "#ai-dock button[type='submit']");
    let replied = wait_for(
        || !query_all(document, "#ai-stream .msg.assistant").is_empty(),
        40,
    )
    .await;
    TimeoutFuture::new(220).await;
    let body = query(document, "#ai-stream .msg.assistant .msg-body");
    let sanitized = body
        .as_ref()
        .map(|element| {
            element.query_selector("script").ok().flatten().is_none()
                && element.query_selector("img").ok().flatten().is_none()
                && element
                    .query_selector("a[href^='javascript:']")
                    .ok()
                    .flatten()
                    .is_none()
        })
        .unwrap_or(false);
    let local_badge = query(document, "#ai-stream .msg.assistant .badge")
        .map(|element| text(&element).contains("本地"))
        .unwrap_or(false);
    let remote_disabled = query(document, "#ai-model option[value='remote']")
        .map(|element| element.has_attribute("disabled"))
        .unwrap_or(false);
    let detail = format!(
        "回复 {replied} · 消毒 {sanitized} · 本地徽标 {local_badge} · 远端关闭 {remote_disabled}"
    );
    result(
        "S10",
        replied && sanitized && local_badge && remote_disabled,
        detail,
    )
}

/* --------------------------------------------------------------------- S11 */

/// A tab's own frontend must drive both the wasm build and `/api/verify`.
/// Before ADR-0095 the project input's format was reused for every tab, so a
/// Markdown/HTML tab opened inside a LaTeX project was parsed as LaTeX.
async fn s11(document: &Document, studio: Studio) -> String {
    let mut parts = Vec::new();
    let mut ok = true;
    for (path, marker, forbidden, format) in [
        ("extra.html", "网页标记", "<h1", SourceFormat::Html),
        ("extra.tex", "源码命令", "\\section", SourceFormat::Latex),
    ] {
        let before = studio
            .summary()
            .map(|summary| summary.hash)
            .unwrap_or_default();
        click_selector(document, &format!(".nav-item[title='{path}']"));
        let active_ok = wait_for(
            || {
                studio.tabs.with_untracked(|tabs| {
                    tabs.get(studio.active.get_untracked())
                        .map(|tab| tab.path == path)
                        .unwrap_or(false)
                })
            },
            60,
        )
        .await;
        let verified = wait_for(
            || {
                let wasm = studio.summary();
                let native = studio.verify.get_untracked();
                match (wasm, native) {
                    (Some(wasm), Some(native)) => {
                        wasm.hash != before
                            && wasm.hash == native.hash
                            && wasm.pages == native.pages
                            && wasm.page_lines == native.page_lines
                            && !studio.building.get_untracked()
                    }
                    _ => false,
                }
            },
            200,
        )
        .await;
        TimeoutFuture::new(250).await;
        let format_ok = studio.tabs.with_untracked(|tabs| {
            tabs.get(studio.active.get_untracked())
                .map(|tab| tab.format == format)
                .unwrap_or(false)
        });
        let text = proof_text(document, studio.page_count().min(4));
        let compact = compact_text(&text);
        let marker_ok = compact.contains(&compact_text(marker));
        let clean_ok = !compact.contains(&compact_text(forbidden));
        ok = ok && active_ok && verified && format_ok && marker_ok && clean_ok;
        parts.push(format!(
            "{path} 活动 {active_ok} · 校验 {verified} · 前端 {format_ok} · 标记 {marker_ok} · 原文残留 {clean_ok} · 文本 {} 字",
            text.chars().count(),
        ));
    }
    result("S11", ok, parts.join(" | "))
}

/// Concatenated text of the first `pages` proof pages.
fn proof_text(document: &Document, pages: usize) -> String {
    let mut out = String::new();
    for page in 1..=pages {
        if let Some(shadow) = shadow_host(document, page) {
            if let Ok(Some(page)) = shadow.query_selector(".page") {
                out.push_str(&text(&page));
                out.push('\n');
            }
        }
    }
    out
}

/// The renderer puts each atom on its own HTML line, so `textContent` inserts
/// whitespace between glyphs. Compare without it.
fn compact_text(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}
