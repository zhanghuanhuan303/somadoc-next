//! Vertical Knuth-Plass over lines.
//!
//! The page breaker sees one `VItem` per line of a splittable block (or per
//! atomic block). Heights are fixed and additive, so the "stop once the page is
//! full" window is an exact prune, not a heuristic.
//!
//! Page fill includes the glue before each item (except at the top of a page,
//! which is discarded, as in TeX) and the glue after every item. Stretch and
//! shrink on that glue feed the page badness, so a page can consciously pull
//! title spacing apart instead of leaving a hole at the bottom.

use alloc::vec;
use alloc::vec::Vec;

use super::*;

pub(crate) fn build_stream(
    items: &[FlowItem],
    params: VerticalParams,
) -> (Vec<VItem>, Vec<(usize, usize)>) {
    let mut stream: Vec<VItem> = Vec::new();
    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;

    for (index, item) in items.iter().enumerate() {
        let force_new_segment = item.forced_break || item.page_break_before;
        if force_new_segment && stream.len() > start {
            segments.push((start, stream.len()));
            start = stream.len();
        }
        if item.forced_break {
            continue;
        }
        push_item(&mut stream, index, item, params);
    }

    if stream.len() > start {
        segments.push((start, stream.len()));
    }
    (stream, segments)
}

fn push_item(out: &mut Vec<VItem>, index: usize, item: &FlowItem, params: VerticalParams) {
    if item.parts.is_empty() {
        out.push(VItem {
            item: index,
            part: 0,
            height: item.height,
            before: item.before,
            after: item.after,
            notes: item.footnotes.clone(),
            penalty: 0,
            can_break: !item.keep_with_next,
            master: item.master,
            table: item.table,
        });
        return;
    }

    let last = item.parts.len() - 1;
    for (part_index, part) in item.parts.iter().enumerate() {
        let mut penalty = 0i64;
        match item.table {
            // A table band starting at row zero that ends on the page leaves
            // the repeated header alone: charge the club penalty for it.
            Some(table) if table.header_rows > 0 && part_index == 0 && last > 0 => {
                penalty += params.club_penalty;
            }
            Some(_) => {}
            None => {
                if part_index == 0 && last > 0 {
                    penalty += params.club_penalty;
                }
                if part_index + 1 == last && last > 0 {
                    penalty += params.widow_penalty;
                }
            }
        }
        // A table's first part is often just the header row. A page must not
        // end there: the header would sit alone at the foot of the page while
        // its body starts on the next one.
        let header_alone = matches!(
            item.table,
            Some(table) if table.header_rows > 0 && part_index == 0 && last > 0
        );
        out.push(VItem {
            item: index,
            part: part_index,
            height: part.height,
            before: if part_index == 0 {
                item.before
            } else {
                Glue::ZERO
            },
            after: if part_index == last {
                item.after
            } else {
                Glue::ZERO
            },
            notes: part.notes.clone(),
            penalty,
            can_break: !header_alone && !(part_index == last && item.keep_with_next),
            master: item.master,
            table: item.table.map(|mut table| {
                table.row = part.row;
                table
            }),
        });
    }
}

/// Header height that must be reserved at the top of a page that starts in the
/// middle of a table.
pub(crate) fn header_reserve(item: &VItem) -> Width {
    match item.table {
        Some(table) if table.header_rows > 0 && table.row >= table.header_rows => {
            table.header_height
        }
        _ => Width::ZERO,
    }
}

/// Optimal page breaks for every segment. Returns page end indices.
pub(crate) fn place_kp(
    items: &[VItem],
    segments: &[(usize, usize)],
    capacity: Width,
    notes: &NoteMap,
    params: VerticalParams,
) -> Option<Vec<usize>> {
    let mut ends = Vec::new();
    let last_segment = segments.len().saturating_sub(1);
    for (index, &(start, end)) in segments.iter().enumerate() {
        let free_last = index == last_segment;
        ends.extend(place_segment(
            items, start, end, capacity, notes, params, free_last,
        )?);
    }
    Some(ends)
}

