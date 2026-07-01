import * as vscode from "vscode";
import { execFile } from "child_process";
import * as fs from "fs";
import * as path from "path";

export interface RunResult {
  code: number;
  stdout: string;
  stderr: string;
}

function run(bin: string, args: string[], cwd?: string): Promise<RunResult> {
  return new Promise((resolve) => {
    execFile(bin, args, { cwd, maxBuffer: 32 * 1024 * 1024 }, (err, stdout, stderr) => {
      const code = err && typeof (err as NodeJS.ErrnoException).code === "number"
        ? Number((err as NodeJS.ErrnoException).code)
        : err
          ? 1
          : 0;
      resolve({ code, stdout: stdout ?? "", stderr: stderr ?? "" });
    });
  });
}

/**
 * Locate the epub-io binary: explicit setting, then the workspace debug/release
 * build, then `epub-io` on PATH.
 */
export function resolveEpubIo(): string {
  const configured = vscode.workspace.getConfiguration("epubStudio").get<string>("epubIoPath");
  if (configured && configured.trim().length > 0) {
    return configured.trim();
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    for (const rel of ["target/release/epub-io", "target/debug/epub-io"]) {
      const p = path.join(folder.uri.fsPath, rel);
      if (fs.existsSync(p)) {
        return p;
      }
    }
  }
  return "epub-io";
}

export interface ConvertOptions {
  input: string;
  output: string;
  enrich?: boolean;
  ttsOptimize?: boolean;
  isbn?: string;
}

/**
 * Run `epub-io convert` to produce a cleaned, optionally enriched EPUB. This is
 * the "repair" path: it re-reads the source and writes a well-formed EPUB.
 */
export async function convert(opts: ConvertOptions): Promise<RunResult> {
  const bin = resolveEpubIo();
  const args = ["convert", "-i", opts.input, opts.output];
  if (opts.enrich === false) {
    args.push("--no-enrich");
  }
  if (opts.ttsOptimize === false) {
    args.push("--no-tts");
  }
  if (opts.isbn && opts.isbn.trim().length > 0) {
    args.push("--isbn", opts.isbn.trim());
  }
  return run(bin, args);
}

export async function version(): Promise<RunResult> {
  return run(resolveEpubIo(), ["--version"]);
}
