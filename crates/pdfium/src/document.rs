use crate::error::PdfiumError;
use crate::ffi;
use crate::library::Library;
use crate::page::Page;

/// An open PDF document.
///
/// The `'lib` lifetime ties this `Document` to the [`Library`] that opened
/// it, statically guaranteeing that no PDFium calls happen after the
/// process-wide PDFium lock has been released.
pub struct Document<'lib> {
    pub(crate) handle: pdfium_sys::FPDF_DOCUMENT,
    pub(crate) _lib: std::marker::PhantomData<&'lib Library>,
}

/// One entry in the document's outline (bookmarks tree).
#[derive(Debug, Clone)]
pub struct OutlineEntry {
    /// Hierarchy depth, 1-based (top-level entries are level 1).
    pub level: u8,
    /// Bookmark title.
    pub title: String,
    /// Zero-based page index of the destination, or `None` if the destination
    /// isn't a page in this document (external link, missing dest, etc).
    pub page_index: Option<i32>,
    /// Y coordinate of the destination on the page in PDF user space (origin
    /// bottom-left), or `None` if the destination doesn't specify one. To
    /// compare against viewport-space line bboxes (origin top-left) use
    /// `page_height - y`.
    pub y: Option<f32>,
}

impl<'lib> Document<'lib> {
    pub fn page_count(&self) -> i32 {
        unsafe { ffi!(FPDF_GetPageCount(self.handle)) }
    }

    pub fn page(&self, index: i32) -> Result<Page<'_, 'lib>, PdfiumError> {
        let handle = unsafe { ffi!(FPDF_LoadPage(self.handle, index)) };
        if handle.is_null() {
            return Err(PdfiumError::PageNotFound);
        }
        Ok(Page {
            handle,
            doc_handle: self.handle,
            _doc: std::marker::PhantomData,
        })
    }

    /// Walk the document outline (bookmarks). Returns entries in pre-order
    /// (depth-first), so parents precede their children. Empty when the
    /// document has no outline.
    pub fn outline(&self) -> Vec<OutlineEntry> {
        let mut out = Vec::new();
        let root = unsafe {
            ffi!(FPDFBookmark_GetFirstChild(
                self.handle,
                std::ptr::null_mut()
            ))
        };
        if !root.is_null() {
            self.walk_bookmark(root, 1, &mut out);
        }
        out
    }

    fn walk_bookmark(
        &self,
        bookmark: pdfium_sys::FPDF_BOOKMARK,
        level: u8,
        out: &mut Vec<OutlineEntry>,
    ) {
        let mut cur = bookmark;
        while !cur.is_null() {
            let title = read_bookmark_title(cur);
            let (page_index, y) = resolve_dest(self.handle, cur);
            out.push(OutlineEntry {
                level,
                title,
                page_index,
                y,
            });

            let child = unsafe { ffi!(FPDFBookmark_GetFirstChild(self.handle, cur)) };
            if !child.is_null() {
                self.walk_bookmark(child, level.saturating_add(1), out);
            }

            cur = unsafe { ffi!(FPDFBookmark_GetNextSibling(self.handle, cur)) };
        }
    }
}

fn read_bookmark_title(bookmark: pdfium_sys::FPDF_BOOKMARK) -> String {
    let needed = unsafe { ffi!(FPDFBookmark_GetTitle(bookmark, std::ptr::null_mut(), 0)) } as usize;
    if needed < 2 {
        return String::new();
    }
    // `needed` is byte length including a trailing UTF-16 NUL terminator.
    let mut buf: Vec<u16> = vec![0; needed / 2];
    let written = unsafe {
        ffi!(FPDFBookmark_GetTitle(
            bookmark,
            buf.as_mut_ptr() as *mut std::os::raw::c_void,
            needed as std::os::raw::c_ulong,
        ))
    } as usize;
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

fn resolve_dest(
    doc: pdfium_sys::FPDF_DOCUMENT,
    bookmark: pdfium_sys::FPDF_BOOKMARK,
) -> (Option<i32>, Option<f32>) {
    let mut dest = unsafe { ffi!(FPDFBookmark_GetDest(doc, bookmark)) };
    if dest.is_null() {
        let action = unsafe { ffi!(FPDFBookmark_GetAction(bookmark)) };
        if !action.is_null() {
            dest = unsafe { ffi!(FPDFAction_GetDest(doc, action)) };
        }
    }
    if dest.is_null() {
        return (None, None);
    }
    let page_index = unsafe { ffi!(FPDFDest_GetDestPageIndex(doc, dest)) };
    let page_index = if page_index >= 0 {
        Some(page_index)
    } else {
        None
    };

    let mut has_x: pdfium_sys::FPDF_BOOL = 0;
    let mut has_y: pdfium_sys::FPDF_BOOL = 0;
    let mut has_z: pdfium_sys::FPDF_BOOL = 0;
    let mut x: f32 = 0.0;
    let mut y: f32 = 0.0;
    let mut z: f32 = 0.0;
    let ok = unsafe {
        ffi!(FPDFDest_GetLocationInPage(
            dest, &mut has_x, &mut has_y, &mut has_z, &mut x, &mut y, &mut z
        ))
    };
    let y_out = if ok != 0 && has_y != 0 { Some(y) } else { None };
    (page_index, y_out)
}

impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe { ffi!(FPDF_CloseDocument(self.handle)) };
    }
}
