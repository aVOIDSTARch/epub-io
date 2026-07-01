import JSZip from "jszip";
import { XMLParser } from "fast-xml-parser";

/** Editable Dublin Core metadata surfaced by EPUB Studio. */
export interface Metadata {
  title: string;
  creators: string[];
  language: string;
  identifier: string;
  publisher: string;
  date: string;
  description: string;
  subjects: string[];
}

export interface ManifestItem {
  id: string;
  href: string;
  mediaType: string;
  properties?: string;
}

export interface SpineItem {
  idref: string;
  href: string;
  mediaType: string;
  title: string;
}

const XHTML = "application/xhtml+xml";

/** Return `node` as an array whether the parser produced one, a scalar, or nothing. */
function asArray<T>(node: T | T[] | undefined): T[] {
  if (node === undefined || node === null) {
    return [];
  }
  return Array.isArray(node) ? node : [node];
}

/** Extract text content from a fast-xml-parser node (string, number, or {#text}). */
function textOf(node: unknown): string {
  if (node === undefined || node === null) {
    return "";
  }
  if (typeof node === "string") {
    return node;
  }
  if (typeof node === "number" || typeof node === "boolean") {
    return String(node);
  }
  if (typeof node === "object") {
    const t = (node as Record<string, unknown>)["#text"];
    return t === undefined ? "" : String(t);
  }
  return "";
}

function xmlEscape(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Normalise a zip-internal path, resolving `.`/`..` segments. */
function normalizePath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  const out: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") {
      continue;
    }
    if (part === "..") {
      out.pop();
    } else {
      out.push(part);
    }
  }
  return out.join("/");
}

/**
 * An in-memory, editable EPUB. Reads the container/OPF to expose metadata,
 * manifest, spine and cover; supports surgical metadata edits and chapter
 * rewrites, and re-serialises back to a zip buffer.
 */
export class EpubModel {
  private constructor(
    private zip: JSZip,
    public opfPath: string,
    public opfDir: string,
    public opfXml: string,
    public metadata: Metadata,
    public manifest: Map<string, ManifestItem>,
    public spine: SpineItem[],
    public coverHref: string | undefined,
    public coverMime: string | undefined,
    public navHref: string | undefined,
  ) {}

  static async open(data: Uint8Array): Promise<EpubModel> {
    const zip = await JSZip.loadAsync(data);

    const containerFile = zip.file("META-INF/container.xml");
    if (!containerFile) {
      throw new Error("Not a valid EPUB: missing META-INF/container.xml");
    }
    const containerXml = await containerFile.async("string");
    const rootMatch = containerXml.match(/full-path\s*=\s*["']([^"']+)["']/i);
    if (!rootMatch) {
      throw new Error("Not a valid EPUB: container.xml has no rootfile full-path");
    }
    const opfPath = normalizePath(rootMatch[1]);
    const opfFile = zip.file(opfPath);
    if (!opfFile) {
      throw new Error(`OPF not found at ${opfPath}`);
    }
    const opfXml = await opfFile.async("string");
    const opfDir = opfPath.includes("/") ? opfPath.slice(0, opfPath.lastIndexOf("/")) : "";

    const parser = new XMLParser({
      ignoreAttributes: false,
      attributeNamePrefix: "@_",
      removeNSPrefix: true,
      trimValues: true,
    });
    const doc = parser.parse(opfXml);
    const pkg = doc.package ?? {};
    const metaNode = pkg.metadata ?? {};

    const metadata: Metadata = {
      title: textOf(asArray(metaNode.title)[0]),
      creators: asArray(metaNode.creator).map(textOf).filter((s) => s.length > 0),
      language: textOf(asArray(metaNode.language)[0]),
      identifier: textOf(asArray(metaNode.identifier)[0]),
      publisher: textOf(asArray(metaNode.publisher)[0]),
      date: textOf(asArray(metaNode.date)[0]),
      description: textOf(asArray(metaNode.description)[0]),
      subjects: asArray(metaNode.subject).map(textOf).filter((s) => s.length > 0),
    };

    // Manifest.
    const manifest = new Map<string, ManifestItem>();
    for (const raw of asArray<Record<string, unknown>>(pkg.manifest?.item)) {
      const id = String(raw["@_id"] ?? "");
      const href = String(raw["@_href"] ?? "");
      if (!id || !href) {
        continue;
      }
      manifest.set(id, {
        id,
        href,
        mediaType: String(raw["@_media-type"] ?? ""),
        properties: raw["@_properties"] ? String(raw["@_properties"]) : undefined,
      });
    }

    // Cover: prefer a manifest item with properties="cover-image" (EPUB3),
    // else the <meta name="cover" content="id"> pointer (EPUB2).
    let coverHref: string | undefined;
    for (const item of manifest.values()) {
      if (item.properties && item.properties.split(/\s+/).includes("cover-image")) {
        coverHref = item.href;
        break;
      }
    }
    if (!coverHref) {
      for (const m of asArray<Record<string, unknown>>(metaNode.meta)) {
        if (String(m["@_name"] ?? "") === "cover") {
          const item = manifest.get(String(m["@_content"] ?? ""));
          if (item) {
            coverHref = item.href;
          }
        }
      }
    }
    const coverMime = coverHref
      ? [...manifest.values()].find((i) => i.href === coverHref)?.mediaType
      : undefined;

    // Nav document (EPUB3).
    let navHref: string | undefined;
    for (const item of manifest.values()) {
      if (item.properties && item.properties.split(/\s+/).includes("nav")) {
        navHref = item.href;
        break;
      }
    }

    // TOC titles: parse the nav/ncx for href -> title.
    const titleByHref = await EpubModel.readTocTitles(zip, opfDir, navHref, manifest);

    // Spine.
    const spine: SpineItem[] = [];
    for (const raw of asArray<Record<string, unknown>>(pkg.spine?.itemref)) {
      const idref = String(raw["@_idref"] ?? "");
      const item = manifest.get(idref);
      if (!item) {
        continue;
      }
      const base = item.href.split("/").pop() ?? item.href;
      spine.push({
        idref,
        href: item.href,
        mediaType: item.mediaType,
        title: titleByHref.get(item.href) ?? titleByHref.get(base) ?? base,
      });
    }

    return new EpubModel(
      zip,
      opfPath,
      opfDir,
      opfXml,
      metadata,
      manifest,
      spine,
      coverHref,
      coverMime,
      navHref,
    );
  }

