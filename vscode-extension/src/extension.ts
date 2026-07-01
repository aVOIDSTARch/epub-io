import * as vscode from "vscode";
import * as path from "path";
import { EpubEditorProvider } from "./EpubEditorProvider";
import { EpubTreeProvider, activeEpubUri } from "./EpubTreeProvider";
import { ChapterEditManager } from "./ChapterEditManager";
import { EpubModel } from "./epub/EpubModel";
import * as EpubIo from "./tools/EpubIo";
import * as EpubCheck from "./tools/EpubCheck";

let output: vscode.OutputChannel;

export function activate(context: vscode.ExtensionContext): void {
  output = vscode.window.createOutputChannel("EPUB Studio");
  context.subscriptions.push(output);

  // Custom editor for .epub files.
  context.subscriptions.push(EpubEditorProvider.register(context));

  // Structure tree.
  const tree = new EpubTreeProvider();
  context.subscriptions.push(vscode.window.registerTreeDataProvider("epubStudio.structure", tree));
  context.subscriptions.push(
    vscode.window.tabGroups.onDidChangeTabGroups(() => tree.refresh()),
    vscode.window.tabGroups.onDidChangeTabs(() => tree.refresh()),
  );

  // Chapter source editing (temp file <-> zip).
  const chapters = new ChapterEditManager(context.globalStorageUri);
  context.subscriptions.push(chapters.register());

  const resolve = (arg?: vscode.Uri): vscode.Uri | undefined =>
    arg instanceof vscode.Uri ? arg : activeEpubUri();

  context.subscriptions.push(
    vscode.commands.registerCommand("epubStudio.refresh", () => tree.refresh()),

    vscode.commands.registerCommand("epubStudio.open", async (arg?: vscode.Uri) => {
      const uri = resolve(arg);
      if (!uri) {
        vscode.window.showWarningMessage("EPUB Studio: no .epub selected.");
        return;
      }
      await vscode.commands.executeCommand("vscode.openWith", uri, EpubEditorProvider.viewType);
    }),

    vscode.commands.registerCommand("epubStudio.editMetadata", async (arg?: vscode.Uri) => {
      const uri = resolve(arg);
      if (!uri) {
        vscode.window.showWarningMessage("EPUB Studio: no .epub selected.");
        return;
      }
      await vscode.commands.executeCommand("vscode.openWith", uri, EpubEditorProvider.viewType);
    }),

    vscode.commands.registerCommand("epubStudio.editChapter", async (arg?: vscode.Uri, href?: string) => {
      const uri = resolve(arg);
      if (!uri) {
        return;
      }
      if (!href) {
        href = await pickChapter(uri);
      }
      if (href) {
        await chapters.edit(uri, href);
      }
    }),

    vscode.commands.registerCommand("epubStudio.extractText", async (arg?: vscode.Uri, href?: string) => {
      const uri = resolve(arg);
      if (!uri) {
        return;
      }
      if (!href) {
        href = await pickChapter(uri);
      }
      if (!href) {
        return;
      }
      const model = await EpubModel.open(await vscode.workspace.fs.readFile(uri));
      const html = await model.readText(href);
      const text = htmlToText(html);
      const doc = await vscode.workspace.openTextDocument({ content: text, language: "plaintext" });
      await vscode.window.showTextDocument(doc, { preview: false });
    }),

    vscode.commands.registerCommand("epubStudio.repair", (arg?: vscode.Uri) =>
      runConvert(resolve(arg), { enrich: false, suffix: "cleaned", label: "Repair & Clean" }),
    ),

    vscode.commands.registerCommand("epubStudio.enrichMetadata", (arg?: vscode.Uri) =>
      runConvert(resolve(arg), { enrich: true, suffix: "enriched", label: "Enrich Metadata" }),
    ),

    vscode.commands.registerCommand("epubStudio.validate", (arg?: vscode.Uri) =>
      runValidate(resolve(arg)),
    ),
  );

  tree.refresh();
}

export function deactivate(): void {
  /* subscriptions disposed by VS Code */
}

interface ConvertOpts {
  enrich: boolean;
  suffix: string;
  label: string;
}

async function runConvert(uri: vscode.Uri | undefined, opts: ConvertOpts): Promise<void> {
  if (!uri) {
    vscode.window.showWarningMessage("EPUB Studio: no .epub selected.");
    return;
  }
  const dir = path.dirname(uri.fsPath);
  const base = path.basename(uri.fsPath, path.extname(uri.fsPath));
  const outPath = path.join(dir, `${base}.${opts.suffix}.epub`);

  await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: `EPUB Studio: ${opts.label}…` },
    async () => {
      const result = await EpubIo.convert({
        input: uri.fsPath,
        output: outPath,
        enrich: opts.enrich,
      });
      output.appendLine(`$ epub-io convert -i "${uri.fsPath}" "${outPath}"`);
      if (result.stdout) {
        output.appendLine(result.stdout);
      }
      if (result.stderr) {
        output.appendLine(result.stderr);
      }
      if (result.code !== 0) {
        output.show(true);
        vscode.window.showErrorMessage(
          `EPUB Studio: ${opts.label} failed (exit ${result.code}). See the EPUB Studio output.`,
        );
        return;
      }
      const open = await vscode.window.showInformationMessage(
        `EPUB Studio: wrote ${path.basename(outPath)}`,
        "Open",
      );
      if (open === "Open") {
        await vscode.commands.executeCommand(
          "vscode.openWith",
          vscode.Uri.file(outPath),
          EpubEditorProvider.viewType,
        );
      }
    },
  );
}

async function runValidate(uri: vscode.Uri | undefined): Promise<void> {
  if (!uri) {
    vscode.window.showWarningMessage("EPUB Studio: no .epub selected.");
    return;
  }
  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: "EPUB Studio: validating…" },
      async () => {
        const res = await EpubCheck.validate(uri.fsPath);
        output.clear();
        output.appendLine(`EPUBCheck — ${path.basename(uri.fsPath)}`);
        output.appendLine(res.stdout || res.stderr || "(no output)");
        if (res.stderr && res.stdout) {
          output.appendLine(res.stderr);
        }
        output.show(true);
        if (res.code === 0) {
          vscode.window.showInformationMessage("EPUB Studio: EPUBCheck found no errors.");
        } else {
          vscode.window.showWarningMessage("EPUB Studio: EPUBCheck reported issues — see output.");
        }
      },
    );
  } catch (err) {
    const choice = await vscode.window.showErrorMessage(
      `EPUB Studio: ${(err as Error).message}`,
      "Open Settings",
    );
    if (choice === "Open Settings") {
      await vscode.commands.executeCommand("workbench.action.openSettings", "epubStudio");
    }
  }
}

async function pickChapter(uri: vscode.Uri): Promise<string | undefined> {
  const model = await EpubModel.open(await vscode.workspace.fs.readFile(uri));
  const items = model.spine.map((s) => ({ label: s.title, description: s.href }));
  const pick = await vscode.window.showQuickPick(items, { placeHolder: "Select a chapter" });
  return pick?.description;
}

function htmlToText(html: string): string {
  const body = html.match(/<body[^>]*>([\s\S]*?)<\/body>/i);
  const inner = body ? body[1] : html;
  return inner
    .replace(/<(script|style)[\s\S]*?<\/\1>/gi, "")
    .replace(/<\/(p|div|h[1-6]|li|br)>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/gi, " ")
    .replace(/&amp;/gi, "&")
    .replace(/&lt;/gi, "<")
    .replace(/&gt;/gi, ">")
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&quot;/gi, '"')
    .replace(/[ \t]+\n/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
