const $ = (id) => document.getElementById(id);

const METHODS = {
  "direct": "The address given is the catalog itself.",
  "link-header": "Advertised by a Link header on the home page.",
  "html-link": "Advertised by a link element in the home page.",
  "well-known": "Served at the well-known URI.",
};

const TYPES = {
  "application/ai-catalog+json": "Nested catalog",
  "application/mcp-server-card+json": "MCP server card",
  "application/mcp-server+json": "MCP server description",
  "application/a2a-agent-card+json": "A2A agent card",
  "application/ai-skill": "Agent skill",
  "application/agent-skills+json": "Agent skills index",
  "application/openapi+json": "OpenAPI description",
  "application/linkset+json": "Link set",
  "text/markdown": "Markdown document",
  "text/plain": "Plain text",
  "text/html": "Web page",
};

function typeName(type) {
  return TYPES[type.split(";")[0].trim().toLowerCase()] || "Other";
}

function field(parent, label, value, cls) {
  const row = add(parent, "div", undefined, "field");
  add(row, "span", label, "label");
  add(row, "span", value, cls);
}

function add(parent, tag, text, cls) {
  const el = document.createElement(tag);
  if (text !== undefined) el.textContent = text;
  if (cls) el.className = cls;
  parent.appendChild(el);
  return el;
}

const MEANING = {
  trusted: "Trusted: a publisher signed a trust manifest for an entry, and the signature, the release it covers and the artifact bytes all checked out.",
  discoverable: "Discoverable: a valid catalog that names its publisher. Nothing in it is verified, which is fine; it just proves nothing about who published it.",
  minimal: "Minimal: the least a catalog can be. The document has errors, or it does not name its publisher.",
};

function pill(parent, text, state) {
  return add(parent, "span", text, "pill " + state);
}

function checkPill(parent, label, result, missing) {
  if (!result) return pill(parent, label + ": " + missing, "none");
  const word = { verified: "verified", failed: "failed", unverifiable: "could not be checked" }[result.status];
  return pill(parent, label + ": " + word, { verified: "ok", failed: "bad", unverifiable: "none" }[result.status]);
}

function reasonOf(report) {
  const trust = report.trust;
  const endorsed = report.entries.filter((e) => e.publisher_endorsed).length;
  const failed = report.entries.some((e) => e.manifests.some((m) => [m.signature, m.release].some((r) => r && r.status === "failed")) || (e.artifact && e.artifact.status === "failed"));
  const n = report.entries.length;
  if (trust.level === "trusted") return endorsed + " of " + n + (n === 1 ? " entry is" : " entries are") + " endorsed by " + (endorsed === 1 ? "its" : "their") + " publisher, and the checks passed.";
  if (trust.errors.length) return "The document has " + trust.errors.length + (trust.errors.length === 1 ? " error." : " errors.");
  if (trust.level === "minimal") return "The catalog does not name its publisher.";
  if (failed) return "A signature or digest check failed, so nothing is endorsed.";
  return "Valid, but no entry has a verified publisher endorsement.";
}

function showVerdict(report) {
  const box = $("verdict");
  box.hidden = false;
  const hints = $("hints");
  hints.replaceChildren();
  if (!report.found) {
    box.className = "verdict bad";
    $("level").textContent = "No catalog";
    $("reason").textContent = report.outcome;
    $("meaning").textContent = "A catalog may be advertised in a Link header, in the home page HTML, or served at /.well-known/ai-catalog.json. None was found here.";
    const guide = document.createElement("a");
    guide.href = "/publish";
    guide.textContent = "If this is your domain, see how to publish one.";
    $("meaning").append(" ", guide);
    return;
  }
  const trust = report.trust;
  box.className = "verdict " + (trust ? trust.level : "minimal");
  $("level").textContent = trust ? trust.level : "found";
  $("reason").textContent = trust ? reasonOf(report) : report.outcome;
  $("meaning").textContent = trust ? MEANING[trust.level] : "";
  if (!trust) return;
  for (const issue of trust.errors) add(hints, "li", issue.message + (issue.path ? " (" + issue.path + ")" : ""), "problem");
  for (const issue of trust.warnings) add(hints, "li", issue.message);
  for (const hint of report.hints) add(hints, "li", hint);
}

