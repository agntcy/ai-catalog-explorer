"use strict";

const $ = (id) => document.getElementById(id);
const WELL_KNOWN = "/.well-known/ai-catalog.json";
const MEDIA_TYPE = "application/ai-catalog+json";

const TYPES = [
  { type: "application/mcp-server-card+json", label: "MCP server card", ns: "mcp" },
  { type: "application/a2a-agent-card+json", label: "A2A agent card", ns: "a2a" },
  { type: "application/agent-skills+zip", label: "Agent skill (zip)", ns: "skill" },
  { type: "application/agent-plugins+zip", label: "Agent plugin (zip)", ns: "agent" },
  { type: "application/ai-catalog+json", label: "Another catalog", ns: "catalog" },
];

const HOSTS = [
  {
    id: "nginx",
    label: "nginx",
    serve: {
      note: "Put the file at .well-known/ai-catalog.json under your web root, then add this to your server block. The empty types block makes nginx use the media type below instead of application/json.",
      code: (d) => [
        "location = " + WELL_KNOWN + " {",
        "    root /var/www/" + d + ";",
        "    types { }",
        "    default_type " + MEDIA_TYPE + ";",
        "}",
      ],
    },
    link: {
      note: "Add this to the same server block. It is sent with every response, the home page included.",
      code: (d) => ["add_header Link '<" + url(d) + ">; rel=\"ai-catalog\"' always;"],
    },
  },
  {
    id: "apache",
    label: "Apache",
    serve: {
      note: "Put the file at .well-known/ai-catalog.json under your document root, then add this to your virtual host.",
      code: () => [
        "<Location \"" + WELL_KNOWN + "\">",
        "    ForceType " + MEDIA_TYPE,
        "</Location>",
      ],
    },
    link: {
      note: "Needs mod_headers. Add this to the same virtual host.",
      code: (d) => ["Header always set Link \"<" + url(d) + ">; rel=\\\"ai-catalog\\\"\""],
    },
  },
  {
    id: "caddy",
    label: "Caddy",
    serve: {
      note: "Put the file at .well-known/ai-catalog.json under the site root, then use a Caddyfile like this.",
      code: (d) => [
        d + " {",
        "    root * /var/www/" + d,
        "    @catalog path " + WELL_KNOWN,
        "    header @catalog Content-Type " + MEDIA_TYPE,
        "    file_server",
        "}",
      ],
    },
    link: {
      note: "Add this line inside the same site block.",
      code: (d) => ["header Link `<" + url(d) + ">; rel=\"ai-catalog\"`"],
    },
  },
  {
    id: "pages",
    label: "Netlify or Cloudflare Pages",
    serve: {
      note: "Put the file at .well-known/ai-catalog.json in your publish folder, and add a file named _headers next to your index page with this content.",
      code: () => ["" + WELL_KNOWN, "  Content-Type: " + MEDIA_TYPE],
    },
    link: {
      note: "Add this to the same _headers file.",
      code: (d) => ["/", "  Link: <" + url(d) + ">; rel=\"ai-catalog\""],
    },
  },
  {
    id: "github",
    label: "GitHub Pages",
    serve: {
      note: "Commit the file as .well-known/ai-catalog.json, plus an empty file named .nojekyll. Without it, Jekyll skips folders that start with a dot. GitHub Pages serves the file as application/json, which is accepted.",
      code: () => [
        "mkdir -p .well-known",
        "cp ai-catalog.json .well-known/ai-catalog.json",
        "touch .nojekyll",
        "git add .nojekyll .well-known",
        "git commit -m \"Publish AI Catalog\" && git push",
      ],
    },
    link: {
      note: "GitHub Pages cannot set HTTP headers. Use the HTML link above.",
      code: null,
    },
  },
];

let rows = [{ type: TYPES[0].type, name: "", url: "" }];
let selected = HOSTS[0];
let timer;
let run = 0;

