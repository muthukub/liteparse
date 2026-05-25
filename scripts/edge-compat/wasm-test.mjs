/**
 * Edge runtime compatibility test for LiteParse WASM module.
 *
 * Spins up a Miniflare (Cloudflare Workers) instance that loads the WASM
 * module and parses a PDF, verifying it works in an edge runtime environment.
 *
 * Usage: node scripts/edge-compat/wasm-test.mjs
 *
 * Requires: miniflare (npm i -D miniflare)
 * Expects:  packages/wasm/pkg/ to contain the built WASM files
 *           demo/docs/apple-10k-2024.pdf to exist
 */

import { Miniflare } from "miniflare";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(fileURLToPath(import.meta.url), "../../..");

const wasmPath = resolve(ROOT, "packages/wasm/pkg/liteparse_wasm_bg.wasm");
const gluePath = resolve(ROOT, "packages/wasm/pkg/liteparse_wasm.js");
const pdfPath = resolve(ROOT, "demo/docs/apple-10k-2024.pdf");

// Build a self-contained worker script.
// The generated glue exports an async `init()` that fetches the .wasm via URL.
// In edge runtimes, we import the WASM as a module and use `initSync` instead.
let glueSource = readFileSync(gluePath, "utf-8");

// Strip default export (the async init / fetch-based loader)
glueSource = glueSource.replace(/export\s*\{[^}]*__wbg_init\s+as\s+default[^}]*\};?/g, "");
glueSource = glueSource.replace(/export\s+default\s+__wbg_init\s*;?/g, "");
glueSource = glueSource.replace(/export\s*\{\s*initSync\s*(?:,\s*__wbg_init\s+as\s+default\s*)?\}\s*;?/g, "");

const workerScript = `
// Import WASM as a module (standard edge runtime pattern)
import __liteparse_wasm_mod from "liteparse.wasm";

// --- LiteParse WASM glue (patched) ---
${glueSource}

// --- Edge worker handler ---
export default {
  async fetch(request) {
    try {
      initSync({ module: __liteparse_wasm_mod });

      const parser = new LiteParse({ ocrEnabled: false, outputFormat: "text", quiet: true });

      const pdfBytes = new Uint8Array(await request.arrayBuffer());
      const result = await parser.parse(pdfBytes);

      const pageCount = result.pages.length;
      const textLength = result.text.length;

      return new Response(JSON.stringify({
        ok: true,
        pages: pageCount,
        textLength: textLength,
      }), {
        headers: { "Content-Type": "application/json" },
      });
    } catch (err) {
      return new Response(JSON.stringify({
        ok: false,
        error: err.message || String(err),
        stack: err.stack,
      }), {
        status: 500,
        headers: { "Content-Type": "application/json" },
      });
    }
  }
};
`;

async function main() {
  console.log("Starting Miniflare (Cloudflare Workers runtime)...");

  const wasmBytes = readFileSync(wasmPath);

  const mf = new Miniflare({
    modules: [
      { type: "ESModule", path: "worker.mjs", contents: workerScript },
      { type: "CompiledWasm", path: "liteparse.wasm", contents: wasmBytes },
    ],
    compatibilityDate: "2024-01-01",
  });

  try {
    const pdfBytes = readFileSync(pdfPath);
    console.log(`Sending ${(pdfBytes.length / 1024 / 1024).toFixed(1)}MB PDF to edge worker...`);

    const response = await mf.dispatchFetch("http://localhost/parse", {
      method: "POST",
      body: pdfBytes,
    });

    const result = await response.json();

    if (!result.ok) {
      console.error(`FAIL: ${result.error}`);
      if (result.stack) console.error(result.stack);
      process.exit(1);
    }

    if (result.pages === 0) {
      console.error("FAIL: No pages parsed");
      process.exit(1);
    }

    if (result.textLength < 100) {
      console.error(`FAIL: Text too short: ${result.textLength} chars`);
      process.exit(1);
    }

    console.log(`PASS: ${result.pages} pages, ${result.textLength} chars`);
  } finally {
    await mf.dispose();
  }
}

main().catch((err) => {
  console.error("Test runner failed:", err);
  process.exit(1);
});
