//! Verify: the wasm build and the native build must agree byte for byte.
//!
//! This is the D95 gate made visible: the canonical `PagedLayout::hash()`,
//! page count, line counts and parameter hash are compared. A mismatch is a
//! red state, not a warning.

use leptos::prelude::*;
use soma_studio_core::NativeSummary;

use crate::state::{use_studio, verify_now};

#[component]
pub fn VerifyPane() -> impl IntoView {
    let studio = use_studio();

    view! {
        <section class="doc-page" hidden=move || { studio.mode.get() != crate::state::Mode::Verify }>
            <h1>"校验：wasm vs native"</h1>
            <p class="lead">
                "同一份源文、模板与引擎参数，分别在浏览器里和 Axum 里排版。canonical 哈希、页数与每页行数必须一致。"
            </p>

            {move || {
                let wasm = studio.summary();
                let native = studio.verify.get();
                let error = studio.verify_error.get();
                let Some(wasm) = wasm else {
                    return view! {
                        <div class="banner warn">
                            <span class="fill">"还没有可校验的 wasm 构建。"</span>
                        </div>
                    }.into_any();
                };
                let Some(native) = native else {
                    return view! {
                        <div class="banner info">
                            <span class="fill">
                                {if let Some(error) = error {
                                    format!("等待 native 校验；上一次失败：{error}")
                                } else {
                                    String::from("等待 native 校验…")
                                }}
                            </span>
                            <button class="ghost" on:click=move |_| verify_now(studio)>"重试"</button>
                        </div>
                        <Summary title="wasm（浏览器）" summary=wasm.clone() other=None/>
                    }.into_any();
                };

                let same_hash = wasm.hash == native.hash;
                let same_params = wasm.params_hash == native.params_hash;
                let same_pages = wasm.pages == native.pages;
                let same_lines = wasm.page_lines == native.page_lines;
                let all = same_hash && same_params && same_pages && same_lines;
                let banner = if all {
                    view! {
                        <div class="banner ok">
                            <span class="pill ok">"一致"</span>
                            <span class="fill">"wasm 与 native 的 canonical 哈希、参数、页数与每页行数全部相等。"</span>
                        </div>
                    }.into_any()
                } else {
                    view! {
                        <div class="banner bad">
                            <span class="pill bad">"不一致"</span>
                            <span class="fill">"这是一个必须修的红状态；不要用其中一边的结果交付。"</span>
                        </div>
                    }.into_any()
                };

                view! {
                    {banner}
                    <div class="workspace" style="gap:14px;padding:0 0 14px">
                        <div style="flex:1;min-width:0">
                            <Summary title="wasm（浏览器）" summary=wasm.clone() other=Some(native.clone())/>
                        </div>
                        <div style="flex:1;min-width:0">
                            <Summary title="native（Axum）" summary=native.clone() other=Some(wasm.clone())/>
                        </div>
                    </div>
                    <div class="panel">
                        <div class="panel-head">
                            <div>
                                <h2>"逐页行数"</h2>
                                <p>"用来抓住总行数相同但分页不同的情况。"</p>
                            </div>
                            <button class="ghost" on:click=move |_| verify_now(studio)>"重新校验"</button>
                        </div>
                        <div class="table-wrap">
                            <table class="compact">
                                <thead>
                                    <tr><th class="num">"页"</th><th class="num">"wasm 行"</th><th class="num">"native 行"</th><th>"结果"</th></tr>
                                </thead>
                                <tbody>
                                    {move || {
                                        let wasm = studio.summary();
                                        let native = studio.verify.get();
                                        let Some(wasm) = wasm else { return Vec::new() };
                                        wasm.page_lines.iter().enumerate().map(|(index, lines)| {
                                            let native_lines = native.as_ref().and_then(|native| native.page_lines.get(index).copied());
                                            let ok = native_lines == Some(*lines);
                                            view! {
                                                <tr>
                                                    <td class="num">{(index + 1).to_string()}</td>
                                                    <td class="num">{lines.to_string()}</td>
                                                    <td class="num">{native_lines.map(|value| value.to_string()).unwrap_or_else(|| String::from("—"))}</td>
                                                    <td>{if ok { view! { <span class="pill ok">"ok"</span> }.into_any() } else { view! { <span class="pill bad">"diff"</span> }.into_any() }}</td>
                                                </tr>
                                            }
                                        }).collect_view()
                                    }}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }.into_any()
            }}
        </section>
    }
}

#[component]
fn Summary(
    title: &'static str,
    summary: NativeSummary,
    other: Option<NativeSummary>,
) -> impl IntoView {
    let hash_ok = other
        .as_ref()
        .map(|other| other.hash == summary.hash)
        .unwrap_or(true);
    let params_ok = other
        .as_ref()
        .map(|other| other.params_hash == summary.params_hash)
        .unwrap_or(true);
    let pages_ok = other
        .as_ref()
        .map(|other| other.pages == summary.pages)
        .unwrap_or(true);
    let lines_ok = other
        .as_ref()
        .map(|other| other.page_lines == summary.page_lines)
        .unwrap_or(true);
    view! {
        <div class="panel">
            <div class="panel-head">
                <div>
                    <h2>{title}</h2>
                    <p>{format!("{} ms", summary.elapsed_ms)}</p>
                </div>
            </div>
            <div class="panel-body">
                <div class="kv-grid">
                    <Kv label="canonicalHash" value=summary.hash.clone() ok=hash_ok/>
                    <Kv label="paramHash" value=summary.params_hash.clone() ok=params_ok/>
                    <Kv label="pages" value=summary.pages.to_string() ok=pages_ok/>
                    <Kv label="blocks" value=summary.blocks.to_string() ok=true/>
                    <Kv label="lines" value=summary.lines.to_string() ok=lines_ok/>
                    <Kv label="rulesVersion" value=summary.rules_version.to_string() ok=true/>
                    <Kv label="metricsVersion" value=summary.metrics_version.to_string() ok=true/>
                    <Kv label="engineHash" value=summary.engine_hash.clone() ok=true/>
                </div>
            </div>
        </div>
    }
}

#[component]
fn Kv(label: &'static str, value: String, ok: bool) -> impl IntoView {
    view! {
        <div class="kv">
            <div class="k">{label}</div>
            <div class="v" class:bad={!ok} style=move || { if ok { "" } else { "color:var(--red)" } }>{value}</div>
        </div>
    }
}