function resolve(reference, base) {
  try {
    return new URL(reference, base).href;
  } catch (error) {
    return null;
  }
}

async function getJson(path) {
  const response = await fetch(path);
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}

const INSPECTOR_RELEASES = "https://github.com/a2aproject/a2a-inspector/releases";

function isA2aCard(entry) {
  return /a2a-agent-card/i.test(entry.entry_type);
}

// The address comes from the catalog, so it is quoted: it may be pasted into a shell.
function shellQuote(text) {
  return "'" + text.replace(/'/g, "'\\''") + "'";
}

function inspectorRows(flag, address) {
  const quoted = shellQuote(address);
  return [
    ["With the binary", "a2a-inspector --port 8081 " + flag + " " + quoted],
    ["With Docker", "docker run --rm -p 127.0.0.1:8081:8080 ghcr.io/a2aproject/a2a-inspector " + flag + " " + quoted],
  ];
}

function mcpRows(remote) {
  const command = "npx @modelcontextprotocol/inspector --server-url " + shellQuote(remote.url) + " --transport " + remote.transport;
  return [["With Node.js", command]];
}

async function copyText(button, text) {
  const label = button.textContent;
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "Copied";
  } catch (error) {
    button.textContent = "Select and copy";
  }
  setTimeout(() => { button.textContent = label; }, 1500);
}

function fillInspector(panel, rows, app) {
  const intro = add(panel, "p", "The " + app.name + " is a separate tool that runs on your machine. Run one of these, then open the address it prints. ");
  const link = add(intro, "a", "Get it here.", "url");
  link.href = app.link;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  for (const [title, command] of rows) {
    add(panel, "div", title, "panel-title");
    add(panel, "pre", command, "raw");
    const copy = tool(add(panel, "div", undefined, "tools"), "Copy");
    copy.onclick = () => copyText(copy, command);
  }
}

const A2A_APP = { name: "A2A Inspector", link: INSPECTOR_RELEASES };
const MCP_APP = { name: "MCP Inspector", link: "https://github.com/modelcontextprotocol/inspector" };

function inspectorButton(tools, host, label, rows, app = A2A_APP) {
  const button = tool(tools, label);
  const panel = add(host, "div", undefined, "panel");
  panel.hidden = true;
  button.onclick = () => {
    if (!panel.childElementCount) fillInspector(panel, rows, app);
    panel.hidden = !panel.hidden;
  };
}

function addActions(li, entry, base, inlineText) {
  const address = entry.url ? resolve(entry.url, base) : null;
  if (entry.nested) {
    if (!address) return;
    const link = add(add(li, "div", undefined, "tools action"), "a", "Explore this catalog →", "tool link");
    link.href = "/discover?domain=" + encodeURIComponent(address);
    return;
  }
  if (!entry.inspection && !address) return;
  const tools = add(li, "div", undefined, "tools action");
  const button = tool(tools, "Inspect");
  const panel = add(li, "div", undefined, "panel");
  panel.hidden = true;
  let loaded = false;
  button.onclick = async () => {
    panel.hidden = !panel.hidden;
    if (panel.hidden || loaded) return;
    loaded = true;
    panel.textContent = "Loading…";
    try {
      const inspection = entry.inspection || (await getJson(inspectPath(entry, address)));
      panel.replaceChildren();
      renderInspection(panel, inspection, inlineText, isA2aCard(entry) ? address : null);
    } catch (error) {
      panel.replaceChildren();
      add(panel, "div", String(error.message || error), "status-bad");
      loaded = false;
    }
  };
}

