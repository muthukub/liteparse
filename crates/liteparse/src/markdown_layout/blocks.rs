use super::inline::escape_inline;
use super::paragraphs::ParaAccum;
use super::tables::escape_table_cell;

/// Coarse block representation: the output of page classification, consumed by
/// `render_blocks` to produce the final markdown string.
#[derive(Debug, Clone)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        text: String,
        bold: bool,
        italic: bool,
    },
    ListItem {
        ordered: bool,
        marker: String,
        level: u8,
        text: String,
        bold: bool,
        italic: bool,
    },
    /// Fenced code block — content rendered between triple-backtick fences.
    /// Each entry in `lines` is one source line; preserved as-is (only trailing
    /// whitespace stripped) so indentation survives.
    CodeBlock {
        lines: Vec<String>,
    },
    /// Confident table emitted as a markdown pipe table. `header` is `None`
    /// when the first row didn't qualify (e.g. wasn't bold and the table mode
    /// can't otherwise distinguish it).
    Table {
        header: Option<Vec<String>>,
        rows: Vec<Vec<String>>,
    },
    /// Tabular-looking region we couldn't classify confidently — rendered
    /// verbatim inside a fenced block to preserve visual structure for the
    /// downstream LLM. Strictly better than emitting a mangled pipe table.
    GridFallback {
        lines: Vec<String>,
    },
    /// A horizontal rule detected from a long thin horizontal stroke in the
    /// page's vector graphics (e.g. divider line between sections).
    HorizontalRule,
    /// Reference to a raster image on the page. Rendered as
    /// `![](image_{id}.png)`. Suppressed entirely when `ImageMode::Off`.
    Figure {
        id: String,
        bbox: crate::types::Rect,
    },
}

/// Resolve a `ParaAccum` to a `Block::Paragraph`. When the paragraph was
/// uniformly styled across all lines, return the raw text with block-level
/// `bold`/`italic` flags set so the renderer wraps it once. Otherwise return
/// the per-line inline-styled text with the flags cleared.
pub(super) fn paragraph_from_accum(accum: ParaAccum) -> Block {
    match accum.uniform {
        Some((bold, italic)) if bold || italic => Block::Paragraph {
            text: escape_inline(&accum.raw),
            bold,
            italic,
        },
        Some(_) => Block::Paragraph {
            // Uniformly plain — no emphasis markers anywhere, so the raw text
            // (with markdown specials escaped) is the right rendering.
            text: escape_inline(&accum.raw),
            bold: false,
            italic: false,
        },
        None => Block::Paragraph {
            text: accum.inline,
            bold: false,
            italic: false,
        },
    }
}

/// Wrap `text` in markdown emphasis markers based on `bold`/`italic`. Both →
/// `***text***`. Headings deliberately skip this (the `#` is the emphasis).
fn wrap_emphasis(text: &str, bold: bool, italic: bool) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    match (bold, italic) {
        (true, true) => format!("***{text}***"),
        (true, false) => format!("**{text}**"),
        (false, true) => format!("*{text}*"),
        (false, false) => text.to_string(),
    }
}

