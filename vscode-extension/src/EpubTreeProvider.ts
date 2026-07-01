import * as vscode from "vscode";
import { EpubModel } from "./epub/EpubModel";

type NodeKind = "group" | "value" | "chapter" | "resource";

export class EpubNode {
  constructor(
    public readonly label: string,
    public readonly kind: NodeKind,
    public readonly collapsible: vscode.TreeItemCollapsibleState,
    public readonly children: EpubNode[] = [],
    public readonly href?: string,
    public readonly description?: string,
  ) {}
}

/** Read-only outline of the active EPUB: metadata, spine, and manifest. */
export class EpubTreeProvider implements vscode.TreeDataProvider<EpubNode> {
  private readonly _onDidChange = new vscode.EventEmitter<EpubNode | undefined>();
  public readonly onDidChangeTreeData = this._onDidChange.event;

  private roots: EpubNode[] = [];
  private sourceUri: vscode.Uri | undefined;

  refresh(): void {
    void this.rebuild();
  }

  private async rebuild(): Promise<void> {
    const uri = activeEpubUri();
    this.sourceUri = uri;
    if (!uri) {
      this.roots = [
        new EpubNode("Open an .epub to see its structure", "value", vscode.TreeItemCollapsibleState.None),
      ];
      this._onDidChange.fire(undefined);
      return;
    }
    try {
      const data = await vscode.workspace.fs.readFile(uri);
      const model = await EpubModel.open(data);
      this.roots = this.build(model);
    } catch (err) {
      this.roots = [
        new EpubNode(`Failed to read EPUB: ${(err as Error).message}`, "value", vscode.TreeItemCollapsibleState.None),
      ];
    }
    this._onDidChange.fire(undefined);
  }

  private build(model: EpubModel): EpubNode[] {
    const m = model.metadata;
    const none = vscode.TreeItemCollapsibleState.None;
    const collapsed = vscode.TreeItemCollapsibleState.Collapsed;
    const expanded = vscode.TreeItemCollapsibleState.Expanded;

    const metaValues: EpubNode[] = [
      new EpubNode("Title", "value", none, [], undefined, m.title),
      new EpubNode("Author", "value", none, [], undefined, m.creators.join(", ")),
      new EpubNode("Language", "value", none, [], undefined, m.language),
      new EpubNode("Identifier", "value", none, [], undefined, m.identifier),
      new EpubNode("Publisher", "value", none, [], undefined, m.publisher),
      new EpubNode("Date", "value", none, [], undefined, m.date),
      new EpubNode("Subjects", "value", none, [], undefined, m.subjects.join(", ")),
    ];

    const spine = model.spine.map(
      (s) => new EpubNode(s.title, "chapter", none, [], s.href, s.href),
    );

    const manifest = [...model.manifest.values()].map(
      (i) => new EpubNode(i.href, "resource", none, [], i.href, i.mediaType),
    );

    return [
      new EpubNode("Metadata", "group", expanded, metaValues),
      new EpubNode(`Spine (${spine.length})`, "group", collapsed, spine),
      new EpubNode(`Manifest (${manifest.length})`, "group", collapsed, manifest),
    ];
  }

  getTreeItem(element: EpubNode): vscode.TreeItem {
    const item = new vscode.TreeItem(element.label, element.collapsible);
    item.description = element.description;
    if (element.kind === "chapter") {
      item.iconPath = new vscode.ThemeIcon("book");
      item.tooltip = `Edit ${element.href}`;
      if (this.sourceUri && element.href) {
        item.command = {
          command: "epubStudio.editChapter",
          title: "Edit Chapter Source",
          arguments: [this.sourceUri, element.href],
        };
      }
    } else if (element.kind === "resource") {
      item.iconPath = new vscode.ThemeIcon("file");
    } else if (element.kind === "value") {
      item.iconPath = new vscode.ThemeIcon("symbol-field");
    }
    return item;
  }

  getChildren(element?: EpubNode): EpubNode[] {
    if (!element) {
      if (this.roots.length === 0) {
        this.refresh();
      }
      return this.roots;
    }
    return element.children;
  }
}

/** The .epub backing the active tab (custom editor or plain file), if any. */
export function activeEpubUri(): vscode.Uri | undefined {
  const tab = vscode.window.tabGroups.activeTabGroup?.activeTab;
  const input = tab?.input as { uri?: vscode.Uri; viewType?: string } | undefined;
  if (input?.uri && input.uri.path.toLowerCase().endsWith(".epub")) {
    return input.uri;
  }
  // Fall back to any visible .epub tab.
  for (const group of vscode.window.tabGroups.all) {
    for (const t of group.tabs) {
      const i = t.input as { uri?: vscode.Uri } | undefined;
      if (i?.uri && i.uri.path.toLowerCase().endsWith(".epub")) {
        return i.uri;
      }
    }
  }
  return undefined;
}
