use crate::types::{OutlineTarget, ParsedPage, ProjectedLine};

use super::blocks::{Block, paragraph_from_accum};
use super::headings::{
    HEADING_MAX_TEXT_CHARS, MAX_HEADING_LEVELS, heading_level_for, is_caption_line, is_toc_title,
    looks_like_bold_heading, looks_like_numbered_bold_heading, outline_heading_level, page_is_toc,
    struct_heading_level,
};
use super::hr::detect_horizontal_rules;
use super::inline::{
    append_inline_continuation, line_uniform_style, render_line_inline, render_list_item_text,
};
use super::lists::{LIST_INDENT_STEP_PT, parse_list_marker};
use super::paragraphs::{
    ParaAccum, append_to_paragraph, collapse_whitespace, continues_heading, continues_paragraph,
};
use super::repetition::is_header_or_footer;
use super::tables::{detect_ruled_tables, detect_tables, merge_table_runs};

/// A document-order page interruption that breaks the normal text flow: either
/// a horizontal rule (from vector graphics) or a figure injection (a raster
/// image ref). Collected into one y-sorted stream so the two kinds interleave
/// correctly by vertical position rather than emitting all of one before the
/// other.
enum Interruption {
    Hr,
    Figure(crate::types::ImageRef),
}

/// Returns true if any span on the line is rotated more than ~5° off
/// horizontal — used to exclude sidebar / margin-stamp text (arXiv banners,
/// watermarks, vertical legends) from the body-size and heading-size
/// histograms so it doesn't compete with normal-flow text for heading slots.
pub(super) fn is_rotated_line(line: &ProjectedLine) -> bool {
    line.spans.iter().any(|s| {
        let r = s.rotation.abs() % 360.0;
        // Anything more than ~5° off the horizontal axes is "rotated" for
        // our purposes. 0° and 180° are both horizontal text.
        !(r < 5.0 || (175.0..=185.0).contains(&r) || (355.0..=360.0).contains(&r))
    })
}

/// Classify a single page's `ProjectedLine`s into blocks.
#[cfg(test)]
pub(super) fn classify_page(page: &ParsedPage, heading_map: &[(f32, u8)]) -> Vec<Block> {
    classify_page_with_filters(
        page,
        heading_map,
        &std::collections::HashSet::new(),
        &[],
        crate::config::ImageMode::Placeholder,
        &std::collections::HashSet::new(),
    )
}

