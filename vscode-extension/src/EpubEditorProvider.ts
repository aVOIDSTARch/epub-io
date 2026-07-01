import * as vscode from "vscode";
import { EpubModel, Metadata } from "./epub/EpubModel";

/** A live EPUB opened in the custom editor. `model` is replaced on revert. */
export class EpubDocument implements vscode.CustomDocument {
  constructor(
    public readonly uri: vscode.Uri,
    public model: EpubModel,
  ) {}
  dispose(): void {
    /* nothing retained beyond the model */
  }
}

/**
 * Rich EPUB editor: a webview showing the cover, an editable Dublin Core
 * metadata form, and a spine browser/reader with an editable source view.
 * Edits flow through VS Code's custom-document dirty/save/undo machinery.
 */
export class EpubEditorProvider implements vscode.CustomEditorProvider<EpubDocument> {
  public static readonly viewType = "epubStudio.epubEditor";

  private readonly _onDidChangeCustomDocument =
    new vscode.EventEmitter<vscode.CustomDocumentEditEvent<EpubDocument>>();
  public readonly onDidChangeCustomDocument = this._onDidChangeCustomDocument.event;

  private readonly panels = new WeakMap<EpubDocument, vscode.WebviewPanel>();

  constructor(private readonly context: vscode.ExtensionContext) {}

  static register(context: vscode.ExtensionContext): vscode.Disposable {
    return vscode.window.registerCustomEditorProvider(
      EpubEditorProvider.viewType,
      new EpubEditorProvider(context),
      {
        webviewOptions: { retainContextWhenHidden: true },
        supportsMultipleEditorsPerDocument: false,
      },
    );
  }

  async openCustomDocument(uri: vscode.Uri): Promise<EpubDocument> {
    const data = await vscode.workspace.fs.readFile(uri);
    const model = await EpubModel.open(data);
    return new EpubDocument(uri, model);
  }

