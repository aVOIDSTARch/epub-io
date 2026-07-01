// EPUB Studio webview controller. Runs sandboxed; talks to the extension host
// via postMessage. No external scripts, no eval.
(function () {
  const vscode = acquireVsCodeApi();

  const $ = (id) => document.getElementById(id);
  const form = $("metaForm");
  const spineList = $("spine");
  const reader = $("reader");
  const rendered = $("rendered");
  const source = $("source");
  const readerTitle = $("readerTitle");
  let currentHref = null;

  // Toolbar buttons dispatch extension commands.
  document.querySelectorAll("[data-cmd]").forEach((btn) => {
    btn.addEventListener("click", () =>
      vscode.postMessage({ type: "command", command: btn.getAttribute("data-cmd") }),
    );
  });

  form.addEventListener("submit", (e) => {
    e.preventDefault();
    const data = new FormData(form);
    const list = (v) => String(v || "").split(",").map((s) => s.trim()).filter(Boolean);
    vscode.postMessage({
      type: "saveMetadata",
      patch: {
        title: String(data.get("title") || ""),
        creators: list(data.get("creators")),
        language: String(data.get("language") || ""),
        identifier: String(data.get("identifier") || ""),
        publisher: String(data.get("publisher") || ""),
        date: String(data.get("date") || ""),
        subjects: list(data.get("subjects")),
        description: String(data.get("description") || ""),
      },
    });
  });

  $("closeReader").addEventListener("click", () => {
    reader.hidden = true;
    currentHref = null;
  });

  $("toggleSource").addEventListener("click", () => {
    const editing = source.hidden;
    source.hidden = !editing;
    rendered.hidden = editing;
    $("saveChapter").hidden = !editing;
    $("toggleSource").textContent = editing ? "Preview" : "Edit Source";
  });

  $("saveChapter").addEventListener("click", () => {
    if (currentHref !== null) {
      vscode.postMessage({ type: "saveChapter", href: currentHref, content: source.value });
    }
  });

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (msg.type === "state") {
      renderState(msg);
    } else if (msg.type === "chapter") {
      showChapter(msg.href, msg.raw);
    }
  });

  function renderState(state) {
    $("fileName").textContent = state.fileName || "EPUB";
    const cover = $("cover");
    if (state.cover) {
      cover.src = state.cover;
      cover.hidden = false;
    } else {
      cover.hidden = true;
    }

    const m = state.metadata || {};
    setField("title", m.title);
    setField("creators", (m.creators || []).join(", "));
    setField("language", m.language);
    setField("identifier", m.identifier);
    setField("publisher", m.publisher);
    setField("date", m.date);
    setField("subjects", (m.subjects || []).join(", "));
    setField("description", m.description);

    const spine = state.spine || [];
    $("spineCount").textContent = String(spine.length);
    spineList.innerHTML = "";
    spine.forEach((s) => {
      const li = document.createElement("li");
      li.textContent = s.title || s.href;
      li.title = s.href;
      li.addEventListener("click", () => vscode.postMessage({ type: "openChapter", href: s.href }));
      spineList.appendChild(li);
    });
  }

  function setField(name, value) {
    const el = form.elements.namedItem(name);
    if (el) {
      el.value = value || "";
    }
  }

  function showChapter(href, raw) {
    currentHref = href;
    reader.hidden = false;
    readerTitle.textContent = href;
    source.value = raw;
    source.hidden = true;
    $("saveChapter").hidden = true;
    rendered.hidden = false;
    $("toggleSource").textContent = "Edit Source";
    // Render the body without executing scripts. Assigning innerHTML does not
    // run <script> tags, and the CSP forbids inline execution regardless.
    const body = raw.match(/<body[^>]*>([\s\S]*?)<\/body>/i);
    const html = (body ? body[1] : raw).replace(/<(script|style)[\s\S]*?<\/\1>/gi, "");
    rendered.innerHTML = html;
  }

  vscode.postMessage({ type: "ready" });
})();