  private static async readTocTitles(
    zip: JSZip,
    opfDir: string,
    navHref: string | undefined,
    manifest: Map<string, ManifestItem>,
  ): Promise<Map<string, string>> {
    const map = new Map<string, string>();
    const resolve = (href: string) => normalizePath(opfDir ? `${opfDir}/${href}` : href);

    // EPUB3 nav.xhtml: <a href="chapter.xhtml">Title</a>
    if (navHref) {
      const f = zip.file(resolve(navHref));
      if (f) {
        const html = await f.async("string");
        const re = /<a\b[^>]*href\s*=\s*["']([^"'#]+)[^"']*["'][^>]*>([\s\S]*?)<\/a>/gi;
        let m: RegExpExecArray | null;
        while ((m = re.exec(html)) !== null) {
          const href = m[1].split("/").pop() ?? m[1];
          const title = m[2].replace(/<[^>]+>/g, "").trim();
          if (title) {
            map.set(href, title);
          }
        }
      }
    }

    // EPUB2 toc.ncx: <navPoint><navLabel><text>Title</text></navLabel><content src="chapter.xhtml"/>
    const ncx = [...manifest.values()].find((i) => i.mediaType === "application/x-dtbncx+xml");
    if (ncx) {
      const f = zip.file(resolve(ncx.href));
      if (f) {
        const xml = await f.async("string");
        const re =
          /<navPoint\b[\s\S]*?<text>([\s\S]*?)<\/text>[\s\S]*?<content\b[^>]*src\s*=\s*["']([^"'#]+)/gi;
        let m: RegExpExecArray | null;
        while ((m = re.exec(xml)) !== null) {
          const title = m[1].replace(/<[^>]+>/g, "").trim();
          const href = m[2].split("/").pop() ?? m[2];
          if (title && !map.has(href)) {
            map.set(href, title);
          }
        }
      }
    }
    return map;
  }

  /** Absolute (zip-internal) path for an OPF-relative href. */
  resolve(href: string): string {
    return normalizePath(this.opfDir ? `${this.opfDir}/${href}` : href);
  }

  /** Reading-order XHTML documents (the actual chapters). */
  chapters(): SpineItem[] {
    return this.spine.filter((s) => s.mediaType === XHTML || s.href.endsWith(".xhtml") || s.href.endsWith(".html"));
  }

  async readText(href: string): Promise<string> {
    const f = this.zip.file(this.resolve(href));
    if (!f) {
      throw new Error(`entry not found: ${href}`);
    }
    return f.async("string");
  }

  async readBinary(href: string): Promise<Uint8Array> {
    const f = this.zip.file(this.resolve(href));
    if (!f) {
      throw new Error(`entry not found: ${href}`);
    }
    return f.async("uint8array");
  }

  async cover(): Promise<{ data: Uint8Array; mime: string } | undefined> {
    if (!this.coverHref) {
      return undefined;
    }
    try {
      const data = await this.readBinary(this.coverHref);
      return { data, mime: this.coverMime ?? "image/jpeg" };
    } catch {
      return undefined;
    }
  }

  /** Overwrite a chapter's XHTML source (href is OPF-relative). */
  writeChapter(href: string, content: string): void {
    this.zip.file(this.resolve(href), content);
  }

  /** Restore a previous OPF state (used for undo/redo of metadata edits). */
  restoreOpf(xml: string, metadata: Metadata): void {
    this.opfXml = xml;
    this.zip.file(this.opfPath, xml);
    this.metadata = metadata;
  }

  /** Apply a partial metadata edit, rewriting the OPF's <metadata> block. */
  updateMetadata(patch: Partial<Metadata>): void {
    const match = this.opfXml.match(/<metadata\b[\s\S]*?<\/metadata>/i);
    if (!match) {
      throw new Error("OPF has no <metadata> block");
    }
    let block = match[0];

    if (patch.title !== undefined) {
      block = setElementText(block, "title", patch.title);
    }
    if (patch.language !== undefined) {
      block = setElementText(block, "language", patch.language);
    }
    if (patch.publisher !== undefined) {
      block = setElementText(block, "publisher", patch.publisher);
    }
    if (patch.date !== undefined) {
      block = setElementText(block, "date", patch.date);
    }
    if (patch.description !== undefined) {
      block = setElementText(block, "description", patch.description);
    }
    if (patch.identifier !== undefined) {
      block = setElementText(block, "identifier", patch.identifier);
    }
    if (patch.creators !== undefined) {
      block = setCreators(block, patch.creators);
    }
    if (patch.subjects !== undefined) {
      block = setSubjects(block, patch.subjects);
    }
    block = touchModified(block);

    this.opfXml = this.opfXml.replace(match[0], block);
    this.zip.file(this.opfPath, this.opfXml);
    this.metadata = { ...this.metadata, ...patch };
  }

  async toBuffer(): Promise<Uint8Array> {
    return this.zip.generateAsync({
      type: "uint8array",
      mimeType: "application/epub+zip",
      compression: "DEFLATE",
    });
  }
}

/** Update the first `<[ns:]tag>text</[ns:]tag>` inside a metadata block, or insert one. */
function setElementText(block: string, tag: string, value: string): string {
  const re = new RegExp(`(<(?:\\w+:)?${tag}\\b[^>]*>)([\\s\\S]*?)(</(?:\\w+:)?${tag}>)`, "i");
  if (re.test(block)) {
    return block.replace(re, `$1${xmlEscape(value)}$3`);
  }
  return insertBeforeClose(block, `  <dc:${tag}>${xmlEscape(value)}</dc:${tag}>\n`);
}

function setCreators(block: string, creators: string[]): string {
  let out = block.replace(/[ \t]*<(?:\w+:)?creator\b[^>]*>[\s\S]*?<\/(?:\w+:)?creator>\s*\n?/gi, "");
  const inject = creators
    .filter((c) => c.trim().length > 0)
    .map((c) => `  <dc:creator>${xmlEscape(c.trim())}</dc:creator>\n`)
    .join("");
  return insertBeforeClose(out, inject);
}

function setSubjects(block: string, subjects: string[]): string {
  let out = block.replace(/[ \t]*<(?:\w+:)?subject\b[^>]*>[\s\S]*?<\/(?:\w+:)?subject>\s*\n?/gi, "");
  const inject = subjects
    .filter((s) => s.trim().length > 0)
    .map((s) => `  <dc:subject>${xmlEscape(s.trim())}</dc:subject>\n`)
    .join("");
  return insertBeforeClose(out, inject);
}

/** Refresh the EPUB3 dcterms:modified timestamp if present (required for EPUB3). */
function touchModified(block: string): string {
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  const re = /(<meta\b[^>]*property\s*=\s*["']dcterms:modified["'][^>]*>)([\s\S]*?)(<\/meta>)/i;
  if (re.test(block)) {
    return block.replace(re, `$1${now}$3`);
  }
  return block;
}

function insertBeforeClose(block: string, injection: string): string {
  if (!injection) {
    return block;
  }
  return block.replace(/<\/metadata>/i, `${injection}</metadata>`);
}