function inspectPath(entry, address) {
  const query = new URLSearchParams({ url: address, type: entry.entry_type });
  if (entry.digest) query.set("digest", entry.digest);
  return "/api/inspect?" + query;
}

function renderInspection(panel, inspection, inlineText, a2aAddress) {
  if (inspection.error) {
    add(panel, "div", "Could not read it: " + inspection.error, "status-bad");
    return;
  }
  const pills = add(panel, "div", undefined, "pills");
  if (inspection.digest) checkPill(pills, "Digest", inspection.digest, "");
  else pill(pills, "No digest stated", "none");
  pill(pills, inspection.size.toLocaleString() + " bytes", "none");
  if (inspection.digest && inspection.digest.reason) add(panel, "div", inspection.digest.reason, "reason-line");
  for (const f of inspection.fields) field(panel, f.label, f.value);
  if (inspection.items.length) {
    add(panel, "div", inspection.items_title, "panel-title");
    const list = add(panel, "ul", undefined, "list compact");
    for (const item of inspection.items) {
      const row = add(list, "li");
      add(row, "span", item.title, "item-title");
      if (item.detail) add(row, "span", item.detail, "meta");
    }
  }
  if (a2aAddress && a2aAddress.startsWith("https://")) {
    inspectorButton(add(panel, "div", undefined, "tools"), panel, "Test in A2A Inspector", inspectorRows("-a", a2aAddress));
  }
  for (const remote of inspection.remotes || []) {
    const tools = add(panel, "div", undefined, "tools");
    inspectorButton(tools, panel, "Test " + remote.url + " in MCP Inspector", mcpRows(remote), MCP_APP);
  }
  // An inline artifact is shown in full from the catalog the page already has.
  const text = inlineText || inspection.preview;
  const truncated = inlineText ? false : inspection.truncated;
  if (text) {
    const box = add(panel, "details");
    box.open = true;
    const summary = add(box, "summary");
    add(summary, "h2", inspection.kind === "other" ? "The content" : "The document");
    add(summary, "span", text.length.toLocaleString() + " characters", "count");
    documentViewer(add(box, "div", undefined, "body"), text, truncated, null, "Plain text");
  }
}

function showEntry(li, entry, base, inlineText) {
  add(li, "div", entry.display_name || entry.identifier, "name");
  field(li, "Identifier", entry.identifier, "url");
  field(li, "Kind", typeName(entry.entry_type) + " (" + entry.entry_type + ")");
  const where = entry.nested
    ? ["Content", "another catalog"]
    : entry.inline
      ? ["Content", "included in the catalog itself"]
      : entry.url
        ? ["Content at", entry.url]
        : ["Content", "no address given"];
  field(li, where[0], where[1], entry.url ? "url" : undefined);
  const pills = add(li, "div", undefined, "pills");
  if (!entry.manifests.length) pill(pills, "No trust information", "none");
  if (entry.publisher_endorsed) pill(pills, "Publisher endorsed", "ok");
  entry.manifests.forEach((m) => {
    const who = m.publisher ? "Publisher" : "Assessor";
    checkPill(pills, who + " signature", m.signature, "unsigned");
    if (m.signature) checkPill(pills, who + " covers release", m.release, "not checked");
  });
  if (entry.artifact) checkPill(pills, "Artifact bytes", entry.artifact, "");
  const reasons = [];
  for (const m of entry.manifests) for (const r of [m.signature, m.release]) if (r && r.reason) reasons.push(r.reason);
  if (entry.artifact && entry.artifact.reason) reasons.push(entry.artifact.reason);
  for (const reason of reasons) add(li, "div", reason, "reason-line");
  addActions(li, entry, base, inlineText);
}

const OPEN_DEPTH = 2;

function span(parent, text, cls) {
  return add(parent, "span", text, cls);
}

