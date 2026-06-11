//! Walk the PDF structure tree (tagged-PDF tree) for a page.
//!
//! Each struct element has:
//! - a role (string: "H1", "Figure", "Table", ...)
//! - zero or more marked content ids (mcids) that tie the element to
//!   page-content stream blocks
//! - zero or more child elements
//!
//! For LiteParse's purposes we want a flat list of nodes with:
//! - the role string
//! - viewport-space bbox derived from joining page objects whose
//!   `FPDFPageObj_GetMarkedContentID` matches one of this node's mcids
//! - the mcid list itself (so the layout pass can map text runs to nodes)

use crate::ffi;
use crate::page::{Page, ViewportTransform};
use crate::types::RectF;

/// One node in the structure tree, flattened for downstream use.
#[derive(Debug, Clone)]
pub struct StructNode {
    /// Role string from `FPDF_StructElement_GetType` (e.g. "H1", "P", "Figure").
    pub role: String,
    /// Marked content ids attached to this element (and its non-struct child markers).
    pub mcids: Vec<i32>,
    /// Union bbox of page objects tagged with any of `mcids`, in viewport
    /// coordinates (top-left origin, 72 DPI). `None` when none of the mcids
    /// resolved to a bbox on the page.
    pub bbox: Option<RectF>,
    /// Optional alt text (set for Figure / Formula elements when present).
    pub alt_text: Option<String>,
}

impl Page<'_, '_> {
    /// Walk this page's structure tree (tagged-PDF tree). Returns an empty
    /// vec when the page is untagged or the document has no struct tree.
    /// Nodes are returned in pre-order (parent before children).
    pub fn struct_tree(&self, view_box: &RectF) -> Vec<StructNode> {
        let tree = unsafe { ffi!(FPDF_StructTree_GetForPage(self.handle)) };
        if tree.is_null() {
            return Vec::new();
        }

        let mcid_bboxes = collect_mcid_bboxes(self, view_box);
        let mut out = Vec::new();

        let count = unsafe { ffi!(FPDF_StructTree_CountChildren(tree)) };
        for i in 0..count {
            let elem = unsafe { ffi!(FPDF_StructTree_GetChildAtIndex(tree, i)) };
            if !elem.is_null() {
                walk_element(elem, &mcid_bboxes, &mut out);
            }
        }

        unsafe { ffi!(FPDF_StructTree_Close(tree)) };

        out
    }
}

/// Pre-scan all page objects on the page, building `mcid → union(bbox)` in
/// viewport space. Each struct node then unions the bboxes for its own mcids.
fn collect_mcid_bboxes(
    page: &Page<'_, '_>,
    view_box: &RectF,
) -> std::collections::HashMap<i32, RectF> {
    let vp = page.viewport_transform(view_box);
    let obj_count = unsafe { ffi!(FPDFPage_CountObjects(page.handle)) };
    let mut map: std::collections::HashMap<i32, RectF> = std::collections::HashMap::new();

    for i in 0..obj_count {
        let obj = unsafe { ffi!(FPDFPage_GetObject(page.handle, i)) };
        if obj.is_null() {
            continue;
        }
        let mcid = unsafe { ffi!(FPDFPageObj_GetMarkedContentID(obj)) };
        if mcid < 0 {
            continue;
        }
        let bbox = page_object_bbox(obj, &vp);
        if let Some(b) = bbox {
            map.entry(mcid)
                .and_modify(|cur| *cur = union_rect(cur, &b))
                .or_insert(b);
        }
    }

    map
}

fn page_object_bbox(obj: pdfium_sys::FPDF_PAGEOBJECT, vp: &ViewportTransform) -> Option<RectF> {
    let mut left = 0.0f32;
    let mut bottom = 0.0f32;
    let mut right = 0.0f32;
    let mut top = 0.0f32;
    let ok = unsafe {
        ffi!(FPDFPageObj_GetBounds(
            obj,
            &mut left,
            &mut bottom,
            &mut right,
            &mut top
        ))
    };
    if ok == 0 {
        return None;
    }
    Some(vp.transform_bounds(&RectF {
        left,
        top,
        right,
        bottom,
    }))
}

