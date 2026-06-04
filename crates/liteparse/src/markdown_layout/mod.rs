//! Block classification for the markdown emitter.
//!
//! Consumes `ProjectedLine` entries from each `ParsedPage` and groups them into
//! a sequence of `Block`s: headings, paragraphs, and (for now) raw lines that
//! don't fit a recognized shape. Tables, lists, and code blocks land in later
//! build-order steps.

mod blocks;
mod classify;
mod headings;
mod hr;
mod inline;
mod lists;
mod paragraphs;
mod repetition;
mod tables;

pub use blocks::{Block, render_blocks};
pub use classify::classify_page_with_filters;
pub use headings::{build_heading_map, compute_body_size};
pub use repetition::{compute_header_footer_set, detect_single_page_chrome};
pub use tables::detect_table_rects;

#[cfg(test)]
pub(crate) mod test_helpers;