// The check result of the signature at `path`, from the verification report.
function signatureResult(report, path) {
  if (path.length === 1) return report.trust && report.trust.catalog_signature;
  if (path[0] === "entries" && path[2] === "trustManifests" && path.length === 5) {
    const entry = report.entries[path[1]];
    const manifest = entry && entry.manifests[path[3]];
    return manifest && manifest.signature;
  }
  return null;
}

function primitive(parent, value) {
  if (typeof value === "string") span(parent, JSON.stringify(value), "j-str");
  else if (typeof value === "number") span(parent, String(value), "j-num");
  else span(parent, String(value), "j-lit");
}

function label(parent, key) {
  if (typeof key !== "string") return;
  span(parent, JSON.stringify(key), "j-key");
  span(parent, ": ", "j-punc");
}

function node(parent, value, key, path, depth, comma, report) {
  const container = value !== null && typeof value === "object";
  const isArray = Array.isArray(value);
  const members = container ? (isArray ? value.map((v, i) => [i, v]) : Object.entries(value)) : [];
  if (!container || members.length === 0) {
    const line = add(parent, "div", undefined, "line");
    label(line, key);
    if (container) span(line, isArray ? "[]" : "{}", "j-punc");
    else primitive(line, value);
    span(line, comma, "j-punc");
    return;
  }
  const open = isArray ? "[" : "{";
  const close = isArray ? "]" : "}";
  const tagged = key === "signature" && report;
  const box = add(parent, "details", undefined, tagged ? "signature" : undefined);
  box.open = depth < OPEN_DEPTH;
  const summary = add(box, "summary", undefined, "line");
  label(summary, key);
  span(summary, open, "j-punc");
  span(summary, " … " + close + comma + "  " + members.length + (members.length === 1 ? " item" : " items"), "hint");
  if (tagged) {
    const result = signatureResult(report, path);
    if (result) {
      box.classList.add(result.status);
      const text = { verified: "signature verified", failed: "signature failed", unverifiable: "signature could not be checked" }[result.status];
      const chip = span(summary, text, "chip");
      if (result.reason) chip.title = result.reason;
    }
  }
  const children = add(box, "div", undefined, "children");
  members.forEach(([k, v], i) => node(children, v, isArray ? undefined : k, path.concat([k]), depth + 1, i < members.length - 1 ? "," : "", report));
  const end = add(box, "div", undefined, "line close");
  span(end, close + comma, "j-punc");
}

function tool(parent, text) {
  const button = add(parent, "button", text, "tool");
  button.type = "button";
  return button;
}

// A document as a collapsible tree, or as plain text, with copy and expand controls.
// `report` is optional; with it, signature objects show their verification result.
function documentViewer(host, text, truncated, report, plainLabel) {
  host.replaceChildren();
  let parsed = null;
  if (!truncated) {
    try {
      parsed = JSON.parse(text);
    } catch (error) {
      parsed = null;
    }
  }
  const tools = add(host, "div", undefined, "tools");
  const treeButton = tool(tools, "Tree");
  const plainButton = tool(tools, plainLabel);
  add(tools, "span", undefined, "gap");
  const expand = tool(tools, "Expand all");
  const collapse = tool(tools, "Collapse all");
  const copy = tool(tools, "Copy");
  const tree = add(host, "div", undefined, "tree");
  const pre = add(host, "pre", text, "raw");
  const note = add(host, "p", "The document is large, so only the first part is shown.", "sub");
  note.hidden = !truncated;
  if (parsed !== null) node(tree, parsed, undefined, [], 0, "", report);

  const choose = (asTree) => {
    asTree = asTree && parsed !== null;
    tree.hidden = !asTree;
    pre.hidden = asTree;
    treeButton.setAttribute("aria-pressed", String(asTree));
    plainButton.setAttribute("aria-pressed", String(!asTree));
    expand.hidden = collapse.hidden = !asTree;
  };
  treeButton.disabled = parsed === null;
  treeButton.onclick = () => choose(true);
  plainButton.onclick = () => choose(false);
  const toggleAll = (open) => tree.querySelectorAll("details").forEach((d) => (d.open = open));
  expand.onclick = () => toggleAll(true);
  collapse.onclick = () => toggleAll(false);
  copy.onclick = async () => {
    const value = parsed !== null && !tree.hidden ? JSON.stringify(parsed, null, 2) : text;
    try {
      await navigator.clipboard.writeText(value);
      copy.textContent = "Copied";
    } catch (error) {
      copy.textContent = "Copy failed";
    }
    setTimeout(() => (copy.textContent = "Copy"), 1500);
  };
  choose(true);
}

