import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import Sigma from "sigma";

type NodeKind = "source" | "unread_source" | "asset";

type KogNode = {
  id: string;
  path: string;
  lang: string;
  kind: NodeKind;
  loc: number;
  bytes: number;
  external_deps: string[];
};

type KogEdge = { source: string; target: string; kind: string };

type KogFailure = { path: string; reason: string };

type KogDiagnostic = {
  path: string;
  line: number;
  specifier: string;
  kind: "unresolved" | "excluded";
  reason: string;
  lang: string;
};

type KogLangStats = {
  files: number;
  resolved: number;
  unresolved: number;
  excluded: number;
  resolution_rate: number;
  edges: number;
};

type KogExtensionCoverage = {
  extension: string;
  count: number;
  status: "analysed" | "unsupported_language" | "not_source" | "failed";
  lang: string | null;
  note: string | null;
};

type KogCoverage = {
  files_seen: number;
  files_analysed: number;
  files_unsupported: number;
  files_not_source: number;
  extensions: KogExtensionCoverage[];
  skipped_directories: { name: string; count: number; rule: string }[];
};

type KogStats = {
  files_discovered: number;
  files_parsed: number;
  specifiers_total: number;
  specifiers_internal: number;
  resolved: number;
  unresolved: number;
  excluded: number;
  resolution_rate: number;
  external_specifiers: number;
  external_packages_distinct: number;
  failures: KogFailure[];
  diagnostics: KogDiagnostic[];
  by_lang: Record<string, KogLangStats>;
  coverage: KogCoverage;
};

type KogGraph = {
  nodes: KogNode[];
  edges: KogEdge[];
  stats: KogStats;
};

type KogProject = {
  id: string;
  name: string;
  path: string;
  kinds: string[];
  graph: KogGraph;
};

type KogWorkspace = {
  root: string;
  split: boolean;
  projects: KogProject[];
  totals: {
    projects: number;
    nodes: number;
    edges: number;
    files_analysed: number;
    files_unsupported: number;
    resolution_rate: number;
    source_coverage: number;
  };
  unassigned_files: number;
};

/**
 * Sigma's WebGL renderer parses node colours itself (see
 * `parseColor`/`floatColor` in the `sigma` package) and only understands
 * `#hex` and `rgb()`/`rgba()` — an `hsl()` string silently falls through to
 * black. Convert to hex ourselves so the colour actually reaches the canvas.
 */
