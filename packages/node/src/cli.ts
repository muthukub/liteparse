#!/usr/bin/env node

import { program } from "commander";
import { LiteParse, type LiteParseConfig } from "./lib.js";
import { readFileSync } from "node:fs";
import { writeFileSync } from "node:fs";

program
  .name("liteparse")
  .description("Fast, lightweight PDF and document parsing")
  .version("2.0.0");

program
  .command("parse")
  .description("Parse a document and extract text")
  .argument("<file>", "Path to the document file")
  .option("-o, --output <file>", "Output file path")
  .option("--format <format>", 'Output format: json|text (default: "text")')
  .option("--ocr-server-url <url>", "HTTP OCR server URL")
  .option("--no-ocr", "Disable OCR")
  .option("--ocr-language <lang>", "OCR language (default: eng)")
  .option("--max-pages <n>", "Max pages to parse", parseInt)
  .option(
    "--target-pages <pages>",
    'Pages to parse (e.g., "1-5,10,15-20")',
  )
  .option("--dpi <dpi>", "Rendering DPI", parseFloat)
  .option("--preserve-small-text", "Keep very small text")
  .option("--password <password>", "Password for encrypted documents")
  .option("--config <file>", "JSON config file path")
  .option("-q, --quiet", "Suppress progress output")
  .option("--num-workers <n>", "Number of concurrent OCR workers", parseInt)
  .action(async (file: string, opts: Record<string, unknown>) => {
    try {
      const config: Partial<LiteParseConfig> = {};

      // Load config file if provided
      if (opts.config) {
        const fileConfig = JSON.parse(
          readFileSync(opts.config as string, "utf-8"),
        );
        Object.assign(config, fileConfig);
      }

      // CLI options override config file
      if (opts.format) config.outputFormat = opts.format as "json" | "text";
      if (opts.ocrServerUrl)
        config.ocrServerUrl = opts.ocrServerUrl as string;
      if (opts.ocr === false) config.ocrEnabled = false;
      if (opts.ocrLanguage) config.ocrLanguage = opts.ocrLanguage as string;
      if (opts.maxPages) config.maxPages = opts.maxPages as number;
      if (opts.targetPages) config.targetPages = opts.targetPages as string;
      if (opts.dpi) config.dpi = opts.dpi as number;
      if (opts.preserveSmallText) config.preserveVerySmallText = true;
      if (opts.password) config.password = opts.password as string;
      if (opts.quiet) config.quiet = true;
      if (opts.numWorkers) config.numWorkers = opts.numWorkers as number;

      // Default CLI output to text (library defaults to json)
      if (!config.outputFormat) config.outputFormat = "text";

      const parser = new LiteParse(config);
      const result = await parser.parse(file);

      const output =
        config.outputFormat === "json"
          ? JSON.stringify(
              {
                pages: result.pages.map((p) => ({
                  page: p.pageNum,
                  width: p.width,
                  height: p.height,
                  text: p.text,
                  textItems: p.textItems,
                })),
              },
              null,
              2,
            )
          : result.text;

      if (opts.output) {
        writeFileSync(opts.output as string, output, "utf-8");
      } else {
        process.stdout.write(output);
      }
    } catch (err) {
      console.error(
        `Error: ${err instanceof Error ? err.message : String(err)}`,
      );
      process.exit(1);
    }
  });

program.parse(process.argv);
