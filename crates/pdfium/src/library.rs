use std::ffi::CString;
use std::sync::Once;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::document::Document;
use crate::error::PdfiumError;
use crate::ffi;

static INIT: Once = Once::new();

/// Process-global PDFium serialization lock.
///
/// PDFium's FFI is **not thread-safe**: concurrent calls (even across distinct
/// documents) corrupt internal state and cause heap UB (double-free / heap
/// corruption). Every [`Library`] handle holds this mutex for its entire
/// lifetime, and the owning PDFium resources ([`Document`], `Page`,
/// `TextPage`, `Bitmap`) borrow from a [`Library`] via their `'lib` lifetime,
/// so the borrow checker statically prevents PDFium work outside the lock.
/// (`Font` is a borrowed, non-owning handle constructed through an `unsafe`
/// fn; its lock discipline is the caller's responsibility, not statically
/// enforced.)
#[cfg(not(target_arch = "wasm32"))]
fn pdfium_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// A live, locked PDFium session.
///
/// Holding a `Library` proves the current thread has exclusive,
/// process-wide access to PDFium. All PDFium resources ([`Document`] etc.)
/// borrow from this handle, which makes it impossible to call into PDFium
/// without first acquiring the lock.
///
/// `Library` is intentionally **not `Clone`**. To use PDFium from a
/// different scope, call [`Library::init`] again — this will block until
/// any other in-flight PDFium work has finished.
///
/// On `wasm32` there is no threading, so the lock is elided.
///
/// The snippet below must fail to compile — a `Document` cannot outlive
/// the `Library` that opened it:
///
/// ```compile_fail
/// use liteparse_pdfium::{Library, Document};
/// let doc: Document<'static> = {
///     let lib = Library::init();
///     lib.load_document("x.pdf", None).unwrap()
/// };
/// // `lib` was dropped above — using `doc` here is a use-after-unlock.
/// let _ = doc.page_count();
/// ```
pub struct Library {
    #[cfg(not(target_arch = "wasm32"))]
    _guard: MutexGuard<'static, ()>,
    #[cfg(target_arch = "wasm32")]
    _private: (),
}

impl Library {
    /// Acquire the process-wide PDFium lock, blocking the current thread
    /// until any other in-flight PDFium work has finished. Initializes the
    /// library on first call.
    ///
    /// Multiple concurrent callers are serialized; only one `Library`
    /// instance exists at a time.
    pub fn init() -> Library {
        #[cfg(not(target_arch = "wasm32"))]
        {
            pdfium_sys::dynamic::load_default().expect("failed to load pdfium shared library");
            // Recover from poisoning: a panic mid-FFI may leave PDFium in
            // an odd state, but subsequent calls should still be allowed
            // (the worst case is that the next parse also fails cleanly).
            let guard = pdfium_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            INIT.call_once(|| unsafe { ffi!(FPDF_InitLibrary()) });
            Library { _guard: guard }
        }
        #[cfg(target_arch = "wasm32")]
        {
            INIT.call_once(|| unsafe { ffi!(FPDF_InitLibrary()) });
            Library { _private: () }
        }
    }

    pub fn load_document(
        &self,
        path: &str,
        password: Option<&str>,
    ) -> Result<Document<'_>, PdfiumError> {
        let c_path = CString::new(path).map_err(|_| PdfiumError::FileNotFound)?;
        let c_password = password
            .map(|p| CString::new(p).map_err(|_| PdfiumError::OperationFailed))
            .transpose()?;

        let handle = unsafe {
            ffi!(FPDF_LoadDocument(
                c_path.as_ptr(),
                c_password.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            ))
        };

        if handle.is_null() {
            return Err(PdfiumError::from_last_error());
        }

        Ok(Document {
            handle,
            _lib: std::marker::PhantomData,
        })
    }

    pub fn load_document_from_bytes(
        &self,
        data: &[u8],
        password: Option<&str>,
    ) -> Result<Document<'_>, PdfiumError> {
        let c_password = password
            .map(|p| CString::new(p).map_err(|_| PdfiumError::OperationFailed))
            .transpose()?;

        let handle = unsafe {
            ffi!(FPDF_LoadMemDocument(
                data.as_ptr() as *const std::ffi::c_void,
                data.len() as i32,
                c_password.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            ))
        };

        if handle.is_null() {
            return Err(PdfiumError::from_last_error());
        }

        // SAFETY: pdfium requires the data buffer to outlive the document.
        // The caller must ensure `data` lives long enough. For owned data,
        // consider passing a Vec and having the Document hold it.
        // For now, this is the caller's responsibility.
        Ok(Document {
            handle,
            _lib: std::marker::PhantomData,
        })
    }
}
