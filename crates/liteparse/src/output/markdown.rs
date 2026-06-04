use crate::config::ImageMode;
use crate::markdown_layout::{
    build_heading_map, classify_page_with_filters, compute_body_size, compute_header_footer_set,
    detect_single_page_chrome, render_blocks,
};
use crate::types::{OutlineTarget, ParsedPage};

/// Format parsed pages as markdown.
///
/// Current coverage (build-order step 3): char-weighted font-size histogram →
/// heading detection, paragraph grouping with de-hyphenation. Lines that don't
/// classify as a heading are joined into paragraphs. Lists, code blocks,
/// tables, and inline styling are still pending.
///
/// Pages are emitted in order, separated by `\n\n-----\n\n` with a
/// `<!-- page N -->` marker. Pages that contain no projected lines (e.g. blank
/// or fully-OCR pages without font-size info) fall back to the projected text
/// wrapped in a fenced block so we never silently drop content.
pub fn format_markdown(
    pages: &[ParsedPage],
    outline: &[OutlineTarget],
    image_mode: ImageMode,
) -> String {
    if pages.is_empty() {
        return String::new();
    }

    let body_size = compute_body_size(pages);
    let heading_map = build_heading_map(pages, body_size);
    let header_footer = compute_header_footer_set(pages);

    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n-----\n\n");
        }
        out.push_str(&format!("<!-- page {} -->\n\n", page.page_number));

        if page.projected_lines.is_empty() {
            // No structural metadata for this page — fall back to the
            // projection text inside a fence so nothing is dropped.
            out.push_str("```text\n");
            out.push_str(&page.text);
            if !page.text.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```");
            continue;
        }

        // Filter outline entries to this page so the classifier's y/title
        // match is a O(entries_on_page) scan per line, not O(whole doc).
        let target_index = (page.page_number as i32).saturating_sub(1);
        let page_outline: Vec<OutlineTarget> = outline
            .iter()
            .filter(|e| e.page_index == target_index)
            .cloned()
            .collect();
        let chrome_indices = detect_single_page_chrome(page, body_size);
        let blocks = classify_page_with_filters(
            page,
            &heading_map,
            &header_footer,
            &page_outline,
            image_mode,
            &chrome_indices,
        );
        out.push_str(&render_blocks(&blocks));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Anchor, ProjectedLine, Rect, TextItem};

    fn line(text: &str, x: f32, y: f32, h: f32, size: f32) -> ProjectedLine {
        ProjectedLine {
            text: text.into(),
            bbox: Rect {
                x,
                y,
                width: text.chars().count() as f32 * (size * 0.5),
                height: h,
            },
            anchor: Anchor::Left,
            indent_x: x,
            dominant_font_size: size,
            font_size_is_estimated: false,
            dominant_font_name: Some("Arial".into()),
            all_bold: false,
            all_italic: false,
            all_mono: false,
            all_strike: false,
            spans: vec![TextItem::default()],
            region_path: Vec::new(),
            mcid: None,
        }
    }

    fn page_with(n: usize, lines: Vec<ProjectedLine>) -> ParsedPage {
        ParsedPage {
            page_number: n,
            page_width: 612.0,
            page_height: 792.0,
            text: "fallback".into(),
            text_items: vec![],
            projected_lines: lines,
            regions: crate::types::Region::default(),
            graphics: vec![],
            figures: vec![],
            struct_nodes: vec![],
            image_refs: vec![],
        }
    }

    #[test]
    fn test_empty() {
        assert_eq!(format_markdown(&[], &[], ImageMode::Placeholder), "");
    }

    #[test]
    fn test_fallback_when_no_projected_lines() {
        let p = ParsedPage {
            page_number: 1,
            page_width: 0.0,
            page_height: 0.0,
            text: "hello".into(),
            text_items: vec![],
            projected_lines: vec![],
            regions: crate::types::Region::default(),
            graphics: vec![],
            figures: vec![],
            struct_nodes: vec![],
            image_refs: vec![],
        };
        let out = format_markdown(&[p], &[], ImageMode::Placeholder);
        assert!(out.contains("```text"));
        assert!(out.contains("hello"));
        assert!(out.contains("<!-- page 1 -->"));
    }

    #[test]
    fn test_heading_and_paragraph() {
        let p = page_with(
            1,
            vec![
                line("My Title For This Test Document", 50.0, 50.0, 18.0, 18.0),
                // Enough body text to dominate the char-weighted body-size
                // mode so the title at 18pt registers as larger-than-body.
                line("First sentence of body prose here.", 50.0, 80.0, 10.0, 10.0),
                line(
                    "Second sentence of body prose here.",
                    50.0,
                    92.0,
                    10.0,
                    10.0,
                ),
                line(
                    "Third sentence of body prose here.",
                    50.0,
                    104.0,
                    10.0,
                    10.0,
                ),
            ],
        );
        let out = format_markdown(&[p], &[], ImageMode::Placeholder);
        assert!(out.contains("# My Title For This Test Document"));
        assert!(out.contains("First sentence of body prose here."));
    }

    #[test]
    fn test_multi_page_separator() {
        let a = page_with(1, vec![line("A page.", 50.0, 80.0, 10.0, 10.0)]);
        let b = page_with(2, vec![line("B page.", 50.0, 80.0, 10.0, 10.0)]);
        let out = format_markdown(&[a, b], &[], ImageMode::Placeholder);
        assert!(out.contains("-----"));
        assert!(out.find("A page.").unwrap() < out.find("B page.").unwrap());
    }
}
