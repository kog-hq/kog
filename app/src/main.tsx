import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import Sigma from "sigma";

type MyceliumNode = {
  id: string;
  path: string;
  lang: string;
  loc: number;
  external_deps: string[];
};

type MyceliumEdge = { source: string; target: string; kind: string };

type MyceliumFailure = { path: string; reason: string };

type MyceliumDiagnostic = {
  path: string;
  line: number;
  specifier: string;
  kind: "unresolved" | "excluded";
};

type MyceliumStats = {
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
  failures: MyceliumFailure[];
  diagnostics: MyceliumDiagnostic[];
};

type MyceliumGraph = {
  nodes: MyceliumNode[];
  edges: MyceliumEdge[];
  stats: MyceliumStats;
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

/** Stable colour per top-level directory, so clusters read at a glance. */
function colourFor(id: string): string {
  const top = id.split("/")[0];
  let hash = 0;
  for (let i = 0; i < top.length; i++) {
    hash = (hash * 31 + top.charCodeAt(i)) >>> 0;
  }
  return hslToHex(hash % 360, 65, 55);
}

async function main(): Promise<void> {
  const container = document.getElementById("root");
  if (!container) throw new Error("missing #root");

  const response = await fetch("/graph.json");
  if (!response.ok) throw new Error(`graph.json: ${response.status}`);
  const data: MyceliumGraph = await response.json();

  const graph = new Graph();
  for (const node of data.nodes) {
    graph.addNode(node.id, {
      label: node.id.split("/").pop() ?? node.id,
      size: 2,
      color: colourFor(node.id),
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
    graph.setNodeAttribute(node, "size", 2 + Math.sqrt(graph.degree(node)) * 1.6);
  });

  forceAtlas2.assign(graph, {
    iterations: 300,
    settings: { ...forceAtlas2.inferSettings(graph), gravity: 0.6 },
  });

  new Sigma(graph, container as HTMLElement, {
    renderEdgeLabels: false,
    defaultEdgeColor: "#333",
  });

  // Report what the scan actually measured: a resolution rate alone hides
  // the excluded specifiers (resolved to a real file outside the scanned
  // set), so show nodes, edges, the rate, and the excluded count together.
  const badge = document.createElement("div");
  badge.style.cssText =
    "position:fixed;top:12px;left:12px;font:13px ui-monospace,monospace;color:#ddd;background:#111c;padding:8px 12px;border-radius:6px";
  badge.textContent = `${graph.order} nodes · ${graph.size} edges · resolution ${(
    data.stats.resolution_rate * 100
  ).toFixed(1)}% · ${data.stats.excluded} excluded`;
  document.body.appendChild(badge);
}

main().catch((error) => {
  const pre = document.createElement("pre");
  pre.style.cssText = "color:#f66;padding:24px";
  pre.textContent = String(error);
  document.body.replaceChildren(pre);
});
