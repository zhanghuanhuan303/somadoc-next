//! Verify: local builds show their wasm summary; server mode compares wasm
//! with native byte for byte.
//!
//! The local path deliberately does not claim a runtime native comparison. The
//! native/wasm equality is a CI property (`scripts/wasm-hash.sh`); a browser
//! cannot run the native binary.

use leptos::prelude::*;
use soma_studio_core::NativeSummary;

use crate::state::{use_studio, verify_now, Mode, Studio};

#[component]
pub fn VerifyPane() -> impl IntoView {
    let studio = use_studio();
    if studio.local {
        local_verify(studio).into_any()
    } else {
        server_verify(studio).into_any()
    }
}

fn local_verify(studio: Studio) -> impl IntoView {
    view! {
        <section class="doc-page" hidden=move || { studio.mode.get() != Mode::Verify }>
            <h1>"构建摘要（wasm）"</h1>
            <p class="lead">
                "整份文档在浏览器 wasm 内解析、断行、分页与校样。这里记录本次构建的 canonical 哈希、参数、页数与每页行数；native 与 wasm 的字节一致性由 CI 的 wasm-hash.sh 锁定，运行时不再请求 native。"
            </p>
            {move || {
                let Some(summary) = studio.summary() else {
                    return view! {
                        <div class="banner warn">
                            <span class="fill">"还没有构建。"</span>
                        </div>
                    }.into_any();
                };
                view! {
                    <div class="banner ok">
                        <span class="pill ok">"已记录"</span>
                        <span class="fill">"这份摘要是当前 wasm 构建的唯一事实来源；重新排版会更新它。"</span>
                    </div>
                    <div class="workspace" style="gap:14px;padding:0 0 14px">
                        <div style="flex:1;min-width:0">
                            <Summary title="wasm（浏览器）" summary=summary.clone() other=None/>
                        </div>
                    </div>
                    <div class="panel">
                        <div class="panel-head">
                            <div>
                                <h2>"逐页行数"</h2>
                                <p>"分页结果的一部分；用于同目标跨构建比较。"</p>
                            </div>
                            <button class="ghost" on:click=move |_| verify_now(studio)>"重新记录"</button>
                        </div>
                        <div class="table-wrap">
                            <table class="compact">
                                <thead>
                                    <tr><th class="num">"页"</th><th class="num">"行数"</th></tr>
                                </thead>
                                <tbody>
                                    {summary.page_lines.iter().enumerate().map(|(index, lines)| view! {
                                        <tr>
                                            <td class="num">{(index + 1).to_string()}</td>
                                            <td class="num">{lines.to_string()}</td>
                                        </tr>
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>
                    </div>
                }.into_any()
            }}
        </section>
    }
}

fn server_verify(studio: Studio) -> impl IntoView {
    view! {
        <section class="doc-page" hidden=move || { studio.mode.get() != Mode::Verify }>
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
                            <span class="fill">
                                "wasm 与 native 的 canonical 哈希、参数、页数与每页行数全部相等。"
                            </span>
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
