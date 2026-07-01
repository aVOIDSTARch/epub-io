import * as vscode from "vscode";
import { EpubModel } from "./epub/EpubModel";

interface Binding {
  epub: vscode.Uri;
  href: string;
}

/**
 * Edits a chapter's XHTML source in a normal text editor by extracting it to a
 * temp file; on save, the content is written back into the .epub on disk.
 */
export class ChapterEditManager {
  private readonly bindings = new Map<string, Binding>();

  constructor(private readonly storage: vscode.Uri) {}

  register(): vscode.Disposable {
    return vscode.workspace.onDidSaveTextDocument((doc) => this.onSave(doc));
  }

  async edit(epub: vscode.Uri, href: string): Promise<void> {
    const data = await vscode.workspace.fs.readFile(epub);
    const model = await EpubModel.open(data);
    const text = await model.readText(href);

    const dir = vscode.Uri.joinPath(this.storage, "chapters");
    await vscode.workspace.fs.createDirectory(dir);
    const safe = `${slug(epub.path)}__${href.split("/").pop() ?? "chapter.xhtml"}`;
    const tmp = vscode.Uri.joinPath(dir, safe);
    await vscode.workspace.fs.writeFile(tmp, Buffer.from(text, "utf8"));

    this.bindings.set(tmp.fsPath, { epub, href });
    const doc = await vscode.workspace.openTextDocument(tmp);
    await vscode.languages.setTextDocumentLanguage(doc, "html");
    await vscode.window.showTextDocument(doc, { preview: false });
    vscode.window.setStatusBarMessage(
      `EPUB Studio: editing ${href} — save to write it back into the .epub`,
      4000,
    );
  }

  private async onSave(doc: vscode.TextDocument): Promise<void> {
    const binding = this.bindings.get(doc.uri.fsPath);
    if (!binding) {
      return;
    }
    try {
      const data = await vscode.workspace.fs.readFile(binding.epub);
      const model = await EpubModel.open(data);
      model.writeChapter(binding.href, doc.getText());
      await vscode.workspace.fs.writeFile(binding.epub, await model.toBuffer());
      vscode.window.setStatusBarMessage(`EPUB Studio: saved ${binding.href} into the .epub`, 3000);
      await vscode.commands.executeCommand("epubStudio.refresh");
    } catch (err) {
      vscode.window.showErrorMessage(
        `EPUB Studio: failed to write chapter back — ${(err as Error).message}`,
      );
    }
  }
}

function slug(s: string): string {
  return s.replace(/[^a-zA-Z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(-60);
}