/// Render a list of blocks to a markdown string.
pub fn render_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            // A word hyphenated across a soft line wrap sometimes lands in two
            // *separate* paragraph blocks (the classifier mis-split mid-word):
            // `…they dis-` ‖ `lodged…`. When the previous block ends with
            // `<letter>-` and this block is a plain paragraph beginning with a
            // lowercase letter, splice them: drop the hyphen and join with no
            // separator, healing both the word and the spurious break. The
            // lowercase + plain-text gates keep real compounds (`well-` then a
            // capitalized `Known`) and emphasised/heading/table starts intact.
            if let (
                Block::Paragraph { .. },
                Block::Paragraph {
                    text,
                    bold: false,
                    italic: false,
                },
            ) = (&blocks[i - 1], block)
                && out.ends_with('-')
                && out
                    .chars()
                    .rev()
                    .nth(1)
                    .is_some_and(|p| p.is_ascii_alphabetic())
                && text.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            {
                out.pop();
                out.push_str(text);
                continue;
            }
            // Consecutive list items render as a tight list (single newline).
            // Everything else gets a blank line between blocks.
            let tight = matches!(block, Block::ListItem { .. })
                && matches!(blocks[i - 1], Block::ListItem { .. });
            if tight {
                out.push('\n');
            } else {
                out.push_str("\n\n");
            }
        }
        match block {
            Block::Heading { level, text } => {
                let level = (*level).clamp(1, 6) as usize;
                out.push_str(&"#".repeat(level));
                out.push(' ');
                out.push_str(text);
            }
            Block::Paragraph { text, bold, italic } => {
                out.push_str(&wrap_emphasis(text, *bold, *italic));
            }
            Block::ListItem {
                ordered,
                marker,
                level,
                text,
                bold,
                italic,
            } => {
                let indent = "  ".repeat((*level).min(6) as usize);
                out.push_str(&indent);
                if *ordered {
                    // Preserve the original marker (e.g. `138.` for footnotes
                    // or `iii)` for roman numerals) so semantic numbering
                    // survives the round-trip. CommonMark requires `1.` /
                    // `1)` style but most LLM consumers tolerate alt forms,
                    // and the alternative — renumbering as `1.` — drops info.
                    out.push_str(marker);
                    out.push(' ');
                } else {
                    out.push_str("- ");
                }
                out.push_str(&wrap_emphasis(text, *bold, *italic));
            }
            Block::Table { header, rows } => {
                let column_count = header
                    .as_ref()
                    .map(|h| h.len())
                    .or_else(|| rows.first().map(|r| r.len()))
                    .unwrap_or(0);
                if column_count == 0 {
                    continue;
                }
                if let Some(h) = header {
                    out.push_str("| ");
                    for (i, cell) in h.iter().enumerate() {
                        if i > 0 {
                            out.push_str(" | ");
                        }
                        out.push_str(&escape_table_cell(cell));
                    }
                    out.push_str(" |\n");
                } else {
                    // CommonMark/GFM requires a header row before the
                    // separator; synthesize a blank header so renderers that
                    // refuse header-less tables still display the body.
                    out.push('|');
                    for _ in 0..column_count {
                        out.push_str("   |");
                    }
                    out.push('\n');
                }
                out.push('|');
                for _ in 0..column_count {
                    out.push_str("---|");
                }
                for row in rows {
                    out.push_str("\n| ");
                    for (i, cell) in row.iter().enumerate() {
                        if i > 0 {
                            out.push_str(" | ");
                        }
                        out.push_str(&escape_table_cell(cell));
                    }
                    out.push_str(" |");
                }
            }
            Block::GridFallback { lines } => {
                out.push_str("```text\n");
                for line in lines {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str("```");
            }
            Block::CodeBlock { lines } => {
                // Pick a fence that doesn't appear inside the body. Standard
                // triple-backtick handles ~all real-world code; bump to ~~~ if
                // the body itself contains ``` (rare).
                let fence = if lines.iter().any(|l| l.contains("```")) {
                    "~~~"
                } else {
                    "```"
                };
                out.push_str(fence);
                out.push('\n');
                for line in lines {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push_str(fence);
            }
            Block::HorizontalRule => {
                out.push_str("---");
            }
            Block::Figure { id, .. } => {
                out.push_str("![](image_");
                out.push_str(id);
                out.push_str(".png)");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_blocks_formats_markdown() {
        let blocks = vec![
            Block::Heading {
                level: 1,
                text: "Title".into(),
            },
            Block::Paragraph {
                text: "A paragraph.".into(),
                bold: false,
                italic: false,
            },
            Block::Heading {
                level: 2,
                text: "Sub".into(),
            },
        ];
        let s = render_blocks(&blocks);
        assert_eq!(s, "# Title\n\nA paragraph.\n\n## Sub");
    }

    #[test]
    fn render_lists_are_tight() {
        let blocks = vec![
            Block::Paragraph {
                text: "Intro.".into(),
                bold: false,
                italic: false,
            },
            Block::ListItem {
                ordered: false,
                marker: "•".into(),
                level: 0,
                text: "a".into(),
                bold: false,
                italic: false,
            },
            Block::ListItem {
                ordered: false,
                marker: "•".into(),
                level: 0,
                text: "b".into(),
                bold: false,
                italic: false,
            },
            Block::Paragraph {
                text: "Outro.".into(),
                bold: false,
                italic: false,
            },
        ];
        let s = render_blocks(&blocks);
        assert_eq!(s, "Intro.\n\n- a\n- b\n\nOutro.");

        // Ordered: original marker preserved
        let s = render_blocks(&[
            Block::ListItem {
                ordered: true,
                marker: "138.".into(),
                level: 0,
                text: "footnote".into(),
                bold: false,
                italic: false,
            },
            Block::ListItem {
                ordered: true,
                marker: "139.".into(),
                level: 0,
                text: "next footnote".into(),
                bold: false,
                italic: false,
            },
        ]);
        assert_eq!(s, "138. footnote\n139. next footnote");
    }

    #[test]
    fn render_emphasis_combinations() {
        assert_eq!(wrap_emphasis("hi", false, false), "hi");
        assert_eq!(wrap_emphasis("hi", true, false), "**hi**");
        assert_eq!(wrap_emphasis("hi", false, true), "*hi*");
        assert_eq!(wrap_emphasis("hi", true, true), "***hi***");
    }

    #[test]
    fn code_block_escapes_internal_fence() {
        let blocks = vec![Block::CodeBlock {
            lines: vec!["body containing ``` backticks".into()],
        }];
        let s = render_blocks(&blocks);
        assert!(s.starts_with("~~~\n"));
        assert!(s.ends_with("~~~"));
    }

    #[test]
    fn renders_table_to_pipe_format() {
        let blocks = vec![Block::Table {
            header: Some(vec!["a".into(), "b".into()]),
            rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
        }];
        let s = render_blocks(&blocks);
        assert_eq!(s, "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
    }

    #[test]
    fn splices_hyphen_split_across_paragraph_blocks() {
        let p = |t: &str| Block::Paragraph {
            text: t.into(),
            bold: false,
            italic: false,
        };
        // Mid-word hyphen split into two paragraphs heals into one.
        let s = render_blocks(&[p("they dis-"), p("lodged the part")]);
        assert_eq!(s, "they dislodged the part");
        // Capitalized continuation is a real compound break — left intact.
        let s = render_blocks(&[p("the well-"), p("Known fact")]);
        assert_eq!(s, "the well-\n\nKnown fact");
        // Trailing dash not preceded by a letter doesn't splice.
        let s = render_blocks(&[p("a -"), p("dash line")]);
        assert_eq!(s, "a -\n\ndash line");
    }

    #[test]
    fn render_table_without_header_synthesizes_blank_header() {
        let blocks = vec![Block::Table {
            header: None,
            rows: vec![vec!["1".into(), "2".into()]],
        }];
        let s = render_blocks(&blocks);
        // GFM/CommonMark needs a header row before the separator; we emit a
        // blank one so renderers don't choke.
        assert!(s.contains("|---|---|"));
        assert!(s.ends_with("| 1 | 2 |"));
    }
}
