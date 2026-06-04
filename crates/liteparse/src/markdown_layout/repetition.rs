use crate::types::{ParsedPage, ProjectedLine};

use super::paragraphs::collapse_whitespace;

/// Fraction of page height treated as the "top band" for header detection.
/// Most running headers sit within the top 8–12% of a page; 12% gives some
/// slack for two-line headers without sweeping in body text.
pub(super) const HEADER_BAND_FRACTION: f32 = 0.12;

/// Fraction of page height treated as the "bottom band" for footer detection.
pub(super) const FOOTER_BAND_FRACTION: f32 = 0.12;

/// Fraction of pages on which a normalized line must appear (in the same
/// band) to be classified as a running header/footer.
const HEADER_FOOTER_MIN_FRACTION: f32 = 0.5;

/// Absolute floor on header/footer matches — single-page docs can't have a
/// "running" header by definition, and a single match on a 2-page doc is too
/// weak to act on.
const HEADER_FOOTER_MIN_PAGES: usize = 2;

/// Normalize a line for cross-page header/footer matching. Lowercases,
/// collapses whitespace, and replaces every run of ASCII digits with `#` so
/// `Page 1 of 6` and `Page 2 of 6` collapse to the same key.
pub(super) fn normalize_for_repetition(s: &str) -> String {
    let collapsed = collapse_whitespace(s).to_lowercase();
    let mut out = String::with_capacity(collapsed.len());
    let mut in_digits = false;
    for c in collapsed.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

/// Cross-page repetition detector. Returns the set of normalized strings that
/// appear in the top or bottom band of ≥ `HEADER_FOOTER_MIN_FRACTION` of
/// pages (capped below by `HEADER_FOOTER_MIN_PAGES`). The caller uses this to
/// filter `ProjectedLine`s before classification.
///
/// "Same band" means a line whose top is within `HEADER_BAND_FRACTION` of the
/// page top (header) or whose bottom is within `FOOTER_BAND_FRACTION` of the
/// page bottom (footer). Header and footer bands are tracked separately so a
/// company name that appears as both a header and a body-section title on
/// different pages isn't stripped from the body.
pub fn compute_header_footer_set(pages: &[ParsedPage]) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut set: HashSet<String> = HashSet::new();
    if pages.len() < HEADER_FOOTER_MIN_PAGES {
        return set;
    }
    // Two counters keyed by `(band, normalized_text)` — band is `'h'` or `'f'`.
    let mut counts: HashMap<(char, String), HashSet<usize>> = HashMap::new();
    for page in pages {
        let header_cutoff = page.page_height * HEADER_BAND_FRACTION;
        let footer_cutoff = page.page_height * (1.0 - FOOTER_BAND_FRACTION);
        for line in &page.projected_lines {
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            let norm = normalize_for_repetition(text);
            if norm.is_empty() {
                continue;
            }
            // Header band: top of line within the top band.
            if line.bbox.y <= header_cutoff {
                counts
                    .entry(('h', norm.clone()))
                    .or_default()
                    .insert(page.page_number);
            }
            // Footer band: bottom of line at or below the footer cutoff.
            let line_bottom = line.bbox.y + line.bbox.height;
            if line_bottom >= footer_cutoff {
                counts
                    .entry(('f', norm))
                    .or_default()
                    .insert(page.page_number);
            }
        }
    }
    let threshold = (pages.len() as f32 * HEADER_FOOTER_MIN_FRACTION)
        .ceil()
        .max(HEADER_FOOTER_MIN_PAGES as f32) as usize;
    for ((_, norm), pages_seen) in counts {
        if pages_seen.len() >= threshold {
            set.insert(norm);
        }
    }
    set
}

/// Fraction of page height treated as the top band for the single-page
/// chrome detector (slightly wider than the cross-page band — single-page
/// chrome can sit a bit deeper, e.g. a citation block above a paper title).
const SP_TOP_BAND_FRACTION: f32 = 0.15;
/// Fraction of page height for the bottom band of the single-page detector.
const SP_BOTTOM_BAND_FRACTION: f32 = 0.15;
/// Minimum gap (in multiples of body line height) between candidate chrome
/// and the rest of the page content. A real header/footer is visually
/// separated; body lines that happen to sit near the top/bottom are not.
const SP_ISOLATION_GAP_RATIO: f32 = 1.0;
/// Font-size delta threshold: lines whose size differs from body by this
/// fraction (smaller OR larger) are size-suspicious chrome candidates. The
/// 0.15 floor admits typical 8/9pt chrome on 10pt body and 12+pt banner
/// chrome on 10pt body without sweeping in sub-headings (which usually
/// differ by ≥ 25%).
const SP_FONT_DELTA: f32 = 0.15;