function hslToHex(h: number, s: number, l: number): string {
  const sat = s / 100;
  const lig = l / 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = sat * Math.min(lig, 1 - lig);
  const f = (n: number) => lig - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const toHex = (n: number) =>
    Math.round(f(n) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${toHex(0)}${toHex(8)}${toHex(4)}`;
}

/**
 * Stable colour per directory, using the two leading path segments rather
 * than one. On a monorepo every `apps/*` shares a first segment, so colouring
 * by it wastes the only visual channel that carries structure: `apps/web` and
 * `apps/api` came out the same colour while being the two halves of the map a
 * reader most needs to tell apart.
 */
function colourFor(id: string): string {
  const segments = id.split("/");
  const key = segments.length > 2 ? `${segments[0]}/${segments[1]}` : segments[0];
  let hash = 0;
  for (let i = 0; i < key.length; i++) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0;
  }
  return hslToHex(hash % 360, 65, 55);
}

/** Assets and unreadable languages are drawn back, so code reads first. */
const KIND_STYLE: Record<NodeKind, { alpha: number; scale: number }> = {
  source: { alpha: 1, scale: 1 },
  unread_source: { alpha: 0.75, scale: 0.85 },
  asset: { alpha: 0.35, scale: 0.6 },
};

function fade(hex: string, alpha: number): string {
  if (alpha >= 1) return hex;
  const value = parseInt(hex.slice(1), 16);
  const mix = (channel: number) => Math.round(channel * alpha + 0x18 * (1 - alpha));
  const r = mix((value >> 16) & 0xff);
  const g = mix((value >> 8) & 0xff);
  const b = mix(value & 0xff);
  return `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

function buildGraph(data: KogGraph, showAssets: boolean): Graph {
  const graph = new Graph();
  for (const node of data.nodes) {
    if (!showAssets && node.kind === "asset") continue;
    const style = KIND_STYLE[node.kind] ?? KIND_STYLE.source;
    graph.addNode(node.id, {
      label: node.id.split("/").pop() ?? node.id,
      size: 2 * style.scale,
      color: fade(colourFor(node.id), style.alpha),
      kind: node.kind,
      lang: node.lang,
      x: Math.random(),
      y: Math.random(),
    });
  }
  for (const edge of data.edges) {
    if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
      graph.mergeEdge(edge.source, edge.target, { size: 0.4, color: "#3a3a3a" });
    }
  }

  // Size by degree: hubs stand out without inflating the layout.
  graph.forEachNode((node) => {
    const scale = KIND_STYLE[graph.getNodeAttribute(node, "kind") as NodeKind]?.scale ?? 1;
    graph.setNodeAttribute(node, "size", (2 + Math.sqrt(graph.degree(node)) * 1.6) * scale);
  });

  forceAtlas2.assign(graph, {
    iterations: 300,
    settings: { ...forceAtlas2.inferSettings(graph), gravity: 0.6 },
  });
  return graph;
}

const PANEL =
  "position:fixed;font:12px ui-monospace,monospace;color:#ddd;background:#111d;padding:8px 12px;border-radius:6px;line-height:1.7";

function summarise(project: KogProject): string {
  const { stats } = project.graph;
  const coverage = stats.coverage;
  const source = coverage.files_analysed + coverage.files_unsupported;
  const sourceCoverage = source === 0 ? 1 : coverage.files_analysed / source;
  const languages = Object.entries(stats.by_lang)
    .sort((a, b) => b[1].files - a[1].files)
    .map(([lang, s]) => `${lang} ${(s.resolution_rate * 100).toFixed(1)}%`)
    .join(" · ");
  const gaps = coverage.extensions
    .filter((e) => e.status === "unsupported_language")
    .slice(0, 4)
    .map((e) => `.${e.extension} ${e.count}`)
    .join(" · ");

  return [
    `${project.graph.nodes.length} nodes · ${project.graph.edges.length} edges`,
    `resolution ${(stats.resolution_rate * 100).toFixed(1)}% · ${stats.excluded} excluded`,
    `read ${(sourceCoverage * 100).toFixed(1)}% of source files`,
    languages && `by language: ${languages}`,
    gaps && `not read: ${gaps}`,
  ]
    .filter(Boolean)
    .join("\n");
}

async function main(): Promise<void> {
  const container = document.getElementById("root");
  if (!container) throw new Error("missing #root");

  const response = await fetch("/graph.json");
  if (!response.ok) throw new Error(`graph.json: ${response.status}`);
  const workspace: KogWorkspace = await response.json();
  if (!workspace.projects?.length) throw new Error("no project in this scan");

  let current = 0;
  let showAssets = true;
  let renderer: Sigma | null = null;

  const badge = document.createElement("div");
  badge.style.cssText = `${PANEL};top:12px;left:12px;white-space:pre`;
  document.body.appendChild(badge);

  const controls = document.createElement("div");
  controls.style.cssText = `${PANEL};top:12px;right:12px;display:flex;gap:8px;align-items:center`;
  document.body.appendChild(controls);

  function render(): void {
    const project = workspace.projects[current];
    renderer?.kill();
    container!.replaceChildren();
    renderer = new Sigma(buildGraph(project.graph, showAssets), container as HTMLElement, {
      renderEdgeLabels: false,
      defaultEdgeColor: "#333",
    });
    // Report what the scan actually measured. A resolution rate alone hides
    // both the excluded specifiers and — the bigger omission on a polyglot
    // repository — the files no extractor could read at all.
    badge.textContent = `${project.id === "." ? workspace.root.split("/").pop() : project.id}\n${summarise(project)}`;
  }

  // One graph per project: a directory holding several codebases is not one
  // shape, and a picker is how you say so without merging them.
  if (workspace.projects.length > 1) {
    const picker = document.createElement("select");
    picker.style.cssText =
      "font:12px ui-monospace,monospace;background:#222;color:#ddd;border:1px solid #444;border-radius:4px;padding:2px 4px";
    for (const [index, project] of workspace.projects.entries()) {
      const option = document.createElement("option");
      option.value = String(index);
      option.textContent = `${project.id} (${project.graph.nodes.length})`;
      picker.appendChild(option);
    }
    picker.addEventListener("change", () => {
      current = Number(picker.value);
      render();
    });
    controls.appendChild(picker);
  }

  const toggle = document.createElement("label");
  toggle.style.cssText = "display:flex;gap:4px;align-items:center;cursor:pointer";
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox";
  checkbox.checked = showAssets;
  checkbox.addEventListener("change", () => {
    showAssets = checkbox.checked;
    render();
  });
  toggle.append(checkbox, document.createTextNode("assets"));
  controls.appendChild(toggle);

  render();
}

main().catch((error) => {
  const pre = document.createElement("pre");
  pre.style.cssText = "color:#f66;padding:24px";
  pre.textContent = String(error);
  document.body.replaceChildren(pre);
});
