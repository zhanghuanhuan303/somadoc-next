//! Inspect: the exported line facts, a ratio histogram, and the parameters the
//! numbers came from. No field is invented: `Line` carries total cost only, so
//! the per-term breakdown is shown as `—` (D102).

use leptos::prelude::*;

use crate::state::{use_studio, Mode, Selection, Studio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Lines,
    Stats,
    Params,
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    page: usize,
    block: usize,
    line: usize,
    node: i64,
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
}

#[component]
pub fn InspectPane() -> impl IntoView {
    let studio = use_studio();
    let tab = RwSignal::new(Tab::Lines);
    let rows = Memo::new(move |_| page_rows(&studio));

    view! {
        <aside class="inspect-side" hidden=move || { studio.mode.get() != Mode::Inspect }>
            <div class="inspect-head">
                <span>"检视"</span>
                <span class="mono">{move || format!("第 {} 页", studio.page.get() + 1)}</span>
                <div class="fill"></div>
                <span class="mono">{move || format!("{} 行", rows.with(|rows| rows.len()))}</span>
            </div>
            <div class="inspect-tabs" role="tablist" aria-label="检视页签">
                <button class="inspect-tab" class:active=move || { tab.get() == Tab::Lines }
                    on:click=move |_| tab.set(Tab::Lines)>"行表"</button>
                <button class="inspect-tab" class:active=move || { tab.get() == Tab::Stats }
                    on:click=move |_| tab.set(Tab::Stats)>"统计"</button>
                <button class="inspect-tab" class:active=move || { tab.get() == Tab::Params }
                    on:click=move |_| tab.set(Tab::Params)>"参数"</button>
            </div>
            <div class="inspect-body">
                {move || match tab.get() {
                    Tab::Lines => view! { <LineTable rows=rows/> }.into_any(),
                    Tab::Stats => view! { <Stats rows=rows/> }.into_any(),
                    Tab::Params => view! { <Params/> }.into_any(),
                }}
            </div>
        </aside>
    }
}

#[component]
fn LineTable(rows: Memo<Vec<Row>>) -> impl IntoView {
    let studio = use_studio();
    view! {
        <div class="table-wrap">
            <table class="compact line-table">
                <thead>
                    <tr>
                        <th class="num">"#"</th>
                        <th class="num">"原子"</th>
                        <th>"比例 ‰"</th>
                        <th class="num">"调整"</th>
                        <th class="num">"裁剪 L/R"</th>
                        <th class="num">"边界"</th>
                        <th class="num">"代价"</th>
                        <th>"标记"</th>
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        let selection = studio.selection.get();
                        rows.get().into_iter().map(|row| {
                            let active = selection.page == row.page
                                && selection.block == row.block
                                && selection.line == row.line;
                            let click = move |_| {
                                studio.selection.set(Selection {
                                    page: row.page,
                                    block: row.block,
                                    line: row.line,
                                    node: row.node,
                                });
                                studio.page.set(row.page);
                                studio.mode.set(Mode::Inspect);
                            };
                            view! {
                                <tr class="clickable" class:selected=active
                                    data-line-row="1"
                                    data-page={row.page + 1}
                                    data-b={row.block}
                                    data-l={row.line}
                                    on:click=click>
                                    <td class="num">{row.line + 1}</td>
                                    <td class="num">{format!("{}–{}", row.start, row.end)}</td>
                                    <td>
                                        <div class="ratio-cell">
                                            <span class="mono">{row.ratio}</span>
                                            <span class="ratio-bar"
                                                class:negative=row.ratio < 0
                                                style=format!(
                                                    "width:{}px;left:{}px;",
                                                    6.0 + (row.ratio.unsigned_abs() as f64 / 25.0).min(54.0),
                                                    if row.ratio < 0 { 0.0 } else { 58.0 }
                                                )></span>
                                        </div>
                                    </td>
                                    <td class="num">{row.adjustment}</td>
                                    <td class="num">{format!("{}/{}", row.trim_left, row.trim_right)}</td>
                                    <td class="num">{format!("{}{}", row.boundary, if row.boundary_count > 0 { format!(" ×{}", row.boundary_count) } else { String::new() })}</td>
                                    <td class="num">{row.cost}</td>
                                    <td>
                                        <div class="flag-list">
                                            {row.overfull.then(|| view! { <span class="mini-flag bad">"overfull"</span> })}
                                            {row.emergency.then(|| view! { <span class="mini-flag warn">"emergency"</span> })}
                                            {row.forced.then(|| view! { <span class="mini-flag info">"forced"</span> })}
                                            {row.hyphenated.then(|| view! { <span class="mini-flag">"hyphen"</span> })}
                                        </div>
                                    </td>
                                </tr>
                            }
                        }).collect_view()
                    }}
                </tbody>
            </table>
        </div>
        <div class="banner info" id="inspect-detail" style="margin:10px 12px">
            <span class="fill">
                {move || {
                    let selection = studio.selection.get();
                    if selection.block == usize::MAX {
                        String::from("选中一行查看字段；逐项代价 —（D102），字形墨迹 —（D101）。")
                    } else {
                        format!(
                            "第 {} 页 · 块 {} · 行 {}：逐项代价 —（D102），字形墨迹 —（D101）。边界偏差是引擎导出的近似值。",
                            selection.page + 1,
                            selection.block,
                            selection.line + 1
                        )
                    }
                }}
            </span>
        </div>
    }
}

