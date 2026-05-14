use napi::bindgen_prelude::*;
use napi_derive::napi;

mod types;

use types::{JsLiteParseConfig, JsParseResult, JsScreenshotResult};

/// Main LiteParse parser class.
#[napi]
pub struct LiteParse {
    inner: liteparse::parser::LiteParse,
    config: liteparse::config::LiteParseConfig,
}

#[napi]
impl LiteParse {
    /// Create a new LiteParse instance with optional configuration.
    /// Any fields not provided will use defaults.
    #[napi(constructor)]
    pub fn new(config: Option<JsLiteParseConfig>) -> Self {
        let rust_config = config.map(|c| c.into_rust()).unwrap_or_default();
        let inner = liteparse::parser::LiteParse::new(rust_config.clone());
        Self {
            inner,
            config: rust_config,
        }
    }

    /// Parse a document. Accepts a file path (string) or raw PDF bytes (Buffer).
    #[napi]
    pub async fn parse(&self, input: Either<String, Buffer>) -> Result<JsParseResult> {
        use liteparse::types::PdfInput;

        let pdf_input = match input {
            Either::A(path) => PdfInput::Path(path),
            Either::B(buf) => PdfInput::Bytes(buf.to_vec()),
        };

        let result = self
            .inner
            .parse_input(pdf_input)
            .await
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(JsParseResult::from_rust(&result, &self.config))
    }

    /// Take screenshots of document pages. Returns PNG image buffers.
    #[napi]
    pub fn screenshot(
        &self,
        input: String,
        page_numbers: Option<Vec<u32>>,
    ) -> Result<Vec<JsScreenshotResult>> {
        let dpi = self.config.dpi;
        let lib = pdfium::Library::init();
        let document = lib
            .load_document(&input, self.config.password.as_deref())
            .map_err(|e| Error::from_reason(e.to_string()))?;
        let page_count = document.page_count() as u32;

        let pages: Vec<u32> = match page_numbers {
            Some(nums) => nums,
            None => (1..=page_count).collect(),
        };

        let mut results = Vec::with_capacity(pages.len());
        for page_num in pages {
            if page_num < 1 || page_num > page_count {
                return Err(Error::from_reason(format!(
                    "page {page_num} out of range (document has {page_count} pages)"
                )));
            }
            let page = document
                .page((page_num - 1) as i32)
                .map_err(|e| Error::from_reason(e.to_string()))?;
            let bitmap = page
                .render(dpi)
                .map_err(|e| Error::from_reason(e.to_string()))?;

            let width = bitmap.width() as u32;
            let height = bitmap.height() as u32;
            let rgba = bitmap.to_rgba();

            // Encode as PNG into a buffer
            let mut png_buf: Vec<u8> = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
            use image::ImageEncoder;
            encoder
                .write_image(&rgba, width, height, image::ColorType::Rgba8.into())
                .map_err(|e| Error::from_reason(format!("PNG encode failed: {e}")))?;

            results.push(JsScreenshotResult {
                page_num,
                width,
                height,
                image_buffer: png_buf.into(),
            });
        }

        Ok(results)
    }

    /// Format a parse result according to the configured output format.
    #[napi]
    pub fn format(&self, result: JsParseResult) -> Result<String> {
        let rust_result = result.to_rust();
        self.inner
            .format(&rust_result)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    /// Get the current configuration.
    #[napi(getter)]
    pub fn config(&self) -> JsLiteParseConfig {
        JsLiteParseConfig::from_rust(&self.config)
    }
}