/// Same as `classify_page` but also strips lines matching a precomputed
/// running header/footer set. Use this when emitting a whole document so
/// repeating chrome (titles, page numbers) doesn't show up in every page.
///
/// `outline` is the document outline filtered to entries whose `page_index`
/// matches this page (or the full outline — out-of-page entries are
/// ignored). Heading promotion from struct tree + outline outranks the
/// font-size heading map.
pub fn classify_page_with_filters(
    page: &ParsedPage,
    heading_map: &[(f32, u8)],
    header_footer: &std::collections::HashSet<String>,
    outline: &[OutlineTarget],
    image_mode: crate::config::ImageMode,
    chrome_indices: &std::collections::HashSet<usize>,
) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut paragraph: Option<ParaAccum> = None;
    // Active fenced code block being accumulated (consecutive `all_mono` lines).
    let mut code: Option<Vec<String>> = None;
    // Tracks the "level 0" indent of the current contiguous list run so we can
    // bucket deeper items into nesting levels. Reset whenever a non-list block
    // breaks the run.
    let mut list_base_indent: Option<f32> = None;
    // Index into `blocks` of the most recent ListItem in the current run — used
    // to merge wrapped continuation lines into the same item.
    let mut last_list_item_idx: Option<usize> = None;
    // Index (into `lines`) of the most recent line appended to the active list
    // item, for gap/font-size checks on continuation lines. An index rather than
    // a cloned line so the per-line hot loop doesn't deep-copy `spans` each time.
    let mut last_list_line: Option<usize> = None;
    // (level, last line) of a heading block currently being accumulated. Lets a
    // multi-line wrapped heading/caption (e.g. a photo caption set one size
    // above body) merge into one Heading block instead of shredding into one
    // `###` per wrapped line. Adjacency is enforced by checking `blocks.last()`,
    // so this never reaches across an intervening block.
    let mut heading_run: Option<(u8, usize)> = None;

    let flush_paragraph = |blocks: &mut Vec<Block>, p: Option<ParaAccum>| {
        if let Some(acc) = p
            && !acc.raw.trim().is_empty()
        {
            blocks.push(paragraph_from_accum(acc));
        }
    };
    let flush_code = |blocks: &mut Vec<Block>, c: Option<Vec<String>>| {
        if let Some(lines) = c
            && !lines.is_empty()
        {
            blocks.push(Block::CodeBlock { lines });
        }
    };

    let debug = std::env::var("LITEPARSE_DEBUG_MD").is_ok();

    // TOC suppression: when ≥3 lines on this page look like TOC entries
    // (alpha body + trailing page-number), demote heading promotion so each
    // entry stays a paragraph instead of becoming a fake H1/H2. The TOC's
    // own title ("Contents", "Table of Contents") is shorter without a
    // trailing number — it falls through the size/bold heuristics with no
    // special handling needed.
    let toc_page = page_is_toc(page);

    // Strip running header/footer lines up-front so they don't leak into
    // table detection (a repeating two-column footer would otherwise look
    // like a 2-row table) or paragraph grouping.
    let need_filter = !header_footer.is_empty() || !chrome_indices.is_empty();
    let filtered_owned: Vec<ProjectedLine> = if !need_filter {
        Vec::new()
    } else {
        page.projected_lines
            .iter()
            .enumerate()
            .filter(|(idx, l)| {
                !chrome_indices.contains(idx) && !is_header_or_footer(l, page, header_footer)
            })
            .map(|(_, l)| l.clone())
            .collect()
    };
    let lines: &[ProjectedLine] = if !need_filter {
        &page.projected_lines
    } else {
        &filtered_owned
    };

    // Pre-pass: detect tabular regions so the per-line classifier below can
    // skip over them. Tables take priority over heading / paragraph / list
    // classification because a row of bold short cells would otherwise be
    // misread as a heading or list item.
    //
    // Two detectors run in sequence: ruled-grid (path-based, strongest signal)
    // and the borderless column-alignment fallback. Where ranges overlap, the
    // ruled output wins.
    let ruled_runs = detect_ruled_tables(lines, &page.graphics, page.page_width, page.page_height);
    let borderless_runs = detect_tables(lines);
    let table_runs = merge_table_runs(ruled_runs, borderless_runs);

    // Suppress HRs that fall inside a detected table's y-range — they're the
    // table's own row dividers, not document-level section breaks. Build the
    // y-extents once before we move table_runs into the iterator.
    // Extend each table's HR-suppression band upward to cover any
    // header/sub-header rows we didn't absorb into the table. The expansion
    // is a few row heights — large enough to catch a 2–3 line bold/italic
    // header sitting just above the table, small enough not to swallow a
    // real section divider belonging to a different block. The downward
    // edge gets a small slack to catch HRs drawn flush with the last row.
    const TABLE_HR_SUPPRESS_HEADROOM_ROWS: f32 = 4.0;
    let table_y_extents: Vec<(f32, f32)> = table_runs
        .iter()
        .map(|run| {
            let top_line = &lines[run.start];
            let row_h = top_line.bbox.height.max(8.0);
            let top = top_line.bbox.y - row_h * TABLE_HR_SUPPRESS_HEADROOM_ROWS;
            let last = &lines[run.end.saturating_sub(1).max(run.start)];
            let bot = last.bbox.y + last.bbox.height;
            (top, bot)
        })
        .collect();

    let mut table_iter = table_runs.into_iter().peekable();

    // Pre-pass: collect document-order "interruptions" — horizontal rules
    // (from vector graphics) and figure injections (raster image refs when
    // image mode is on) — into one y-sorted stream. Emitting them from a single
    // stream keeps HRs and figures correctly interleaved by y; a per-type pass
    // would emit all HRs before all figures regardless of vertical position.
    // Both are suppressed inside table y-extents so a row divider or a
    // cell-spanning raster background doesn't spawn a block mid-table.
    let in_table_band = |y: f32| {
        table_y_extents
            .iter()
            .any(|(top, bot)| y >= *top - 2.0 && y <= *bot + 2.0)
    };
    let mut interruptions: Vec<(f32, Interruption)> = detect_horizontal_rules(page)
        .into_iter()
        .filter(|y| !in_table_band(*y))
        .map(|y| (y, Interruption::Hr))
        .collect();
    if !matches!(image_mode, crate::config::ImageMode::Off) {
        for r in &page.image_refs {
            if !in_table_band(r.bbox.y) {
                interruptions.push((r.bbox.y, Interruption::Figure(r.clone())));
            }
        }
    }
    // Stable sort by y. HRs were pushed before figures, so an exact-y tie keeps
    // the prior HR-before-figure ordering.
    interruptions.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut interruptions = interruptions.into_iter().peekable();

    // Emit any interruptions whose y is at or above `before_y`, flushing the
    // active paragraph/code/list state first so each lands as its own block.
    let emit_before = |blocks: &mut Vec<Block>,
                       paragraph: &mut Option<ParaAccum>,
                       code: &mut Option<Vec<String>>,
                       list_base: &mut Option<f32>,
                       last_item: &mut Option<usize>,
                       last_line: &mut Option<usize>,
                       iter: &mut std::iter::Peekable<std::vec::IntoIter<(f32, Interruption)>>,
                       before_y: f32| {
        while let Some((y, _)) = iter.peek() {
            if *y > before_y {
                break;
            }
            let (_, kind) = iter.next().unwrap();
            flush_paragraph(blocks, paragraph.take());
            flush_code(blocks, code.take());
            *list_base = None;
            *last_item = None;
            *last_line = None;
            blocks.push(match kind {
                Interruption::Hr => Block::HorizontalRule,
                Interruption::Figure(r) => Block::Figure {
                    id: r.id,
                    bbox: r.bbox,
                },
            });
        }
    };

    let mut idx = 0;
    while idx < lines.len() {
        if let Some(run) = table_iter.peek()
            && run.start == idx
        {
            // Flush any interruptions above this table's top edge first.
            let table_top = lines[run.start].bbox.y;
            emit_before(
                &mut blocks,
                &mut paragraph,
                &mut code,
                &mut list_base_indent,
                &mut last_list_item_idx,
                &mut last_list_line,
                &mut interruptions,
                table_top,
            );
            flush_paragraph(&mut blocks, paragraph.take());
            flush_code(&mut blocks, code.take());
            list_base_indent = None;
            last_list_item_idx = None;
            last_list_line = None;
            let run = table_iter.next().unwrap();
            blocks.push(run.block);
            idx = run.end;
            continue;
        }
        let line_idx = idx;
        let line = &lines[line_idx];
        // Emit any interruptions that fall above this line.
        emit_before(
            &mut blocks,
            &mut paragraph,
            &mut code,
            &mut list_base_indent,
            &mut last_list_item_idx,
            &mut last_list_line,
            &mut interruptions,
            line.bbox.y,
        );
        idx += 1;
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }
        // Skip rotated text (vertical sidebars, arXiv banners, watermarks).
        // Including it would either inject a paragraph of disconnected
        // characters or be misclassified as a heading.
        if is_rotated_line(line) {
            continue;
        }
        if debug {
            eprintln!(
                "[md] y={:.1} h={:.1} size={:.2} anchor={:?} indent={:.1} text={:?}",
                line.bbox.y,
                line.bbox.height,
                line.dominant_font_size,
                line.anchor,
                line.indent_x,
                text
            );
        }

        // Code block detection runs first so a mono heading-shaped line
        // (rare but plausible — e.g., a code identifier in a large mono font)
        // still becomes code. Mono content also wouldn't make a useful
        // heading.
        if line.all_mono {
            flush_paragraph(&mut blocks, paragraph.take());
            list_base_indent = None;
            last_list_item_idx = None;
            last_list_line = None;
            code.get_or_insert_with(Vec::new)
                .push(line.text.trim_end().to_string());
            continue;
        }
        // Any non-mono line ends the current code block (if any).
        flush_code(&mut blocks, code.take());

        // Priority chain: tagged-PDF struct tree → outline → font-size map.
        let tagged_level = struct_heading_level(line, &page.struct_nodes);
        let outline_level =
            tagged_level.or_else(|| outline_heading_level(line, page.page_height, outline, text));
        // Caption lines ("Figure 7", "Table 3.") are routinely set in a
        // distinct (and slightly larger) font that lands them in the
        // font-size heading map. Suppress font-size promotion for them;
        // outline / struct-tree signals still win since those are explicit.
        let toc_suppress = toc_page && !is_toc_title(text);
        let size_level = if is_caption_line(text) || toc_suppress {
            None
        } else {
            heading_level_for(line.dominant_font_size, heading_map)
        };
        // Guard against height-jitter false headings: a line that flows from
        // the previous line (same paragraph) AND starts lowercase is a
        // mid-paragraph continuation, not a heading — even if its
        // height-estimated size jittered into a heading slot. A real heading
        // has a break above it and is capitalized, so it won't satisfy both.
        // Outline / struct-tree levels are explicit and bypass this guard.
        let size_level = size_level.filter(|_| {
            let starts_lower = text.chars().next().is_some_and(|c| c.is_lowercase());
            let prev = paragraph
                .as_ref()
                .map(|p| &p.last)
                .or(last_list_line.map(|i| &lines[i]));
            !(starts_lower && prev.is_some_and(|p| continues_paragraph(p, line)))
        });
        // Long-text guard: a real heading is a label, not a sentence with
        // multiple clauses. Footnotes / citations / captions that happen to
        // be set slightly larger than body text (e.g. a 10.7pt citation
        // over a 9pt body) score as the smallest heading level in the size
        // map; without a length cap they emit as `##`-shaped sentences.
        // Threshold is generous — book titles and long subsection labels
        // can run 100+ chars — but a 200-char citation is unambiguously
        // not a heading.
        let size_level = size_level.filter(|_| text.chars().count() <= HEADING_MAX_TEXT_CHARS);
        // A standalone "Contents" / "Table of Contents" / "Index" line is
        // almost always a real H1, even when `page_is_toc` is false — many
        // TOCs list entries without inline trailing page numbers, so the
        // page-level detector misses them. `is_toc_title` matches the *entire*
        // trimmed line against an exact whitelist, so it can't fire mid-prose.
        let toc_title_level = if is_toc_title(text) { Some(1u8) } else { None };
        let level = outline_level
            .or(size_level)
            .or(toc_title_level)
            .map(|l| l.clamp(1, MAX_HEADING_LEVELS as u8));
        let mut demoted_heading = false;
        if let Some(level) = level {
            // Merge a wrapped continuation into the heading directly above when
            // it flows as one block: same level, the heading is still the last
            // emitted block, and the line continues the paragraph. A real
            // section heading is followed by body text (which breaks the run),
            // so only a genuinely multi-line heading/caption merges here.
            if let Some((run_level, run_idx)) = heading_run.as_ref()
                && *run_level == level
                && continues_heading(&lines[*run_idx], line)
                && let Some(Block::Heading {
                    level: last_level,
                    text: htext,
                }) = blocks.last_mut()
                && *last_level == level
            {
                // Length guard: a wrapped multi-line heading is fine, but a
                // run that grows past `HEADING_MAX_TEXT_CHARS` is almost
                // certainly a footnote/citation block whose font is only
                // slightly above body (e.g. 10.7pt over 9pt body). Demote
                // the existing heading block back to a paragraph and let
                // the current line continue that paragraph below. The
                // first line scored as a solo heading because it was under
                // the limit on its own; only the second-or-later line tips
                // the cumulative content into footnote territory.
                let combined_chars = htext.chars().count() + 1 + text.chars().count();
                if combined_chars > HEADING_MAX_TEXT_CHARS {
                    let demoted = std::mem::take(htext);
                    blocks.pop();
                    paragraph = Some(ParaAccum {
                        raw: demoted.clone(),
                        inline: demoted,
                        last: lines[*run_idx].clone(),
                        uniform: None,
                    });
                    heading_run = None;
                    demoted_heading = true;
                } else {
                    append_inline_continuation(htext, text, &collapse_whitespace(text));
                    heading_run = Some((level, line_idx));
                    continue;
                }
            }
            if !demoted_heading {
                flush_paragraph(&mut blocks, paragraph.take());
                list_base_indent = None;
                last_list_item_idx = None;
                last_list_line = None;
                blocks.push(Block::Heading {
                    level,
                    text: collapse_whitespace(text),
                });
                heading_run = Some((level, line_idx));
                continue;
            }
        }

        // List item?
        if let Some((ordered, marker, rest)) = parse_list_marker(text) {
            // Numbered bold-section heading: "1. **Foo**" / "5. **The dynamics**".
            // When the post-marker body is uniformly bold + body-sized,
            // standalone (paragraph-break gap above and below), short, and
            // mostly alpha, treat it as a heading rather than the first item
            // of an ordered list. Without this, decimal-numbered section
            // headings in technical/legal/scientific PDFs silently emit as
            // ordered list items and lose all heading structure.
            if ordered
                && !toc_suppress
                && looks_like_numbered_bold_heading(
                    line,
                    rest,
                    paragraph
                        .as_ref()
                        .map(|p| &p.last)
                        .or(last_list_line.map(|i| &lines[i])),
                )
            {
                flush_paragraph(&mut blocks, paragraph.take());
                list_base_indent = None;
                last_list_item_idx = None;
                last_list_line = None;
                let level = (heading_map.len() as u8 + 1).clamp(1, MAX_HEADING_LEVELS as u8);
                blocks.push(Block::Heading {
                    level,
                    text: collapse_whitespace(text),
                });
                continue;
            }
            flush_paragraph(&mut blocks, paragraph.take());
            let base = *list_base_indent.get_or_insert(line.indent_x);
            let level = (((line.indent_x - base) / LIST_INDENT_STEP_PT)
                .round()
                .max(0.0)) as u8;
            last_list_item_idx = Some(blocks.len());
            last_list_line = Some(line_idx);
            // Render the list-item text via the inline pipeline so per-span
            // emphasis surfaces. We strip the marker from `rest` (already
            // done by `parse_list_marker`), but emphasis lives on `line.spans`,
            // which still contain the marker span — render the line and then
            // peel the marker off the front of the rendered string.
            let item_text = render_list_item_text(line, &marker, rest);
            blocks.push(Block::ListItem {
                ordered,
                marker,
                level,
                text: item_text,
                bold: false,
                italic: false,
            });
            continue;
        }

        // Continuation of a list item: same gap/font rules as paragraphs.
        // Footnote-style continuations often left-flush below the marker's
        // hanging indent, so we don't require indent ≥ marker indent.
        if let Some(item_idx) = last_list_item_idx
            && let Some(prev_idx) = last_list_line
            && continues_paragraph(&lines[prev_idx], line)
            && let Some(Block::ListItem {
                text: prev_text, ..
            }) = blocks.get_mut(item_idx)
        {
            // De-hyphenate against the prior rendered text, then append the
            // inline-styled continuation.
            let cont_inline = render_line_inline(line);
            append_inline_continuation(prev_text, text, &cont_inline);
            last_list_line = Some(line_idx);
            continue;
        }

        // Bold body-size heading. Section headings in academic / technical
        // PDFs are routinely body-sized + bold (e.g. "Abstract",
        // "1 Introduction"); without this rule they look just like a bold
        // first sentence of a paragraph. Runs after list-marker detection so
        // bold bullet items stay as list items.
        let prev_for_gap = paragraph
            .as_ref()
            .map(|p| &p.last)
            .or(last_list_line.map(|i| &lines[i]));
        let next_for_gap = lines.get(idx);
        if !toc_suppress && looks_like_bold_heading(line, prev_for_gap, next_for_gap) {
            flush_paragraph(&mut blocks, paragraph.take());
            list_base_indent = None;
            last_list_item_idx = None;
            last_list_line = None;
            // Level: one deeper than the deepest size-based level we already
            // have. With an empty heading_map this lands on H1; with a full
            // 6-level map it caps at H6.
            let level = (heading_map.len() as u8 + 1).clamp(1, MAX_HEADING_LEVELS as u8);
            blocks.push(Block::Heading {
                level,
                text: collapse_whitespace(text),
            });
            continue;
        }

        match paragraph.as_mut() {
            Some(acc) if continues_paragraph(&acc.last, line) => {
                append_to_paragraph(acc, line);
            }
            _ => {
                flush_paragraph(&mut blocks, paragraph.take());
                list_base_indent = None;
                last_list_item_idx = None;
                last_list_line = None;
                let inline = render_line_inline(line);
                let raw = collapse_whitespace(text);
                let uniform = line_uniform_style(line).map(|s| (s.bold, s.italic));
                paragraph = Some(ParaAccum {
                    raw,
                    inline,
                    last: line.clone(),
                    uniform,
                });
            }
        }
    }

    flush_paragraph(&mut blocks, paragraph.take());
    flush_code(&mut blocks, code.take());
    // Flush any trailing interruptions that sat below the last text line.
    emit_before(
        &mut blocks,
        &mut paragraph,
        &mut code,
        &mut list_base_indent,
        &mut last_list_item_idx,
        &mut last_list_line,
        &mut interruptions,
        f32::INFINITY,
    );
    blocks
}