fn place_segment(
    items: &[VItem],
    start: usize,
    end: usize,
    capacity: Width,
    notes: &NoteMap,
    params: VerticalParams,
    free_last: bool,
) -> Option<Vec<usize>> {
    let n = end - start;
    if n == 0 {
        return Some(Vec::new());
    }
    let mut best: Vec<Option<(i64, u32, usize)>> = vec![None; n + 1];
    best[0] = Some((0, 0, start));

    for i in start..end {
        let offset = i - start;
        let Some((base_cost, base_pages, _)) = best[offset] else {
            continue;
        };
        let mut used: i64 = header_reserve(&items[i]).0 as i64;
        let mut stretch: i64 = 0;
        let mut shrink: i64 = 0;
        let mut seen: Vec<usize> = Vec::new();
        let body = items[i].height.0.max(1) as i64;

        for (j, item) in items.iter().enumerate().skip(i).take(end - i) {
            if j > i {
                used += item.before.width.0 as i64;
                stretch += item.before.plus.0 as i64;
                shrink += item.before.minus.0 as i64;
            }
            used += item.height.0 as i64 + item.after.width.0 as i64;
            stretch += item.after.plus.0 as i64;
            shrink += item.after.minus.0 as i64;
            for note in &item.notes {
                if !seen.contains(note) {
                    seen.push(*note);
                }
            }
            let total = used + note_height_of(&seen, notes).0 as i64;
            let capacity_units = capacity.0 as i64;
            let excess = total - capacity_units;
            let single = j == i;

            if excess > 0 && !single && excess > shrink {
                break;
            }

            if single && excess > 0 && excess > shrink {
                // An atomic box taller than the page owns the page.
                let cost = params.page_weight + params.overflow_penalty + item.penalty;
                relax(
                    &mut best,
                    j + 1 - start,
                    (base_cost + cost, base_pages + 1, i),
                );
                break;
            }

            let can_break = j + 1 == end || item.can_break;
            if can_break {
                let last = j + 1 == end;
                let mut cost = params.page_weight + item.penalty;
                if !(last && free_last) {
                    cost += page_badness(
                        total,
                        capacity_units,
                        stretch,
                        shrink,
                        body,
                        params.badness_scale,
                    );
                }
                relax(
                    &mut best,
                    j + 1 - start,
                    (base_cost + cost, base_pages + 1, i),
                );
            }

            if excess > 0 && !single {
                break;
            }
        }
    }

    best[n]?;
    let mut ends = Vec::new();
    let mut at = n;
    while at > 0 {
        let (_, _, previous) = best[at].expect("reachable");
        ends.push(start + at);
        at = previous - start;
    }
    ends.reverse();
    Some(ends)
}

fn relax(best: &mut [Option<(i64, u32, usize)>], to: usize, candidate: (i64, u32, usize)) {
    let replace = match best[to] {
        None => true,
        Some(existing) => candidate < existing,
    };
    if replace {
        best[to] = Some(candidate);
    }
}

/// Greedy fill over the same items. Used as an explicit strategy and as the
/// fallback when the page DP finds no feasible break sequence.
pub(crate) fn place_greedy(
    items: &[VItem],
    segments: &[(usize, usize)],
    capacity: Width,
    reserved: &[Width],
) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut page_index = 0usize;

    for &(start, end) in segments {
        if start == end {
            continue;
        }
        let mut page_start = start;
        let mut used = header_reserve(&items[start]).0 as i64
            + items[start].height.0 as i64
            + items[start].after.width.0 as i64;
        for (i, item) in items
            .iter()
            .enumerate()
            .skip(start + 1)
            .take(end - start - 1)
        {
            let limit =
                (capacity.0 - reserved.get(page_index).copied().unwrap_or(Width::ZERO).0) as i64;
            let next = used
                + item.before.width.0 as i64
                + item.height.0 as i64
                + item.after.width.0 as i64;
            if next > limit {
                ends.push(i);
                page_index += 1;
                page_start = i;
                used = header_reserve(item).0 as i64
                    + item.height.0 as i64
                    + item.after.width.0 as i64;
            } else {
                used = next;
            }
        }
        if page_start < end {
            ends.push(end);
            page_index += 1;
        }
    }

    // `keep-with-next` may not be the last item on a page. Pull the boundary
    // back while it is safe to do so.
    for index in 0..ends.len().saturating_sub(1) {
        let floor = if index == 0 { 0 } else { ends[index - 1] };
        while ends[index] > floor + 1 {
            let last = ends[index] - 1;
            if items[last].can_break || items[last].part == 0 {
                break;
            }
            ends[index] -= 1;
        }
    }
    ends
}