  async resolveCustomEditor(
    document: EpubDocument,
    webviewPanel: vscode.WebviewPanel,
  ): Promise<void> {
    this.panels.set(document, webviewPanel);
    const webview = webviewPanel.webview;
    webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.context.extensionUri, "media")],
    };
    webview.html = this.render(webview);

    webviewPanel.onDidDispose(() => this.panels.delete(document));

    webview.onDidReceiveMessage(async (msg) => {
      try {
        switch (msg?.type) {
          case "ready":
            await this.postState(document, webview);
            break;
          case "saveMetadata":
            this.applyMetadata(document, msg.patch as Partial<Metadata>);
            await this.postState(document, webview);
            break;
          case "openChapter": {
            const raw = await document.model.readText(msg.href);
            webview.postMessage({ type: "chapter", href: msg.href, raw });
            break;
          }
          case "saveChapter":
            await this.applyChapter(document, msg.href, msg.content);
            break;
          case "command":
            await vscode.commands.executeCommand(msg.command, document.uri);
            break;
        }
      } catch (err) {
        vscode.window.showErrorMessage(`EPUB Studio: ${String((err as Error).message ?? err)}`);
      }
    });
  }

  private async postState(document: EpubDocument, webview: vscode.Webview): Promise<void> {
    const model = document.model;
    const cover = await model.cover();
    const coverUri = cover
      ? `data:${cover.mime};base64,${Buffer.from(cover.data).toString("base64")}`
      : undefined;
    webview.postMessage({
      type: "state",
      fileName: document.uri.path.split("/").pop(),
      opfPath: model.opfPath,
      metadata: model.metadata,
      spine: model.spine,
      cover: coverUri,
    });
  }

  private applyMetadata(document: EpubDocument, patch: Partial<Metadata>): void {
    const beforeXml = document.model.opfXml;
    const beforeMeta = { ...document.model.metadata };
    document.model.updateMetadata(patch);
    const afterXml = document.model.opfXml;
    const afterMeta = { ...document.model.metadata };
    this._onDidChangeCustomDocument.fire({
      document,
      label: "Edit metadata",
      undo: () => document.model.restoreOpf(beforeXml, beforeMeta),
      redo: () => document.model.restoreOpf(afterXml, afterMeta),
    });
  }

  private async applyChapter(
    document: EpubDocument,
    href: string,
    content: string,
  ): Promise<void> {
    const before = await document.model.readText(href).catch(() => "");
    document.model.writeChapter(href, content);
    this._onDidChangeCustomDocument.fire({
      document,
      label: "Edit chapter source",
      undo: () => document.model.writeChapter(href, before),
      redo: () => document.model.writeChapter(href, content),
    });
    vscode.window.setStatusBarMessage(`EPUB Studio: updated ${href}`, 2500);
  }

  // --- CustomEditorProvider lifecycle ---------------------------------------

  async saveCustomDocument(document: EpubDocument): Promise<void> {
    const buf = await document.model.toBuffer();
    await vscode.workspace.fs.writeFile(document.uri, buf);
  }

  async saveCustomDocumentAs(document: EpubDocument, destination: vscode.Uri): Promise<void> {
    const buf = await document.model.toBuffer();
    await vscode.workspace.fs.writeFile(destination, buf);
  }

  async revertCustomDocument(document: EpubDocument): Promise<void> {
    const data = await vscode.workspace.fs.readFile(document.uri);
    document.model = await EpubModel.open(data);
    const panel = this.panels.get(document);
    if (panel) {
      await this.postState(document, panel.webview);
    }
  }

  async backupCustomDocument(
    document: EpubDocument,
    context: vscode.CustomDocumentBackupContext,
  ): Promise<vscode.CustomDocumentBackup> {
    const buf = await document.model.toBuffer();
    await vscode.workspace.fs.writeFile(context.destination, buf);
    return {
      id: context.destination.toString(),
      delete: async () => {
        try {
          await vscode.workspace.fs.delete(context.destination);
        } catch {
          /* already gone */
        }
      },
    };
  }

  private render(webview: vscode.Webview): string {
    const media = (name: string) =>
      webview.asWebviewUri(vscode.Uri.joinPath(this.context.extensionUri, "media", name));
    const nonce = getNonce();
    const csp = [
      `default-src 'none'`,
      `img-src ${webview.cspSource} data:`,
      `style-src ${webview.cspSource} 'unsafe-inline'`,
      `font-src ${webview.cspSource}`,
      `script-src 'nonce-${nonce}'`,
    ].join("; ");

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <link href="${media("style.css")}" rel="stylesheet" />
  <title>EPUB Studio</title>
</head>
<body>
  <div id="app">
    <header>
      <h1 id="fileName">EPUB</h1>
      <div class="toolbar">
        <button data-cmd="epubStudio.enrichMetadata">Enrich (Open Library)</button>
        <button data-cmd="epubStudio.repair">Repair &amp; Clean</button>
        <button data-cmd="epubStudio.validate">Validate</button>
      </div>
    </header>
    <div class="columns">
      <section class="left">
        <img id="cover" alt="cover" hidden />
        <form id="metaForm">
          <h2>Metadata</h2>
          <label>Title <input name="title" /></label>
          <label>Author(s) <input name="creators" placeholder="comma-separated" /></label>
          <label>Language <input name="language" /></label>
          <label>Identifier / ISBN <input name="identifier" /></label>
          <label>Publisher <input name="publisher" /></label>
          <label>Date <input name="date" /></label>
          <label>Subjects <input name="subjects" placeholder="comma-separated" /></label>
          <label>Description <textarea name="description" rows="4"></textarea></label>
          <button type="submit">Save Metadata</button>
          <span class="hint">Saving marks the document dirty — press Ctrl/Cmd+S to write the .epub.</span>
        </form>
      </section>
      <section class="right">
        <h2>Spine <span id="spineCount" class="badge"></span></h2>
        <ul id="spine"></ul>
        <div id="reader" hidden>
          <div class="reader-head">
            <strong id="readerTitle"></strong>
            <span class="spacer"></span>
            <button id="toggleSource" type="button">Edit Source</button>
            <button id="closeReader" type="button">Close</button>
          </div>
          <div id="rendered"></div>
          <textarea id="source" hidden spellcheck="false"></textarea>
          <button id="saveChapter" type="button" hidden>Apply Chapter Edit</button>
        </div>
      </section>
    </div>
  </div>
  <script nonce="${nonce}" src="${media("main.js")}"></script>
</body>
</html>`;
  }
}

function getNonce(): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let text = "";
  for (let i = 0; i < 32; i++) {
    text += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return text;
}