function cleanDomain(input) {
  const value = input.trim().toLowerCase().replace(/^[a-z]+:\/\//, "").split(/[/?#]/)[0];
  return /^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$/.test(value) && value.includes(".") ? value : "";
}

function domain() {
  return cleanDomain($("domain").value) || "example.com";
}

function url(d) {
  return "https://" + d + WELL_KNOWN;
}

function slug(text) {
  return text.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

function entryName(row) {
  const fromUrl = row.url.split(/[?#]/)[0].split("/").filter(Boolean).pop() || "";
  return slug(row.name) || slug(fromUrl.replace(/\.[a-z0-9]+$/, "")) || "entry";
}

function catalog() {
  const out = { specVersion: "1.0" };
  const name = $("host").value.trim();
  if (name) out.host = { displayName: name };
  const d = domain();
  out.entries = rows
    .filter((row) => row.name.trim() || row.url.trim())
    .map((row) => {
      const kind = TYPES.find((t) => t.type === row.type);
      return { identifier: "urn:air:" + d + ":" + kind.ns + ":" + entryName(row), type: row.type, url: row.url.trim() };
    });
  return JSON.stringify(out, null, 2);
}

function add(parent, tag, text, cls) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (cls) node.className = cls;
  parent.appendChild(node);
  return node;
}

function renderRows() {
  const list = $("rows");
  list.replaceChildren();
  rows.forEach((row, index) => {
    const li = add(list, "li", undefined, "row");
    const select = add(li, "select");
    select.setAttribute("aria-label", "Entry type");
    for (const t of TYPES) {
      const option = add(select, "option", t.label);
      option.value = t.type;
      option.selected = t.type === row.type;
    }
    select.addEventListener("change", () => { row.type = select.value; update(); });
    const name = add(li, "input", undefined, "name");
    name.placeholder = "short name, e.g. weather";
    name.setAttribute("aria-label", "Short name");
    name.value = row.name;
    name.addEventListener("input", () => { row.name = name.value; update(); });
    const link = add(li, "input", undefined, "target");
    link.placeholder = "https://example.com/mcp/server-card";
    link.setAttribute("aria-label", "Address of the artifact");
    link.value = row.url;
    link.addEventListener("input", () => { row.url = link.value; update(); });
    const remove = add(li, "button", "×", "tool");
    remove.type = "button";
    remove.title = "Remove this entry";
    remove.setAttribute("aria-label", "Remove this entry");
    remove.addEventListener("click", () => {
      rows.splice(index, 1);
      renderRows();
      update();
    });
  });
}

function code(id, lines) {
  $(id).textContent = lines.join("\n");
}

function renderSnippets() {
  const d = domain();
  for (const node of document.querySelectorAll(".domain")) node.textContent = d;
  code("html-code", ['<link rel="ai-catalog" href="' + WELL_KNOWN + '"', '      type="' + MEDIA_TYPE + '">']);
  $("serve-note").textContent = selected.serve.note;
  code("serve-code", selected.serve.code(d));
  $("link-note").textContent = selected.link.note;
  const header = selected.link.code;
  $("link-code").hidden = !header;
  $("link-tools").hidden = !header;
  if (header) code("link-code", header(d));
  code("curl-code", [
    "curl -sI " + url(d) + " | grep -i -E '^(HTTP|content-type)'",
    "curl -sI https://" + d + "/ | grep -i '^link:'",
  ]);
  const valid = cleanDomain($("domain").value);
  $("check").href = valid ? "/discover?domain=" + encodeURIComponent(valid) : "/";
  $("check").textContent = valid ? "Check " + valid : "Check your domain";
}

function renderHosts() {
  const bar = $("hosts");
  bar.replaceChildren();
  for (const host of HOSTS) {
    const button = add(bar, "button", host.label, "tool");
    button.type = "button";
    button.setAttribute("aria-pressed", String(host === selected));
    button.addEventListener("click", () => {
      selected = host;
      renderHosts();
      renderSnippets();
    });
  }
}

function showResult(result) {
  const state = $("draft-state");
  const issues = $("issues");
  issues.replaceChildren();
  if (result.valid) {
    state.textContent = "valid · " + result.level;
    state.className = "count status-ok";
  } else {
    const n = result.errors.length;
    state.textContent = n + (n === 1 ? " problem" : " problems");
    state.className = "count status-bad";
  }
  for (const issue of result.errors) add(issues, "li", issue.message + (issue.path ? " (" + issue.path + ")" : ""), "problem");
  for (const issue of result.warnings) add(issues, "li", issue.message);
  if (result.valid && result.level === "minimal") add(issues, "li", "Add your name above to name the publisher. That makes the catalog Discoverable.");
}

async function check(text, token) {
  try {
    const response = await fetch("/api/validate", { method: "POST", body: text });
    if (!response.ok) throw new Error(response.statusText);
    const result = await response.json();
    if (token === run) showResult(result);
  } catch (error) {
    if (token === run) $("draft-state").textContent = "could not check";
  }
}

function update() {
  const text = catalog();
  $("catalog").textContent = text;
  renderSnippets();
  clearTimeout(timer);
  const token = ++run;
  timer = setTimeout(() => check(text, token), 250);
}

async function copy(button, text) {
  const label = button.textContent;
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "Copied";
  } catch (error) {
    button.textContent = "Select and copy";
  }
  setTimeout(() => { button.textContent = label; }, 1500);
}

function download() {
  const blob = new Blob([catalog() + "\n"], { type: MEDIA_TYPE });
  const link = document.createElement("a");
  link.href = URL.createObjectURL(blob);
  link.download = "ai-catalog.json";
  link.click();
  URL.revokeObjectURL(link.href);
}

$("domain").addEventListener("input", update);
$("host").addEventListener("input", update);
$("add").addEventListener("click", () => {
  rows.push({ type: TYPES[0].type, name: "", url: "" });
  renderRows();
  update();
});
$("copy-catalog").addEventListener("click", (event) => copy(event.currentTarget, catalog() + "\n"));
$("download").addEventListener("click", download);
for (const button of document.querySelectorAll("[data-copy]")) {
  button.addEventListener("click", () => copy(button, $(button.dataset.copy).textContent + "\n"));
}

renderHosts();
renderRows();
update();