/// Page badness. Leftover space is normalized by the body line height, so
/// "one line short" costs a noticeable amount instead of rounding to zero.
pub(crate) fn page_badness(
    natural: i64,
    capacity: i64,
    stretch: i64,
    shrink: i64,
    body: i64,
    scale: i64,
) -> i64 {
    if natural > capacity {
        let excess = natural - capacity;
        if shrink > 0 && excess <= shrink {
            let ratio = excess * 1000 / shrink;
            return scale * ratio * ratio * ratio / 1_000_000_000;
        }
        return 1_000_000;
    }
    let free = capacity - natural;
    if free <= 0 {
        return 0;
    }
    if stretch > 0 && free <= stretch {
        let ratio = free * 1000 / stretch;
        return scale * ratio * ratio * ratio / 1_000_000_000;
    }
    let remaining = if stretch > 0 { free - stretch } else { free };
    let milli = remaining * 1000 / body.max(1);
    scale * milli * milli * milli / 1_000_000_000
}

pub(crate) fn note_height_of(notes_seen: &[usize], notes: &NoteMap) -> Width {
    let mut height = if notes_seen.is_empty() {
        Width::ZERO
    } else {
        crate::FOOTNOTE_RULE_THICKNESS + crate::FOOTNOTE_RULE_GAP
    };
    for note in notes_seen {
        if let Some(blocks) = notes.blocks.get(*note) {
            for block in blocks {
                height = height + block.height;
            }
        }
    }
    height
}

/// Footnote indices used by one page, in first-appearance order.
pub(crate) fn page_note_indices(start: usize, end: usize, items: &[VItem]) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    for item in &items[start..end] {
        for note in &item.notes {
            if !seen.contains(note) {
                seen.push(*note);
            }
        }
    }
    seen
}

pub(crate) fn page_note_height(
    start: usize,
    end: usize,
    items: &[VItem],
    notes: &NoteMap,
) -> Width {
    note_height_of(&page_note_indices(start, end, items), notes)
}

pub(crate) fn footnote_heights(ends: &[usize], items: &[VItem], notes: &NoteMap) -> Vec<Width> {
    let mut out = Vec::with_capacity(ends.len());
    let mut previous = 0usize;
    for &end in ends {
        out.push(page_note_height(previous, end, items, notes));
        previous = end;
    }
    out
}

/// Footnotes for one page, plus the definition index of each returned block.
/// The separator rule is drawn by the renderer at `y = 0`; the text starts
/// below it, so the reservation and the placed geometry agree.
pub(crate) fn assemble_footnotes(
    start: usize,
    end: usize,
    items: &[VItem],
    notes: &NoteMap,
) -> (Vec<PlacedBlock>, Vec<usize>) {
    let used = page_note_indices(start, end, items);
    let mut out = Vec::new();
    let mut ids = Vec::new();
    if used.is_empty() {
        return (out, ids);
    }
    let mut y = crate::FOOTNOTE_RULE_THICKNESS + crate::FOOTNOTE_RULE_GAP;
    for note in used {
        if let Some(blocks) = notes.blocks.get(note) {
            for block in blocks {
                let mut placed = block.clone();
                placed.y = y;
                y = y + placed.height;
                out.push(placed);
                ids.push(note);
            }
        }
    }
    (out, ids)
}

/// Greedy fill with the footnote reservation fixed point: paginate, measure
/// the footnote areas, paginate again until the reservation stops changing.
pub(crate) fn greedy_with_notes(
    items: &[VItem],
    segments: &[(usize, usize)],
    capacity: Width,
    notes: &NoteMap,
    passes: &mut u32,
) -> Vec<usize> {
    let mut reserved: Vec<Width> = Vec::new();
    let mut ends = place_greedy(items, segments, capacity, &reserved);
    for _ in 0..8 {
        let heights = footnote_heights(&ends, items, notes);
        if heights == reserved {
            break;
        }
        reserved = heights;
        ends = place_greedy(items, segments, capacity, &reserved);
        *passes += 1;
    }
    ends
}