function showRaw(report) {
  const section = $("raw-section");
  section.hidden = !report.found || report.raw === null;
  if (section.hidden) return;
  documentViewer($("raw-viewer"), report.raw, report.raw_truncated, report, "As served");
  $("raw-count").textContent = report.raw.length.toLocaleString() + " characters";
}

// The catalog as served, parsed; null when it is too large to have been sent whole.
function parsedDocument(report) {
  if (!report.raw || report.raw_truncated) return null;
  try {
    return JSON.parse(report.raw);
  } catch (error) {
    return null;
  }
}

function show(report) {
  $("title").textContent = report.host || report.input || "Discovery";
  $("subtitle").textContent = report.found ? (METHODS[report.method] || "Found") + " " + report.url : "Nothing to show";
  showVerdict(report);
  showRaw(report);
  $("notice").hidden = !report.found;

  $("entries-section").hidden = !report.found;
  $("entries-count").textContent = report.entries.length + (report.entries.length === 1 ? " entry" : " entries");
  const entries = $("entries");
  entries.replaceChildren();
  if (report.found && !report.entries.length) add(add(entries, "li", undefined, "item"), "div", "Nothing listed.", "meta");
  const served = parsedDocument(report);
  report.entries.forEach((entry, i) => {
    const data = served && served.entries && served.entries[i] && served.entries[i].data;
    const inlineText = data === undefined || data === null ? undefined : JSON.stringify(data, null, 2);
    showEntry(add(entries, "li", undefined, "item"), entry, report.url, inlineText);
  });

  $("trace-section").hidden = report.trace.length === 0;
  $("trace-count").textContent = report.trace.length + (report.trace.length === 1 ? " request" : " requests");
  const trace = $("trace");
  trace.replaceChildren();
  report.trace.forEach((hop, i) => {
    const li = add(trace, "li", undefined, "item");
    field(li, "Request " + (i + 1), hop.url, "url");
    if (hop.final_url) field(li, "Redirected to", hop.final_url, "url");
    const failed = hop.error !== null || hop.status >= 400;
    if (hop.error !== null) {
      field(li, "Result", "no response: " + hop.error, "status-bad");
    } else {
      field(li, "Result", "HTTP " + hop.status + (hop.status >= 400 ? " (nothing served here)" : ""), failed ? "status-bad" : "status-ok");
      if (hop.content_type) field(li, "Content type", hop.content_type);
    }
    for (const link of hop.link_headers) field(li, "Link header", link, "url");
    if (hop.note) field(li, "Verdict", hop.note);
  });
}

async function run() {
  const domain = (new URLSearchParams(location.search).get("domain") || "").trim();
  if (!domain) {
    location.replace("/");
    return;
  }
  document.title = domain + " · AI Catalog Explorer";
  $("title").textContent = domain;
  try {
    const response = await fetch("/api/discover?domain=" + encodeURIComponent(domain));
    if (!response.ok) throw new Error(await response.text());
    show(await response.json());
  } catch (error) {
    show({ input: domain, found: false, outcome: String(error.message || error), trace: [], entries: [], trust: null, hints: [], raw: null, raw_truncated: false });
  }
}

run();
