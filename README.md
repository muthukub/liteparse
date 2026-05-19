# LiteParse

[![CI](https://github.com/run-llama/liteparse/actions/workflows/ci.yml/badge.svg)](https://github.com/run-llama/liteparse/actions/workflows/ci.yml)
|
[![Crates.io version](https://img.shields.io/crates/v/liteparse.svg)](https://crates.io/crates/liteparse)
|
[![npm version](https://img.shields.io/npm/v/@llamaindex/liteparse.svg)](https://www.npmjs.com/package/@llamaindex/liteparse)
|
[![wasm version](https://img.shields.io/npm/v/@llamaindex/liteparse-wasm.svg)](https://www.npmjs.com/package/@llamaindex/liteparse-wasm)
|
[![PyPI version](https://img.shields.io/pypi/v/liteparse.svg)](https://pypi.org/project/liteparse/)
|
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
|
[Docs](https://developers.llamaindex.ai/liteparse/)

<img src="https://github.com/user-attachments/assets/07ba6a82-6bb1-4dea-b0ef-cad7df7d1622" alt="out" width="600">

LiteParse is a standalone OSS PDF parsing tool focused exclusively on **fast and light** parsing. It provides high-quality spatial text parsing with bounding boxes, without proprietary LLM features or cloud dependencies. Everything runs locally on your machine. 

**Hitting the limits of local parsing?**
For complex documents (dense tables, multi-column layouts, charts, handwritten text, or 
scanned PDFs), you'll get significantly better results with [LlamaParse](https://developers.llamaindex.ai/python/cloud/llamaparse/?utm_source=github&utm_medium=liteparse), 
our cloud-based document parser built for production document pipelines. LlamaParse handles the 
hard stuff so your models see clean, structured data and markdown.

>  👉 [Sign up for LlamaParse free](https://cloud.llamaindex.ai?utm_source=github&utm_medium=liteparse)

## Overview

- **Fast Text Parsing**: Spatial text parsing using PDFium
- **Flexible OCR System**:
  - **Built-in**: Tesseract (zero setup, bundled with the library)
  - **HTTP Servers**: Plug in any OCR server (EasyOCR, PaddleOCR, custom)
  - **Standard API**: Simple, well-defined OCR API specification
- **Screenshot Generation**: Generate high-quality page screenshots for LLM agents
- **Multiple Output Formats**: JSON and Text
- **Bounding Boxes**: Precise text positioning information
- **Multi-language**: Use from Rust, Node.js/TypeScript, Python, or the browser (WASM)
- **Multi-platform**: Linux, macOS (Intel/ARM), Windows

## Installation

All versions (except WASM) ship with the same CLI and library API. Install the one that fits your environment:

<details>
  <summary>Node.js / TypeScript</summary>

Install via npm to use the `lit` CLI or the library API:

```bash
npm i -g @llamaindex/liteparse   # CLI (global)
npm i @llamaindex/liteparse      # Library (project dependency)
```

Parse your first document right away:

```bash
lit parse document.pdf
```

Or use the library API in your Node.js or TypeScript project:

```typescript
import { LiteParse } from '@llamaindex/liteparse';

const parser = new LiteParse({ ocrEnabled: true });

const result = await parser.parse('document.pdf');
console.log(result.text);
```
</details>

<details>
  <summary>Python</summary>

Install via pip to use the `lit` CLI or the library API:

```bash
pip install liteparse
```

Parse your first document right away:

```bash
lit parse document.pdf
```

Or use the library API in your Python project:

```python
from liteparse import LiteParse

parser = LiteParse(ocr_enabled=True)
result = parser.parse('document.pdf')
print(result.text)
```

</details>

<details>
  <summary>Browser (WASM)</summary>

You can install a trimmed-down version of LiteParse that runs entirely in the browser, with no server or cloud dependencies.

```bash
npm install @llamaindex/liteparse-wasm
```

It supports PDF parsing and custom OCR engines implemented in JavaScript.

See the [WASM package README](packages/wasm/README.md) for usage details.

</details>

### Agent Skill

You can use `liteparse` as an agent skill, downloading it with the `skills` CLI tool:

```bash
npx skills add run-llama/llamaparse-agent-skills --skill liteparse
```

Or copy-pasting the [`SKILL.md`](https://github.com/run-llama/llamaparse-agent-skills/blob/main/skills/liteparse/SKILL.md) file to your own skills setup.

## CLI Usage

The CLI is the same across all installations (`npm`, `pip`, or the Rust binary).

### Parse Files

```bash
# Basic parsing
lit parse document.pdf

# Parse with specific format
lit parse document.pdf --format json -o output.json

# Parse specific pages
lit parse document.pdf --target-pages "1-5,10,15-20"

# Parse without OCR
lit parse document.pdf --no-ocr

# Parse a remote PDF
curl -sL https://example.com/report.pdf | lit parse -
```

### Batch Parsing

Parse an entire directory of documents:

```bash
lit batch-parse ./input-directory ./output-directory
```

### Generate Screenshots

Screenshots are essential for LLM agents to extract visual information that text alone cannot capture.

```bash
# Screenshot all pages
lit screenshot document.pdf -o ./screenshots

# Screenshot specific pages
lit screenshot document.pdf --target-pages "1,3,5" -o ./screenshots

# Custom DPI
lit screenshot document.pdf --dpi 300 -o ./screenshots
```

### CLI Reference

#### Parse Command

```
lit parse [OPTIONS] <file>

Options:
  -o, --output <file>          Output file path
      --format <format>        Output format: json|text [default: text]
      --no-ocr                 Disable OCR
      --ocr-language <lang>    OCR language, Tesseract format [default: eng]
      --ocr-server-url <url>   HTTP OCR server URL (uses Tesseract if not provided)
      --tessdata-path <path>   Path to tessdata directory
      --max-pages <n>          Max pages to parse [default: 1000]
      --target-pages <pages>   Pages to parse (e.g., "1-5,10,15-20")
      --dpi <dpi>              Rendering DPI [default: 150]
      --preserve-small-text    Keep very small text
      --password <password>    Password for encrypted documents
      --num-workers <n>        Concurrent OCR workers [default: CPU cores - 1]
  -q, --quiet                  Suppress progress output
  -h, --help                   Print help
```

#### Batch Parse Command

```
lit batch-parse [OPTIONS] <input-dir> <output-dir>

Options:
      --format <format>        Output format: json|text [default: text]
      --no-ocr                 Disable OCR
      --ocr-language <lang>    OCR language [default: eng]
      --ocr-server-url <url>   HTTP OCR server URL
      --tessdata-path <path>   Path to tessdata directory
      --max-pages <n>          Max pages per file [default: 1000]
      --dpi <dpi>              Rendering DPI [default: 150]
      --recursive              Recursively search input directory
      --extension <ext>        Only process files with this extension (e.g., ".pdf")
      --password <password>    Password for encrypted documents
      --num-workers <n>        Concurrent OCR workers
  -q, --quiet                  Suppress progress output
  -h, --help                   Print help
```

#### Screenshot Command

```
lit screenshot [OPTIONS] <file>

Options:
  -o, --output-dir <dir>       Output directory [default: ./screenshots]
      --target-pages <pages>   Pages to screenshot (e.g., "1,3,5" or "1-5")
      --dpi <dpi>              Rendering DPI [default: 150]
      --password <password>    Password for encrypted documents
  -q, --quiet                  Suppress progress output
  -h, --help                   Print help
```

## Library Usage

### Buffer / Uint8Array Input

All APIs that accept file paths also accept raw bytes, so you can parse documents from any source (e.g. HTTP responses, in-memory buffers) without writing to disk first.

The WASM package only accepts `Uint8Array` input, while the Node.js and Python versions accept both file paths and bytes.

```typescript
import { LiteParse } from '@llamaindex/liteparse';

const parser = new LiteParse();

// From a file read
const pdfBytes = await readFile('document.pdf');
const result = await parser.parse(pdfBytes);

// From an HTTP response
const response = await fetch('https://example.com/document.pdf');
const buffer = Buffer.from(await response.arrayBuffer());
const result2 = await parser.parse(buffer);
```

#### Screenshots

```typescript
const screenshots = await parser.screenshot('document.pdf', [1, 2, 3]);
for (const s of screenshots) {
  console.log(`Page ${s.pageNum}: ${s.width}x${s.height}`);
  // s.imageBuffer contains PNG bytes
}
```

## OCR Setup

### Default: Tesseract

Tesseract is bundled and works out of the box:

```bash
lit parse document.pdf                    # OCR enabled by default
lit parse document.pdf --ocr-language fra # Specify language
lit parse document.pdf --no-ocr           # Disable OCR
```

For offline or air-gapped environments, set `TESSDATA_PREFIX` to a directory containing pre-downloaded `.traineddata` files:

```bash
export TESSDATA_PREFIX=/path/to/tessdata
lit parse document.pdf --ocr-language eng
```

Or pass the path directly:

```bash
lit parse document.pdf --tessdata-path /path/to/tessdata
```

### Optional: HTTP OCR Servers

For higher accuracy or better performance, you can use an HTTP OCR server. We provide ready-to-use example wrappers for popular OCR engines:

- [EasyOCR](ocr/easyocr/README.md)
- [PaddleOCR](ocr/paddleocr/README.md)

You can integrate any OCR service by implementing the simple LiteParse OCR API specification (see [`OCR_API_SPEC.md`](OCR_API_SPEC.md)).

The API requires:
- POST `/ocr` endpoint
- Accepts `file` and `language` parameters
- Returns JSON: `{ results: [{ text, bbox: [x1,y1,x2,y2], confidence }] }`

## Multi-Format Input Support

LiteParse supports **automatic conversion** of various document formats to PDF before parsing.

### Supported Input Formats

#### Office Documents (via LibreOffice)
- **Word**: `.doc`, `.docx`, `.docm`, `.odt`, `.rtf`, `.pages`
- **PowerPoint**: `.ppt`, `.pptx`, `.pptm`, `.odp`, `.key`
- **Spreadsheets**: `.xls`, `.xlsx`, `.xlsm`, `.ods`, `.csv`, `.tsv`, `.numbers`

Install LibreOffice for automatic conversion:

```bash
# macOS
brew install --cask libreoffice

# Ubuntu/Debian
apt-get install libreoffice

# Windows
choco install libreoffice-fresh
```

> _On Windows, you may need to add LibreOffice's program directory (usually `C:\Program Files\LibreOffice\program`) to your PATH._

#### Images (via ImageMagick)
- **Formats**: `.jpg`, `.jpeg`, `.png`, `.gif`, `.bmp`, `.tiff`, `.webp`, `.svg`

Install ImageMagick for image-to-PDF conversion:

```bash
# macOS
brew install imagemagick

# Ubuntu/Debian
apt-get install imagemagick

# Windows
choco install imagemagick.app
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TESSDATA_PREFIX` | Path to a directory containing Tesseract `.traineddata` files. Used for offline/air-gapped environments. |

## Development

The project is a Rust workspace with the core library and language-specific binding crates.

```
crates/
├── liteparse/          # Core library + CLI binary
├── liteparse-napi/     # Node.js bindings (napi-rs)
├── liteparse-python/   # Python bindings (PyO3)
├── liteparse-wasm/     # WASM bindings (wasm-bindgen)
├── pdfium/             # PDFium Rust wrapper
└── pdfium-sys/         # PDFium FFI bindings
packages/
├── node/               # npm package (TS wrapper + native binary)
├── python/             # PyPI package (Python wrapper + native binary)
└── wasm/               # WASM npm package
```

### Building

```bash
# Build the CLI
cargo build --release -p liteparse

# Build Node.js bindings
cd packages/node && npm run build

# Build Python bindings
cd packages/python && maturin develop --release

# Build WASM
cd packages/wasm && npm run build
```

We provide a fairly rich `AGENTS.md`/`CLAUDE.md` that we recommend using to help with development + coding agents.

## License

Apache 2.0

## Credits

Built on top of:

- [PDFium](https://pdfium.googlesource.com/pdfium/) - PDF rendering and text extraction
- [Tesseract](https://github.com/tesseract-ocr/tesseract) - OCR engine (via tesseract-rs)
- [EasyOCR](https://github.com/JaidedAI/EasyOCR) - HTTP OCR server (optional)
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) - HTTP OCR server (optional)
- [napi-rs](https://napi.rs/) - Node.js native bindings
- [PyO3](https://pyo3.rs/) - Python native bindings
- [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) - WebAssembly bindings