/// Cost of a finished pagination, for reporting and for the optimality test.
pub(crate) fn score_pages(
    ends: &[usize],
    items: &[VItem],
    notes: &NoteMap,
    capacity: Width,
    params: VerticalParams,
) -> i64 {
    let last = ends.len().saturating_sub(1);
    let mut total = 0i64;
    let mut previous = 0usize;
    for (index, &end) in ends.iter().enumerate() {
        let mut used: i64 = header_reserve(&items[previous]).0 as i64;
        let mut stretch: i64 = 0;
        let mut shrink: i64 = 0;
        let mut body = 1i64;
        for (offset, item) in items[previous..end].iter().enumerate() {
            if offset > 0 {
                used += item.before.width.0 as i64;
                stretch += item.before.plus.0 as i64;
                shrink += item.before.minus.0 as i64;
            }
            used += item.height.0 as i64 + item.after.width.0 as i64;
            stretch += item.after.plus.0 as i64;
            shrink += item.after.minus.0 as i64;
            body = item.height.0.max(1) as i64;
        }
        let natural = used + page_note_height(previous, end, items, notes).0 as i64;
        let mut cost = params.page_weight;
        if index != last {
            cost += page_badness(
                natural,
                capacity.0 as i64,
                stretch,
                shrink,
                body,
                params.badness_scale,
            );
            cost += items[previous..end]
                .iter()
                .filter(|item| item.penalty > 0)
                .map(|item| item.penalty)
                .sum::<i64>();
        }
        total += cost;
        previous = end;
    }
    total
}

/// Build the placed blocks for one page, slicing split paragraphs by line.
///
/// Stretchable glue is distributed into the free space of the page (never more
/// than `plus`), so a page with room to spare widens its block spacing instead
/// of leaving the slack at the bottom.
pub(crate) fn assemble_page_blocks(
    items: &[FlowItem],
    vertical: &[VItem],
    start: usize,
    end: usize,
    capacity: Width,
    note_height: Width,
    tables: &[TableHeader],
) -> (Vec<PlacedBlock>, Width) {
    let mut blocks: Vec<PlacedBlock> = Vec::new();
    let reserve = header_reserve(&vertical[start]);
    let mut used = Width::ZERO;
    if reserve.0 > 0 {
        if let Some(table) = vertical[start].table {
            if let Some(header) = tables.get(table.id as usize) {
                for block in &header.blocks {
                    blocks.push(block.clone());
                }
                used = header.height;
            }
        }
    }
    let mut index = start;

    let (natural, stretch) = page_fill(&vertical[start..end]);
    // A repeated header sits above the block stream, so it is not part of the
    // natural fill; subtract it here or the stretch would overshoot the box.
    let free = (capacity.0 - note_height.0 - reserve.0 - natural.0).max(0) as i64;
    let delta = free.min(stretch);
    let mut cumulative = 0i64;

    while index < end {
        let item_index = vertical[index].item;
        let item = &items[item_index];
        if !blocks.is_empty() {
            let plus = vertical[index].before.plus.0 as i64;
            used = used + vertical[index].before.width;
            used = used + Width(stretch_share(delta, stretch, cumulative, plus));
            cumulative += plus;
        }
        let y = used;

        if item.parts.is_empty() {
            for block in &item.blocks {
                let mut placed = block.clone();
                placed.y = placed.y + y;
                blocks.push(placed);
            }
            used = used + vertical[index].height;
            let plus = vertical[index].after.plus.0 as i64;
            used = used + Width(stretch_share(delta, stretch, cumulative, plus));
            cumulative += plus;
            used = used + vertical[index].after.width;
            index += 1;
            continue;
        }

        // A table is one flow item split into row bands. Every break falls on
        // a row boundary that no cell spans, so the continuation can be built
        // by shifting whole cell blocks instead of clipping lines.
        if item.table.is_some() {
            let first_part = vertical[index].part;
            let cut = item
                .parts
                .iter()
                .take(first_part)
                .fold(Width::ZERO, |sum, part| sum + part.height);
            let top = used;
            let mut run = index;
            while run < end && vertical[run].item == item_index {
                if run > index {
                    let plus = vertical[run].before.plus.0 as i64;
                    used = used + vertical[run].before.width;
                    used = used + Width(stretch_share(delta, stretch, cumulative, plus));
                    cumulative += plus;
                }
                used = used + vertical[run].height;
                let plus = vertical[run].after.plus.0 as i64;
                used = used + Width(stretch_share(delta, stretch, cumulative, plus));
                cumulative += plus;
                used = used + vertical[run].after.width;
                run += 1;
            }
            let cut_end = cut + Width((used.0 - top.0).max(0));
            for block in &item.blocks {
                if block.y.0 + block.height.0 <= cut.0 {
                    continue;
                }
                if block.y.0 >= cut_end.0 {
                    continue;
                }
                let mut placed = block.clone();
                placed.y = placed.y - cut + top;
                blocks.push(placed);
            }
            index = run;
            continue;
        }

        let template = &item.blocks[0];
        let lines_total = template.lines.len() as u32;
        let first_part = vertical[index].part;
        let mut lines: Vec<PlacedLine> = Vec::new();
        let mut marks: Vec<Mark> = Vec::new();
        let mut line_y = Width::ZERO;
        let mut run = index;

        while run < end && vertical[run].item == item_index {
            let part = vertical[run].part;
            let line = &template.lines[part];
            lines.push(PlacedLine {
                y: line_y,
                hang: line.hang,
                line: line.line.clone(),
            });
            line_y = line_y + vertical[run].height;
            for mark in &template.marks {
                if mark.line as usize == part {
                    marks.push(*mark);
                }
            }
            if run > index {
                let plus = vertical[run].before.plus.0 as i64;
                used = used + vertical[run].before.width;
                used = used + Width(stretch_share(delta, stretch, cumulative, plus));
                cumulative += plus;
            }
            used = used + vertical[run].height;
            let plus = vertical[run].after.plus.0 as i64;
            used = used + Width(stretch_share(delta, stretch, cumulative, plus));
            cumulative += plus;
            used = used + vertical[run].after.width;
            run += 1;
        }

        let mut block = template.clone();
        block.y = y;
        block.height = line_y;
        block.lines = lines;
        block.marks = marks;
        block.line_offset = first_part as u32;
        block.lines_total = lines_total;
        blocks.push(block);
        index = run;
    }

    (blocks, used)
}