#[component]
fn Stats(rows: Memo<Vec<Row>>) -> impl IntoView {
    let values = Memo::new(move |_| rows.get());
    view! {
        {move || {
            let rows = values.get();
            let count = rows.len().max(1);
            let overfull = rows.iter().filter(|row| row.overfull).count();
            let emergency = rows.iter().filter(|row| row.emergency).count();
            let trim = rows.iter().map(|row| row.trim_left + row.trim_right).sum::<i32>();
            let mean = rows.iter().map(|row| row.ratio as f64).sum::<f64>() / count as f64;
            let mut bins = vec![0usize; 21];
            for row in &rows {
                let index = ((row.ratio + 1000) / 100).clamp(0, 20) as usize;
                bins[index] += 1;
            }
            let max = bins.iter().copied().max().unwrap_or(1).max(1);
            view! {
                <div class="stat-grid">
                    <Stat label="行数" value=rows.len().to_string()/>
                    <Stat label="平均 ratio" value=format!("{:.1}‰", mean)/>
                    <Stat label="overfull" value=overfull.to_string()/>
                    <Stat label="emergency" value=emergency.to_string()/>
                    <Stat label="裁剪总量" value=trim.to_string()/>
                </div>
                <div class="histogram">
                    {bins.into_iter().map(|height| {
                        let hot = height == max && height > 0;
                        view! {
                            <div class="hist-bar" class:hot=hot
                                style=format!("height:{}%", (height as f64 / max as f64) * 100.0)></div>
                        }
                    }).collect_view()}
                </div>
                <div class="hist-axis">
                    <span>"-1000"</span><span>"-500"</span><span>"0"</span><span>"500"</span><span>">1000"</span>
                </div>
            }.into_any()
        }}
    }
}

#[component]
fn Stat(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="stat">
            <div class="k">{label}</div>
            <div class="v">{value}</div>
        </div>
    }
}

#[component]
fn Params() -> impl IntoView {
    let studio = use_studio();
    view! {
        <div class="params">
            {move || {
                let Some(build) = studio.build.with(|build| build.as_ref().map(|build| {
                    let params = build.build.params;
                    (params, build.build.pages.rules_version, build.build.pages.metrics_version)
                })) else {
                    return view! { <div class="empty-state"><span>"尚无参数"</span></div> }.into_any();
                };
                let (params, rules, metrics) = build;
                let costs = params.break_params.costs;
                let tolerance = params.break_params.tolerance.0;
                let emergency_tolerance = params.break_params.emergency_tolerance.0;
                view! {
                    <dl>
                        <dt>"preset"</dt><dd>{params.preset.0}</dd>
                        <dt>"modelVersion"</dt><dd>{params.model_version}</dd>
                        <dt>"rulesVersion"</dt><dd>{rules}</dd>
                        <dt>"metricsVersion"</dt><dd>{metrics}</dd>
                        <dt>"paramHash"</dt><dd>{format!("{:016x}", params.param_hash().0)}</dd>
                        <dt>"tolerance"</dt><dd>{format!("{}‰", tolerance)}</dd>
                        <dt>"emergencyTolerance"</dt><dd>{format!("{}‰", emergency_tolerance)}</dd>
                        <dt>"costs.line"</dt><dd>{costs.line.0}</dd>
                        <dt>"costs.badnessScale"</dt><dd>{costs.badness_scale.0}</dd>
                        <dt>"costs.badnessExponent"</dt><dd>{costs.badness_exponent}</dd>
                        <dt>"costs.breakPenalty"</dt><dd>{costs.break_penalty.0}</dd>
                        <dt>"costs.compressAmount"</dt><dd>{costs.compress_amount.0}</dd>
                        <dt>"costs.compressCount"</dt><dd>{costs.compress_count.0}</dd>
                        <dt>"costs.boundaryDeviation"</dt><dd>{costs.boundary_deviation.0}</dd>
                        <dt>"costs.shortLastScale"</dt><dd>{costs.short_last_scale.0}</dd>
                        <dt>"costs.shortLastThreshold"</dt><dd>{costs.short_last_threshold.0}</dd>
                        <dt>"edgeTrim"</dt><dd>{params.features.edge_trim}</dd>
                        <dt>"boundaryDeviation"</dt><dd>{params.features.boundary_deviation}</dd>
                        <dt>"fitness"</dt><dd>{params.features.fitness}</dd>
                    </dl>
                    <div class="note">
                        "改这些值会改变 paramHash 与断点；模板 YAML 不能写引擎旋钮。"
                    </div>
                }.into_any()
            }}
        </div>
    }
}

fn page_rows(studio: &Studio) -> Vec<Row> {
    let page = studio.page.get();
    studio.build.with(|build| {
        let Some(build) = build.as_ref() else {
            return Vec::new();
        };
        let Some(layout_page) = build.build.pages.pages.get(page) else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for (block_index, block) in layout_page.blocks.iter().enumerate() {
            append_rows(&mut rows, page, block_index, block);
        }
        let offset = layout_page.blocks.len();
        for (block_index, block) in layout_page.footnotes.iter().enumerate() {
            append_rows(&mut rows, page, offset + block_index, block);
        }
        rows
    })
}

fn append_rows(
    rows: &mut Vec<Row>,
    page: usize,
    block_index: usize,
    block: &soma_layout::PlacedBlock,
) {
    for (line_index, placed) in block.lines.iter().enumerate() {
        let line = &placed.line;
        rows.push(Row {
            page,
            block: block_index,
            line: line_index,
            node: block.node.0 as i64,
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
        });
    }
}