fn union_rect(a: &RectF, b: &RectF) -> RectF {
    RectF {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn walk_element(
    elem: pdfium_sys::FPDF_STRUCTELEMENT,
    mcid_bboxes: &std::collections::HashMap<i32, RectF>,
    out: &mut Vec<StructNode>,
) {
    let role = read_element_type(elem);
    let alt_text = read_alt_text(elem);

    // Collect mcids: the multi-mcid getters + per-child marked-content-only children.
    let mut mcids: Vec<i32> = Vec::new();
    let n_mcids = unsafe { ffi!(FPDF_StructElement_GetMarkedContentIdCount(elem)) };
    for i in 0..n_mcids {
        let m = unsafe { ffi!(FPDF_StructElement_GetMarkedContentIdAtIndex(elem, i)) };
        if m >= 0 {
            mcids.push(m);
        }
    }
    // Also legacy: single direct mcid getter (older tag-trees expose it here).
    let single = unsafe { ffi!(FPDF_StructElement_GetMarkedContentID(elem)) };
    if single >= 0 && !mcids.contains(&single) {
        mcids.push(single);
    }

    let n_children = unsafe { ffi!(FPDF_StructElement_CountChildren(elem)) };
    for i in 0..n_children {
        // Non-struct children expose their mcid via GetChildMarkedContentID
        // (returns -1 when the child is itself a struct element).
        let child_mcid = unsafe { ffi!(FPDF_StructElement_GetChildMarkedContentID(elem, i)) };
        if child_mcid >= 0 && !mcids.contains(&child_mcid) {
            mcids.push(child_mcid);
        }
    }

    let bbox = union_mcid_bboxes(&mcids, mcid_bboxes);
    out.push(StructNode {
        role,
        mcids,
        bbox,
        alt_text,
    });

    for i in 0..n_children {
        let child_elem = unsafe { ffi!(FPDF_StructElement_GetChildAtIndex(elem, i)) };
        if !child_elem.is_null() {
            walk_element(child_elem, mcid_bboxes, out);
        }
    }
}

fn union_mcid_bboxes(
    mcids: &[i32],
    mcid_bboxes: &std::collections::HashMap<i32, RectF>,
) -> Option<RectF> {
    let mut acc: Option<RectF> = None;
    for m in mcids {
        if let Some(b) = mcid_bboxes.get(m) {
            acc = Some(match acc {
                Some(a) => union_rect(&a, b),
                None => *b,
            });
        }
    }
    acc
}

fn read_element_type(elem: pdfium_sys::FPDF_STRUCTELEMENT) -> String {
    read_widestring(|buf, len| unsafe { ffi!(FPDF_StructElement_GetType(elem, buf, len)) })
}

fn read_alt_text(elem: pdfium_sys::FPDF_STRUCTELEMENT) -> Option<String> {
    let s =
        read_widestring(|buf, len| unsafe { ffi!(FPDF_StructElement_GetAltText(elem, buf, len)) });
    if s.is_empty() { None } else { Some(s) }
}

/// Read a PDFium UTF-16LE widestring out-param via the "call once for size,
/// allocate, call again" pattern. `getter` is `(buf, buflen) -> bytes_written`.
fn read_widestring<F>(getter: F) -> String
where
    F: Fn(*mut std::os::raw::c_void, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let needed = getter(std::ptr::null_mut(), 0) as usize;
    if needed < 2 {
        return String::new();
    }
    let mut buf: Vec<u16> = vec![0; needed / 2];
    let written = getter(
        buf.as_mut_ptr() as *mut std::os::raw::c_void,
        needed as std::os::raw::c_ulong,
    ) as usize;
    if written < 2 {
        return String::new();
    }
    let chars = written / 2;
    let end = if buf.get(chars - 1) == Some(&0) {
        chars - 1
    } else {
        chars
    };
    String::from_utf16_lossy(&buf[..end])
}