/// Natural page fill and total stretch of one page slice.
fn page_fill(items: &[VItem]) -> (Width, i64) {
    let mut natural: i64 = 0;
    let mut stretch: i64 = 0;
    for (offset, item) in items.iter().enumerate() {
        if offset > 0 {
            natural += item.before.width.0 as i64;
            stretch += item.before.plus.0 as i64;
        }
        natural += item.height.0 as i64;
        natural += item.after.width.0 as i64;
        stretch += item.after.plus.0 as i64;
    }
    let width = if natural > i32::MAX as i64 {
        i32::MAX
    } else {
        natural as i32
    };
    (Width(width), stretch)
}

fn stretch_share(delta: i64, total: i64, cumulative: i64, plus: i64) -> i32 {
    if total <= 0 || delta <= 0 || plus <= 0 {
        return 0;
    }
    let before = delta * cumulative / total;
    let after = delta * (cumulative + plus) / total;
    (after - before) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(height: i32, keep: bool) -> VItem {
        VItem {
            item: 0,
            part: 0,
            height: Width(height),
            before: Glue::ZERO,
            after: Glue::ZERO,
            notes: Vec::new(),
            penalty: 0,
            can_break: !keep,
            master: 0,
            table: None,
        }
    }

    fn notes() -> crate::NoteMap {
        crate::NoteMap {
            labels: Vec::new(),
            blocks: Vec::new(),
        }
    }

    #[test]
    fn a_figure_and_its_caption_stay_together() {
        // figure + caption + figure + caption + paragraph. The first four fit
        // on a page; the paragraph does not, and no break may fall between a
        // figure and its caption.
        let items = vec![
            item(85_514, true),
            item(4_914, false),
            item(85_514, true),
            item(4_914, false),
            item(13_926, false),
        ];
        let ends = place_kp(
            &items,
            &[(0, items.len())],
            Width(194_600),
            &notes(),
            VerticalParams::default_v1(),
        )
        .expect("feasible");
        assert_eq!(ends, vec![4, 5]);

        // With room for everything, one page holds the whole run.
        let one = place_kp(
            &items,
            &[(0, items.len())],
            Width(200_000),
            &notes(),
            VerticalParams::default_v1(),
        )
        .expect("feasible");
        assert_eq!(one, vec![items.len()]);
    }

    #[test]
    fn debug_two_figures_with_paragraphs() {
        let items = vec![
            item(85_514, true),
            item(4_914, false),
            item(13_926, false),
            item(85_514, true),
            item(4_914, false),
            item(13_926, false),
        ];
        let ends = place_kp(
            &items,
            &[(0, items.len())],
            Width(194_600),
            &notes(),
            VerticalParams::default_v1(),
        )
        .expect("feasible");
        println!("ends={ends:?}");
    }

    #[test]
    fn a_page_never_ends_between_a_figure_and_its_caption() {
        // A page that can hold the figure but not the caption must move the
        // pair, not leave a lone figure at the foot.
        let items = vec![item(60_000, false), item(85_514, true), item(4_914, false)];
        let ends = place_kp(
            &items,
            &[(0, items.len())],
            Width(150_000),
            &notes(),
            VerticalParams::default_v1(),
        )
        .expect("feasible");
        assert_eq!(ends, vec![1, 3]);
    }
}