/// Heuristic check: does the line text match a known running-chrome
/// signature? URLs/DOIs, journal-article preamble ("Please cite this
/// article…"), `Page N of M`, copyright marks, volume/issue markers, and
/// journal citation lines (year + page range + ≤ ~120 chars).
///
/// Returns true only for unambiguous chrome patterns — false-positive
/// recall on body prose is the main risk to guard.
pub(super) fn matches_chrome_pattern(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();

    // URL / DOI prefixes — chrome lines are very often a citation URL.
    if lower.contains("http://")
        || lower.contains("https://")
        || lower.starts_with("www.")
        || lower.contains(" www.")
        || lower.contains("doi:")
        || lower.contains("doi.org/")
        || lower.contains("dx.doi.org")
    {
        return true;
    }

    // Common journal-paper top-banner phrases.
    if lower.contains("please cite this article")
        || lower.contains("contents lists available at")
        || lower.contains("available online at")
        || lower.contains("downloaded from")
    {
        return true;
    }

    // Copyright / trademark chrome.
    if t.contains('©') || lower.contains("copyright ") || lower.contains("all rights reserved")
    {
        return true;
    }

    // "Page N", "Page N of M", standalone page numbers.
    if let Some(rest) = lower.strip_prefix("page ") {
        let head: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !head.is_empty() {
            return true;
        }
    }
    // Lone page number (≤4 digits, possibly with a leading "p." or similar).
    if t.chars().all(|c| c.is_ascii_digit()) && t.len() <= 4 {
        return true;
    }

    // Volume / Issue markers ("Vol. 24" / "Vol 24" / "No. 3").
    let lb = lower.as_bytes();
    for i in 0..lb.len().saturating_sub(4) {
        // ASCII-only check — safe to byte-index because the prefix we look
        // for ("vol") is pure ASCII and we test bytes one at a time.
        let starts_word = i == 0 || !lb[i - 1].is_ascii_alphanumeric();
        if !starts_word {
            continue;
        }
        if lb[i] == b'v' && lb[i + 1] == b'o' && lb[i + 2] == b'l' {
            let sep = lb[i + 3];
            if sep == b'.' || sep == b' ' || sep == b',' {
                // any digit later in the line is enough
                if lb[i + 4..].iter().any(|b| b.is_ascii_digit()) {
                    return true;
                }
            }
        }
    }
    // Journal-cite-style: contains a 4-digit year (19xx/20xx) AND a numeric
    // page range (digits[-–]digits) AND short enough to plausibly be chrome.
    if t.len() <= 120 && has_year(&lower) && has_digit_range(t) {
        return true;
    }

    false
}

fn has_year(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        let starts_word = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !starts_word {
            continue;
        }
        if (bytes[i] == b'1' && bytes[i + 1] == b'9')
            || (bytes[i] == b'2' && bytes[i + 1] == b'0')
        {
            if bytes[i + 2].is_ascii_digit() && bytes[i + 3].is_ascii_digit() {
                let ends_word = i + 4 >= bytes.len() || !bytes[i + 4].is_ascii_alphanumeric();
                if ends_word {
                    return true;
                }
            }
        }
    }
    false
}