#[cfg(test)]
mod tests {
    use super::super::blocks::{Block, render_blocks};
    use super::super::headings::{build_heading_map, compute_body_size};
    use super::super::repetition::compute_header_footer_set;
    use super::super::test_helpers::{
        header_footer_page, line, mono_line, page, page_with_graphics, stroke, styled_line,
    };
    use super::*;
    use crate::types::TextItem;

    #[test]
    fn classify_emits_heading_and_paragraph() {
        let p = page(vec![
            line("Title of the document goes here", 50.0, 50.0, 18.0, 18.0),
            line("First sentence of the para-", 50.0, 80.0, 10.0, 10.0),
            line("graph continues here.", 50.0, 92.0, 10.0, 10.0),
            line("Another paragraph.", 50.0, 130.0, 10.0, 10.0),
        ]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 3);
        match &blocks[0] {
            Block::Heading { level, text } => {
                assert_eq!(*level, 1);
                assert_eq!(text, "Title of the document goes here");
            }
            other => panic!("expected heading, got {other:?}"),
        }
        match &blocks[1] {
            Block::Paragraph { text: t, .. } => {
                assert!(t.contains("paragraph continues"), "got: {t}");
                assert!(!t.contains("para-"), "de-hyphenation failed: {t}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
        match &blocks[2] {
            Block::Paragraph { text: t, .. } => assert_eq!(t, "Another paragraph."),
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn paragraph_break_on_big_gap() {
        let p = page(vec![
            line("Line A.", 50.0, 80.0, 10.0, 10.0),
            line("Line B.", 50.0, 200.0, 10.0, 10.0),
        ]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn classify_emits_list_items() {
        let p = page(vec![
            line("Intro line.", 50.0, 50.0, 10.0, 10.0),
            line("• first bullet", 60.0, 80.0, 10.0, 10.0),
            line("• second bullet", 60.0, 92.0, 10.0, 10.0),
            line("◦ nested item", 72.0, 104.0, 10.0, 10.0),
            line("• back to top", 60.0, 116.0, 10.0, 10.0),
        ]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        let list_items: Vec<&Block> = blocks
            .iter()
            .filter(|b| matches!(b, Block::ListItem { .. }))
            .collect();
        assert_eq!(list_items.len(), 4);
        if let Block::ListItem { level, text, .. } = list_items[0] {
            assert_eq!(*level, 0);
            assert_eq!(text, "first bullet");
        } else {
            panic!();
        }
        // The "- nested item" line is indented +12pt from the base bullet.
        if let Block::ListItem { level, .. } = list_items[2] {
            assert_eq!(*level, 1);
        } else {
            panic!();
        }
    }

    #[test]
    fn classify_emits_code_block() {
        let p = page(vec![
            line("Intro line.", 50.0, 50.0, 10.0, 10.0),
            mono_line("    let x = 1;", 80.0),
            mono_line("    let y = x + 2;", 92.0),
            line("After the code.", 50.0, 120.0, 10.0, 10.0),
        ]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        // Expect: Paragraph("Intro line."), CodeBlock(2 lines), Paragraph("After...")
        assert_eq!(blocks.len(), 3);
        match &blocks[1] {
            Block::CodeBlock { lines } => {
                assert_eq!(lines.len(), 2);
                assert!(lines[0].contains("let x = 1;"));
                assert!(lines[1].contains("let y = x + 2;"));
            }
            other => panic!("expected code block, got {other:?}"),
        }
        let s = render_blocks(&blocks);
        assert!(s.contains("```\n    let x = 1;"));
        assert!(s.ends_with("After the code."));
    }

    #[test]
    fn classify_marks_paragraph_bold_when_all_lines_bold() {
        let mut a = line("Bold line one.", 50.0, 50.0, 10.0, 10.0);
        let mut b = line("bold continuation.", 50.0, 62.0, 10.0, 10.0);
        // Mark the underlying spans as bold so per-span style detection sees
        // it — the new inline pipeline reads from `spans`, not the per-line
        // `all_bold` shortcut flag.
        let bold_span = TextItem {
            text: "x".into(),
            font_name: Some("Arial-Bold".into()),
            ..Default::default()
        };
        a.spans = vec![bold_span.clone()];
        b.spans = vec![bold_span];
        a.all_bold = true;
        b.all_bold = true;
        let p = page(vec![a, b]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph { bold, italic, .. } => {
                assert!(*bold);
                assert!(!*italic);
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
        let s = render_blocks(&blocks);
        assert!(s.starts_with("**") && s.ends_with("**"), "got: {s}");
    }

    #[test]
    fn detects_simple_borderless_table() {
        use super::super::test_helpers::line_with_spans;
        let lines = vec![
            line_with_spans(
                &[("Name", 50.0), ("Age", 150.0), ("City", 250.0)],
                100.0,
                10.0,
            ),
            line_with_spans(
                &[("Alice", 50.0), ("30", 150.0), ("NYC", 250.0)],
                115.0,
                10.0,
            ),
            line_with_spans(&[("Bob", 50.0), ("25", 150.0), ("LA", 250.0)], 130.0, 10.0),
        ];
        let p = page(lines);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 1, "got: {blocks:?}");
        match &blocks[0] {
            Block::Table { header, rows } => {
                // Header isn't bold so no header row promoted.
                assert!(header.is_none());
                assert_eq!(rows.len(), 3);
                assert_eq!(rows[0][0], "Name");
                assert_eq!(rows[1][2], "NYC");
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn full_format_strips_header_footer() {
        let pages = vec![
            header_footer_page(1, "Acme Confidential", "Page 1 of 2", "First page body."),
            header_footer_page(2, "Acme Confidential", "Page 2 of 2", "Second page body."),
        ];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let set = compute_header_footer_set(&pages);
        let blocks = classify_page_with_filters(
            &pages[0],
            &map,
            &set,
            &[],
            crate::config::ImageMode::Placeholder,
            &std::collections::HashSet::new(),
        );
        let s = render_blocks(&blocks);
        assert!(!s.contains("Acme Confidential"), "got: {s}");
        assert!(!s.contains("Page 1 of 2"), "got: {s}");
        assert!(s.contains("First page body."));
    }

    #[test]
    fn classify_paragraph_with_mid_line_bold() {
        // First line has a bold word mid-line → not uniformly styled; paragraph
        // should emit baked-in `**bold**` inside the text and `bold=false` at
        // the block level.
        let a = styled_line(
            &[
                ("a sentence with a", 50.0, Some("Arial")),
                ("bold", 180.0, Some("Arial-Bold")),
                ("word in it.", 230.0, Some("Arial")),
            ],
            50.0,
            10.0,
        );
        let p = page(vec![a]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 1, "got: {blocks:?}");
        match &blocks[0] {
            Block::Paragraph { text, bold, italic } => {
                assert!(!*bold, "mixed-style paragraph shouldn't set block bold");
                assert!(!*italic);
                assert!(text.contains("**bold**"), "got: {text}");
            }
            other => panic!("expected paragraph, got {other:?}"),
        }
    }

    #[test]
    fn classify_list_item_strips_marker_under_emphasis() {
        // Whole bullet line is bold (marker + text). Rendered text should be
        // wrapped, with the marker dropped (the renderer prints it).
        let l = styled_line(
            &[
                ("•", 60.0, Some("Arial-Bold")),
                ("important item", 80.0, Some("Arial-Bold")),
            ],
            50.0,
            10.0,
        );
        let p = page(vec![l]);
        let pages = vec![p];
        let body = compute_body_size(&pages);
        let map = build_heading_map(&pages, body);
        let blocks = classify_page(&pages[0], &map);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::ListItem { text, .. } => {
                assert_eq!(text, "**important item**");
            }
            other => panic!("expected list item, got {other:?}"),
        }
    }

    #[test]
    fn hr_emitted_between_lines_by_y_order() {
        let lines = vec![
            line("before the rule", 50.0, 100.0, 10.0, 10.0),
            line("after the rule", 50.0, 300.0, 10.0, 10.0),
        ];
        // Stroke between the two lines, far from either's baseline.
        let p = page_with_graphics(lines, vec![stroke(50.0, 200.0, 450.0, 200.0, 0.5)]);
        let blocks = classify_page(&p, &[]);
        let has_hr = blocks
            .iter()
            .position(|b| matches!(b, Block::HorizontalRule));
        assert!(has_hr.is_some(), "expected an HR block, got {blocks:?}");
        // HR must land between the two paragraphs, not before/after both.
        let pos = has_hr.unwrap();
        assert!(pos > 0 && pos < blocks.len() - 1);
    }
}
