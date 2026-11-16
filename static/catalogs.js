// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

"use strict";

const $ = (id) => document.getElementById(id);

const TYPES = {
  "application/ai-catalog+json": "Nested catalog",
  "application/mcp-server-card+json": "MCP server card",
  "application/mcp-server+json": "MCP server description",
  "application/a2a-agent-card+json": "A2A agent card",
  "application/ai-skill": "Agent skill",
  "application/agent-skills+json": "Agent skills index",
  "application/agent-skills+zip": "Agent skill (zip)",
  "application/openapi+json": "OpenAPI description",
  "application/linkset+json": "Link set",
  "text/markdown": "Markdown document",
  "text/plain": "Plain text",
  "text/html": "Web page",
};
const METHODS = { "well-known": "Well-known", "link-header": "Link header", "html-link": "HTML link", direct: "Listed by address" };
const LEVELS = [
  ["trusted", "Trusted"],
  ["discoverable", "Discoverable"],
  ["minimal", "Minimal"],
  ["invalid", "Invalid"],
  ["unreachable", "Unreachable"],
];
const SHOWN_ITEMS = 5;

function typeName(type) {
  const base = type.split(";")[0].trim().toLowerCase();
  return TYPES[base] || base;
}

function add(parent, tag, text, cls) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (cls) node.className = cls;
  parent.appendChild(node);
  return node;
}

function ago(seconds) {
  if (!seconds) return "never";
  const s = Math.max(0, Math.floor(Date.now() / 1000) - seconds);
  if (s < 90) return "just now";
  if (s < 5400) return Math.round(s / 60) + " min ago";
  if (s < 129600) return Math.round(s / 3600) + " h ago";
  return Math.round(s / 86400) + " days ago";
}

function stat(parent, number, label) {
  const box = add(parent, "div", undefined, "stat");
  add(box, "div", String(number), "n");
  add(box, "div", label, "l");
}

function meters(host, rows, total) {
  host.replaceChildren();
  if (!rows.length) add(host, "p", "Nothing yet.", "sub");
  for (const row of rows) {
    const line = add(host, "div", undefined, "mrow");
    add(line, "span", row.label, "mlabel").title = row.label;
    const track = add(line, "span", undefined, "track");
    const fill = add(track, "span", undefined, "fill " + (row.cls || ""));
    fill.style.width = (total ? Math.max(2, Math.round((row.count / total) * 100)) : 0) + "%";
    add(line, "span", String(row.count), "mcount");
  }
}

function showStats(stats) {
  const box = $("stats");
  box.replaceChildren();
  stat(box, stats.catalogs, "catalogs");
  stat(box, stats.entries, "entries listed");
  stat(box, stats.levels.trusted || 0, "trusted catalogs");
  stat(box, stats.endorsed, "publisher-endorsed entries");
  $("lead").textContent = stats.catalogs
    ? "Catalogs the explorer knows about, each checked from its own domain. Last check " + ago(stats.last_checked) + "."
    : "No catalogs yet. The first check is still running, or none were found.";
  meters($("by-level"), LEVELS.filter(([key]) => stats.levels[key]).map(([key, label]) => ({ label, count: stats.levels[key], cls: key })), stats.catalogs);
  meters($("by-type"), stats.types.slice(0, 8).map(([type, count]) => ({ label: typeName(type), count })), stats.entries);
  const methods = Object.entries(stats.methods).sort((a, b) => b[1] - a[1]);
  meters($("by-method"), methods.map(([key, count]) => ({ label: METHODS[key] || key, count })), stats.catalogs);
  const select = $("type");
  const chosen = select.value;
  select.replaceChildren();
  add(select, "option", "Any entry type").value = "";
  for (const [type, count] of stats.types) {
    const option = add(select, "option", typeName(type) + " (" + count + ")");
    option.value = type;
  }
  select.value = chosen;
}

function entryName(item) {
  return item.name || item.identifier.split(":").pop() || item.identifier;
}

function card(record) {
  const li = document.createElement("li");
  li.className = "item";
  const title = add(li, "div", undefined, "name");
  const link = add(title, "a", record.host || record.input);
  link.href = "/discover?domain=" + encodeURIComponent(record.catalog_url || record.input);
  add(li, "div", record.catalog_url || record.input, "url meta");
  const pills = add(li, "div", undefined, "pills");
  add(pills, "span", (LEVELS.find(([key]) => key === record.rating) || [0, record.rating])[1], "pill " + record.rating);
  if (record.method) add(pills, "span", METHODS[record.method] || record.method, "pill none");
  if (record.status === "ok") add(pills, "span", record.items.length + (record.items.length === 1 ? " entry" : " entries"), "pill none");
  add(pills, "span", "checked " + ago(record.checked), "pill none");
  if (record.reason) add(li, "div", record.reason, "reason-line");
  const list = add(li, "ul", undefined, "list compact entries");
  for (const item of record.items.slice(0, SHOWN_ITEMS)) {
    const row = add(list, "li");
    add(row, "span", typeName(item.entry_type), "kind");
    add(row, "span", " " + entryName(item));
    if (item.endorsed) add(row, "span", "publisher-endorsed", "pill ok tiny");
  }
  if (record.items.length > SHOWN_ITEMS) add(li, "div", "and " + (record.items.length - SHOWN_ITEMS) + " more", "meta more");
  return li;
}

let run = 0;
let timer;

async function load() {
  const token = ++run;
  const params = new URLSearchParams({ q: $("q").value, type: $("type").value, level: $("level").value, method: $("method").value });
  try {
    const response = await fetch("/api/index?" + params);
    if (!response.ok) throw new Error(response.statusText);
    const answer = await response.json();
    if (token !== run) return;
    showStats(answer.stats);
    const list = $("results");
    list.replaceChildren();
    for (const record of answer.records) list.appendChild(card(record));
    const n = answer.records.length;
    $("result-count").textContent = n === 1 ? "1 catalog" : n + " catalogs";
  } catch (error) {
    if (token === run) $("result-count").textContent = "Could not load the index.";
  }
}

async function submit() {
  const domain = $("submit-domain").value.trim();
  const out = $("submit-result");
  if (!domain) return;
  const button = $("submit-go");
  button.disabled = true;
  out.textContent = "Checking " + domain + "…";
  try {
    const response = await fetch("/api/index/submit?domain=" + encodeURIComponent(domain), { method: "POST" });
    if (!response.ok) throw new Error(await response.text());
    const result = await response.json();
    out.textContent = result.message;
    if (result.added) {
      $("submit-domain").value = "";
      load();
    }
  } catch (error) {
    out.textContent = String(error.message || error);
  } finally {
    button.disabled = false;
  }
}

$("q").addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(load, 200); });
for (const id of ["type", "level", "method"]) $(id).addEventListener("change", load);
$("submit-go").addEventListener("click", submit);
$("submit-domain").addEventListener("keydown", (event) => { if (event.key === "Enter") submit(); });

load();
setInterval(load, 30000);
