use pyo3::prelude::*;
use pyo3::types::PyBytes;

use liteparse::config::{LiteParseConfig, OutputFormat};
use liteparse::types::PdfInput;

mod cli;

// ---------------------------------------------------------------------------
// Python type wrappers
// ---------------------------------------------------------------------------

#[pyclass(frozen, from_py_object)]
#[derive(Clone)]
struct PyTextItem {
    #[pyo3(get)]
    text: String,
    #[pyo3(get)]
    x: f64,
    #[pyo3(get)]
    y: f64,
    #[pyo3(get)]
    width: f64,
    #[pyo3(get)]
    height: f64,
    #[pyo3(get)]
    font_name: Option<String>,
    #[pyo3(get)]
    font_size: Option<f64>,
    #[pyo3(get)]
    confidence: Option<f64>,
}

#[pymethods]
impl PyTextItem {
    fn __repr__(&self) -> String {
        format!(
            "TextItem(text={:?}, x={}, y={}, width={}, height={})",
            self.text, self.x, self.y, self.width, self.height
        )
    }
}

impl PyTextItem {
    fn from_rust(item: &liteparse::types::TextItem) -> Self {
        Self {
            text: item.text.clone(),
            x: item.x as f64,
            y: item.y as f64,
            width: item.width as f64,
            height: item.height as f64,
            font_name: item.font_name.clone(),
            font_size: item.font_size.map(|v| v as f64),
            confidence: item.confidence.map(|v| v as f64).or(Some(1.0)),
        }
    }
}

#[pyclass(frozen, from_py_object)]
#[derive(Clone)]
struct PyParsedPage {
    #[pyo3(get)]
    page_num: u32,
    #[pyo3(get)]
    width: f64,
    #[pyo3(get)]
    height: f64,
    #[pyo3(get)]
    text: String,
    #[pyo3(get)]
    text_items: Vec<PyTextItem>,
}

#[pymethods]
impl PyParsedPage {
    fn __repr__(&self) -> String {
        format!(
            "ParsedPage(page_num={}, width={}, height={}, text_items={})",
            self.page_num,
            self.width,
            self.height,
            self.text_items.len()
        )
    }
}

impl PyParsedPage {
    fn from_rust(page: &liteparse::types::ParsedPage) -> Self {
        Self {
            page_num: page.page_number as u32,
            width: page.page_width as f64,
            height: page.page_height as f64,
            text: page.text.clone(),
            text_items: page.text_items.iter().map(PyTextItem::from_rust).collect(),
        }
    }
}

#[pyclass(frozen, from_py_object)]
#[derive(Clone)]
struct PyParseResult {
    #[pyo3(get)]
    pages: Vec<PyParsedPage>,
    #[pyo3(get)]
    text: String,
}

#[pymethods]
impl PyParseResult {
    #[getter]
    fn num_pages(&self) -> usize {
        self.pages.len()
    }

    fn get_page(&self, page_num: u32) -> Option<PyParsedPage> {
        self.pages.iter().find(|p| p.page_num == page_num).cloned()
    }

    fn __repr__(&self) -> String {
        format!(
            "ParseResult(pages={}, text_len={})",
            self.pages.len(),
            self.text.len()
        )
    }
}

impl PyParseResult {
    fn from_rust(result: &liteparse::parser::ParseResult) -> Self {
        Self {
            pages: result.pages.iter().map(PyParsedPage::from_rust).collect(),
            text: result.text.clone(),
        }
    }
}

#[pyclass(frozen, from_py_object)]
#[derive(Clone)]
struct PyScreenshotResult {
    #[pyo3(get)]
    page_num: u32,
    #[pyo3(get)]
    width: u32,
    #[pyo3(get)]
    height: u32,
    image_buffer: Vec<u8>,
}

#[pymethods]
impl PyScreenshotResult {
    #[getter]
    fn image_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.image_buffer)
    }

    fn __repr__(&self) -> String {
        format!(
            "ScreenshotResult(page_num={}, width={}, height={})",
            self.page_num, self.width, self.height
        )
    }
}

// ---------------------------------------------------------------------------
// Main LiteParse class
// ---------------------------------------------------------------------------

#[pyclass]
struct LiteParse {
    inner: liteparse::parser::LiteParse,
    config: LiteParseConfig,
    runtime: tokio::runtime::Runtime,
}