fn has_digit_range(t: &str) -> bool {
    // Look for digit, optional spaces, '-' or '–', optional spaces, digit.
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            // Skip optional spaces.
            let mut k = j;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && (chars[k] == '-' || chars[k] == '–' || chars[k] == '—') {
                let mut m = k + 1;
                while m < chars.len() && chars[m] == ' ' {
                    m += 1;
                }
                if m < chars.len() && chars[m].is_ascii_digit() {
                    return true;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// Detect running header/footer chrome on a single page using position +
/// isolation + (pattern hint OR font-size delta from body). Returns the set
/// of `projected_lines` indices to strip before classification.
///
/// Designed to complement `compute_header_footer_set`: the cross-page
/// detector needs ≥2 pages and matching repetition; this one fires on
/// single-page docs or on multi-page docs where chrome doesn't repeat
/// consistently (e.g. per-page citation tags). Conservative by design —
/// requires a strong signature (pattern OR clear size delta) plus an
/// isolation gap, so real titles and section headings are preserved.
pub fn detect_single_page_chrome(
    page: &ParsedPage,
    body_size: f32,
) -> std::collections::HashSet<usize> {
    use std::collections::HashSet;
    let mut out: HashSet<usize> = HashSet::new();
    if page.projected_lines.is_empty() {
        return out;
    }
    let h = page.page_height;
    if h <= 0.0 {
        return out;
    }
    let top_cutoff = h * SP_TOP_BAND_FRACTION;
    let bottom_cutoff = h * (1.0 - SP_BOTTOM_BAND_FRACTION);
    // Use body size as the gap reference when known; otherwise fall back to
    // the line's own height.
    let body_gap_ref = if body_size > 0.0 { body_size } else { 0.0 };

    // Cache line top/bottom so isolation checks are straightforward.
    let tops: Vec<f32> = page.projected_lines.iter().map(|l| l.bbox.y).collect();
    let bots: Vec<f32> = page
        .projected_lines
        .iter()
        .map(|l| l.bbox.y + l.bbox.height)
        .collect();

    for (idx, line) in page.projected_lines.iter().enumerate() {
        let text = line.text.trim();
        if text.is_empty() {
            continue;
        }
        let in_top = bots[idx] <= top_cutoff;
        let in_bottom = tops[idx] >= bottom_cutoff;
        if !(in_top || in_bottom) {
            continue;
        }

        // Isolation: gap to the nearest non-band neighbor must be ≥ the
        // configured ratio of body line height. For a top header we look
        // *down*; for a bottom footer we look *up*.
        let gap_ref = if body_gap_ref > 0.0 {
            body_gap_ref
        } else {
            line.bbox.height.max(1.0)
        };
        let required_gap = SP_ISOLATION_GAP_RATIO * gap_ref;
        // Isolation: gap to the nearest neighbor line (by y) on the body
        // side must be ≥ required_gap. Multi-line chrome counts as one
        // unit — we check the gap from this group's outer edge to the
        // nearest *non-chrome-candidate* line. As a simple proxy, look
        // at the nearest line in y-order whose pattern doesn't match.
        let isolated = if in_top {
            page.projected_lines
                .iter()
                .enumerate()
                .filter(|(j, l)| {
                    *j != idx
                        && tops[*j] > bots[idx]
                        && !matches_chrome_pattern(l.text.trim())
                })
                .map(|(j, _)| tops[j] - bots[idx])
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .map(|gap| gap >= required_gap)
                .unwrap_or(true)
        } else {
            page.projected_lines
                .iter()
                .enumerate()
                .filter(|(j, l)| {
                    *j != idx
                        && bots[*j] < tops[idx]
                        && !matches_chrome_pattern(l.text.trim())
                })
                .map(|(j, _)| tops[idx] - bots[j])
                .min_by(|a, b| a.partial_cmp(b).unwrap())
                .map(|gap| gap >= required_gap)
                .unwrap_or(true)
        };
        if !isolated {
            continue;
        }

        // Require a chrome pattern signature — drops the false-positive risk
        // of nuking real titles/headings that happen to sit in the band.
        // Font-size delta alone is too coarse: section titles routinely
        // differ from body by ≥15% but are not chrome.
        if !matches_chrome_pattern(text) {
            continue;
        }
        // SP_FONT_DELTA reserved for a future size-aware pass; keep the
        // constant so the threshold stays documented.
        let _ = SP_FONT_DELTA;

        // Length guard: avoid stripping long paragraphs that happen to sit in
        // the band and start with a URL — chrome lines are short. 200 char
        // ceiling accommodates the 'Please cite' preamble (often 100-150
        // chars) without admitting body paragraphs.
        if text.chars().count() > 200 {
            continue;
        }

        out.insert(idx);
    }

    out
}

/// Returns true if `line` (located on `page`) matches the running
/// header/footer set: the line sits in the top or bottom band AND its
/// normalized text is in `header_footer`.
pub(super) fn is_header_or_footer(
    line: &ProjectedLine,
    page: &ParsedPage,
    header_footer: &std::collections::HashSet<String>,
) -> bool {
    if header_footer.is_empty() {
        return false;
    }
    let header_cutoff = page.page_height * HEADER_BAND_FRACTION;
    let footer_cutoff = page.page_height * (1.0 - FOOTER_BAND_FRACTION);
    let in_band = line.bbox.y <= header_cutoff || line.bbox.y + line.bbox.height >= footer_cutoff;
    if !in_band {
        return false;
    }
    let norm = normalize_for_repetition(line.text.trim());
    header_footer.contains(&norm)
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::header_footer_page;
    use super::*;

    #[test]
    fn normalize_collapses_digits_and_case() {
        assert_eq!(normalize_for_repetition("Page 1 of 6"), "page # of #");
        assert_eq!(normalize_for_repetition("PAGE 12 OF 6"), "page # of #");
        assert_eq!(normalize_for_repetition("Confidential"), "confidential");
    }

    #[test]
    fn detects_repeating_header_and_footer() {
        let pages = vec![
            header_footer_page(1, "Acme Confidential", "Page 1 of 3", "Body one."),
            header_footer_page(2, "Acme Confidential", "Page 2 of 3", "Body two."),
            header_footer_page(3, "Acme Confidential", "Page 3 of 3", "Body three."),
        ];
        let set = compute_header_footer_set(&pages);
        assert!(set.contains("acme confidential"));
        assert!(set.contains("page # of #"));
    }

    #[test]
    fn skips_repetition_check_on_single_page() {
        let pages = vec![header_footer_page(1, "Solo", "footer", "body")];
        let set = compute_header_footer_set(&pages);
        assert!(set.is_empty());
    }

    #[test]
    fn body_text_not_classified_as_header() {
        // Same text in the body of every page should NOT be stripped — only
        // text within the top/bottom band qualifies.
        let mut pages = Vec::new();
        for n in 1..=3 {
            let mut p = header_footer_page(n, "unique header", "unique footer", "shared body text");
            // Move "shared body text" out of header/footer bands (already at y=50 in mid-page).
            // No-op — just illustrating intent.
            p.projected_lines[0].text = format!("unique header {n}");
            p.projected_lines[2].text = format!("unique footer {n}");
            pages.push(p);
        }
        let set = compute_header_footer_set(&pages);
        // "shared body text" sits mid-page — never matched.
        assert!(!set.contains("shared body text"));
    }

    // ----- single-page chrome detector tests -----

    use super::super::test_helpers::{line, page};

    #[test]
    fn chrome_pattern_recognizes_common_signatures() {
        assert!(matches_chrome_pattern("http://example.com/foo"));
        assert!(matches_chrome_pattern("www.nature.com/scientificreports/"));
        assert!(matches_chrome_pattern("Please cite this article in press as: ..."));
        assert!(matches_chrome_pattern("Page 12 of 24"));
        assert!(matches_chrome_pattern("9"));
        assert!(matches_chrome_pattern("© 2023 Acme Corp"));
        assert!(matches_chrome_pattern(
            "Cell Chemical Biology 24, 1–9, November 16, 2017"
        ));
        // Not chrome
        assert!(!matches_chrome_pattern(
            "The quick brown fox jumps over the lazy dog."
        ));
        assert!(!matches_chrome_pattern("Introduction"));
        // Title with year but no range — should not be stripped as chrome.
        assert!(!matches_chrome_pattern("Acme Annual Report 2023"));
    }

    #[test]
    fn detects_top_url_chrome_on_single_page() {
        // 792pt page; top band = 0..118pt.
        let lines = vec![
            // Chrome line at y=20 (in top band)
            line(
                "www.nature.com/scientificreports/",
                50.0,
                20.0,
                10.0,
                10.0,
            ),
            // Body title well below chrome (clear gap)
            line("Main Body Title", 50.0, 200.0, 14.0, 14.0),
            line("Body prose line one.", 50.0, 220.0, 10.0, 10.0),
            line("Body prose line two.", 50.0, 232.0, 10.0, 10.0),
        ];
        let p = page(lines);
        let strip = detect_single_page_chrome(&p, 10.0);
        assert!(strip.contains(&0), "top-band URL should strip");
        assert!(!strip.contains(&1));
        assert!(!strip.contains(&2));
    }

    #[test]
    fn detects_bottom_journal_citation_chrome() {
        let lines = vec![
            line("Body line.", 50.0, 300.0, 10.0, 10.0),
            line("More body.", 50.0, 312.0, 10.0, 10.0),
            // Footer at y=770 (page height 792, bottom band 673..792)
            line(
                "Cell Chemical Biology 24, 1–9, November 16, 2017",
                50.0,
                770.0,
                10.0,
                10.0,
            ),
        ];
        let p = page(lines);
        let strip = detect_single_page_chrome(&p, 10.0);
        assert!(strip.contains(&2), "bottom journal-cite should strip");
        assert!(!strip.contains(&0));
    }

    #[test]
    fn preserves_title_at_top_without_chrome_pattern() {
        // A document title at the top should NOT be stripped — no pattern
        // hint AND we no longer fall back on font-size delta.
        let lines = vec![
            line("My Important Document", 50.0, 30.0, 18.0, 18.0),
            line("Author Name", 50.0, 60.0, 10.0, 10.0),
            line("Body prose here.", 50.0, 200.0, 10.0, 10.0),
        ];
        let p = page(lines);
        let strip = detect_single_page_chrome(&p, 10.0);
        assert!(
            strip.is_empty(),
            "title without chrome pattern must survive, got {:?}",
            strip
        );
    }

    #[test]
    fn chrome_with_no_isolation_gap_is_not_stripped() {
        // URL at top followed *immediately* by body — no gap → don't strip.
        let lines = vec![
            line("http://example.com/foo", 50.0, 20.0, 10.0, 10.0),
            // Next line within 1× body size below — no isolation.
            line("Body line right after.", 50.0, 32.0, 10.0, 10.0),
            line("Continuing body.", 50.0, 44.0, 10.0, 10.0),
        ];
        let p = page(lines);
        let strip = detect_single_page_chrome(&p, 10.0);
        assert!(
            strip.is_empty(),
            "no isolation gap means it's part of body, got {:?}",
            strip
        );
    }
}