#[pymethods]
impl LiteParse {
    #[new]
    #[pyo3(signature = (
        *,
        ocr_language = None,
        ocr_enabled = None,
        ocr_server_url = None,
        tessdata_path = None,
        max_pages = None,
        target_pages = None,
        dpi = None,
        output_format = None,
        preserve_very_small_text = None,
        password = None,
        quiet = None,
        num_workers = None,
    ))]
    fn new(
        ocr_language: Option<String>,
        ocr_enabled: Option<bool>,
        ocr_server_url: Option<String>,
        tessdata_path: Option<String>,
        max_pages: Option<usize>,
        target_pages: Option<String>,
        dpi: Option<f32>,
        output_format: Option<String>,
        preserve_very_small_text: Option<bool>,
        password: Option<String>,
        quiet: Option<bool>,
        num_workers: Option<usize>,
    ) -> PyResult<Self> {
        let mut cfg = LiteParseConfig::default();
        if let Some(v) = ocr_language {
            cfg.ocr_language = v;
        }
        if let Some(v) = ocr_enabled {
            cfg.ocr_enabled = v;
        }
        if let Some(v) = ocr_server_url {
            cfg.ocr_server_url = Some(v);
        }
        if let Some(v) = tessdata_path {
            cfg.tessdata_path = Some(v);
        }
        if let Some(v) = max_pages {
            cfg.max_pages = v;
        }
        if let Some(v) = target_pages {
            cfg.target_pages = Some(v);
        }
        if let Some(v) = dpi {
            cfg.dpi = v;
        }
        if let Some(v) = output_format {
            cfg.output_format = match v.as_str() {
                "text" => OutputFormat::Text,
                _ => OutputFormat::Json,
            };
        }
        if let Some(v) = preserve_very_small_text {
            cfg.preserve_very_small_text = v;
        }
        if let Some(v) = password {
            cfg.password = Some(v);
        }
        if let Some(v) = quiet {
            cfg.quiet = v;
        }
        if let Some(v) = num_workers {
            cfg.num_workers = v;
        }

        let inner = liteparse::parser::LiteParse::new(cfg.clone());
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        Ok(Self {
            inner,
            config: cfg,
            runtime,
        })
    }

    /// Parse a document from a file path.
    fn parse(&self, input: String) -> PyResult<PyParseResult> {
        let pdf_input = PdfInput::Path(input);
        let result = self
            .runtime
            .block_on(self.inner.parse_input(pdf_input))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        Ok(PyParseResult::from_rust(&result))
    }

    /// Parse a document from raw bytes.
    fn parse_bytes(&self, data: Vec<u8>) -> PyResult<PyParseResult> {
        let pdf_input = PdfInput::Bytes(data);
        let result = self
            .runtime
            .block_on(self.inner.parse_input(pdf_input))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        Ok(PyParseResult::from_rust(&result))
    }

    /// Take screenshots of document pages. Returns a list of ScreenshotResult.
    #[pyo3(signature = (input, page_numbers = None))]
    fn screenshot(
        &self,
        input: String,
        page_numbers: Option<Vec<u32>>,
    ) -> PyResult<Vec<PyScreenshotResult>> {
        let dpi = self.config.dpi;
        let lib = pdfium::Library::init();
        let document = lib
            .load_document(&input, self.config.password.as_deref())
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        let page_count = document.page_count() as u32;

        let pages: Vec<u32> = match page_numbers {
            Some(nums) => nums,
            None => (1..=page_count).collect(),
        };

        let mut results = Vec::with_capacity(pages.len());
        for page_num in pages {
            if page_num < 1 || page_num > page_count {
                return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                    "page {page_num} out of range (document has {page_count} pages)"
                )));
            }
            let page = document
                .page((page_num - 1) as i32)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            let bitmap = page
                .render(dpi)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let width = bitmap.width() as u32;
            let height = bitmap.height() as u32;
            let rgba = bitmap.to_rgba();

            let mut png_buf: Vec<u8> = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
            use image::ImageEncoder;
            encoder
                .write_image(&rgba, width, height, image::ColorType::Rgba8.into())
                .map_err(|e| {
                    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                        "PNG encode failed: {e}"
                    ))
                })?;

            results.push(PyScreenshotResult {
                page_num,
                width,
                height,
                image_buffer: png_buf,
            });
        }

        Ok(results)
    }

    fn __repr__(&self) -> String {
        format!(
            "LiteParse(ocr_enabled={}, dpi={}, max_pages={})",
            self.config.ocr_enabled, self.config.dpi, self.config.max_pages
        )
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

/// Run the `lit` CLI with the given arguments.
#[pyfunction]
fn run_cli(args: Vec<String>) -> PyResult<()> {
    cli::run_cli(args).map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
}

#[pymodule]
fn _liteparse(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<LiteParse>()?;
    m.add_class::<PyParseResult>()?;
    m.add_class::<PyParsedPage>()?;
    m.add_class::<PyTextItem>()?;
    m.add_class::<PyScreenshotResult>()?;
    m.add_function(wrap_pyfunction!(run_cli, m)?)?;
    Ok(())
}
