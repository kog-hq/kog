/**
 * Turning a scan into something sigma can draw.
 *
 * The layout is computed once per view and never again: filtering, selecting
 * and highlighting all run through sigma's reducers, which repaint without
 * moving a single node. A graph that rearranges itself every time you tick a
 * checkbox is unreadable, however fast it is.
 */

import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import type { KogProject, NodeKind, ProjectIndex } from "@/lib/kog";
import { dirName } from "@/lib/kog";

export type NodeAttributes = {
  label: string;
  size: number;
  color: string;
  hue: number;
  kind: NodeKind;
  lang: string;
  /** Files behind this node: one, unless folders are collapsed. */
  members: string[];
  x: number;
  y: number;
};

/**
 * Sigma's WebGL renderer parses colours itself and only understands `#hex`
 * and `rgb()`; an `hsl()` string silently falls through to black.
 */
function hslToHex(h: number, s: number, l: number): string {
  const sat = s / 100;
  const lig = l / 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = sat * Math.min(lig, 1 - lig);
  const f = (n: number) => lig - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const channel = (n: number) =>
    Math.round(f(n) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${channel(0)}${channel(8)}${channel(4)}`;
}

/**
 * A stable hue per directory, keyed on the two leading path segments.
 *
 * On a monorepo every `apps/*` shares a first segment, so keying on one
 * wastes the only visual channel that carries structure: `apps/web` and
 * `apps/api` came out the same colour while being the two halves of the map
 * a reader most needs to tell apart.
 */
export function hueFor(id: string): number {
  const segments = id.split("/");
  const key = segments.length > 2 ? `${segments[0]}/${segments[1]}` : segments[0];
  let hash = 0;
  for (let i = 0; i < key.length; i++) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0;
  }
  // Two hue bands are reserved, and a directory may never be given one:
  // ~340° is the signal magenta that means "KOG could not read this", and
  // ~210° is the blue of the current selection. Found on a real monorepo,
  // where `apps/backend` hashed to a magenta close enough to the signal that
  // a whole NestJS tree read as unread code. A palette whose meaning depends
  // on a hash not colliding is not a palette, it is a coincidence.
  const hue = hash % 290;
  return hue < 200 ? hue : hue + 30;
}

export function readCanvasTheme() {
  const style = getComputedStyle(document.documentElement);
  const value = (name: string) => style.getPropertyValue(name).trim();
  return {
    edge: value("--edge"),
    labelHalo: value("--label-halo"),
    label: value("--foreground"),
    background: value("--background"),
    saturation: Number.parseFloat(value("--node-saturation")) || 65,
    lightness: Number.parseFloat(value("--node-lightness")) || 57,
    signal: value("--color-signal") || "#e0457b",
    focus: value("--color-focus") || "#4a90d9",
  };
}

export type CanvasTheme = ReturnType<typeof readCanvasTheme>;

/** Assets sit back; unread source is the colour of the thing KOG missed. */
export function colourFor(
  hue: number,
  kind: NodeKind,
  theme: CanvasTheme,
): string {
  if (kind === "unread_source") return theme.signal;
  const lightness = kind === "asset" ? theme.lightness * 0.72 : theme.lightness;
  const saturation = kind === "asset" ? theme.saturation * 0.35 : theme.saturation;
  return hslToHex(hue, saturation, lightness);
}

function sizeFor(degree: number, kind: NodeKind): number {
  const base = 2.4 + Math.sqrt(degree) * 1.7;
  return kind === "asset" ? base * 0.62 : base;
}

export type BuildOptions = {
  /** Collapse every file into the folder that holds it. */
  groupByFolder: boolean;
};

/**
 * Build the drawable graph. Every node of the scan is here, whatever the
 * filters say: filtering is a reducer, not a rebuild, so a node hidden now
 * keeps the position it will have when it comes back.
 */
export function buildGraph(
  project: KogProject,
  index: ProjectIndex,
  theme: CanvasTheme,
  options: BuildOptions,
): Graph {
  const graph = new Graph({ type: "directed", multi: false });

  if (options.groupByFolder) {
    const members = new Map<string, string[]>();
    for (const node of project.graph.nodes) {
      const folder = dirName(node.id);
      const list = members.get(folder);
      if (list) list.push(node.id);
      else members.set(folder, [node.id]);
    }
    for (const [folder, files] of members) {
      // A folder takes the kind of what it mostly holds, so a package of
      // unread Swift still reads as unread.
      const counts = new Map<NodeKind, number>();
      for (const id of files) {
        const kind = index.byId.get(id)?.kind ?? "asset";
        counts.set(kind, (counts.get(kind) ?? 0) + 1);
      }
      const kind = [...counts.entries()].sort((a, b) => b[1] - a[1])[0][0];
      const hue = hueFor(`${folder}/x`);
      graph.addNode(folder, {
        label: folder,
        size: 3,
        color: colourFor(hue, kind, theme),
        hue,
        kind,
        lang: index.byId.get(files[0])?.lang ?? "",
        members: files,
        x: Math.random(),
        y: Math.random(),
      });
    }
    for (const edge of project.graph.edges) {
      const from = dirName(edge.source);
      const to = dirName(edge.target);
      if (from === to || !graph.hasNode(from) || !graph.hasNode(to)) continue;
      graph.mergeEdge(from, to);
    }
  } else {
    for (const node of project.graph.nodes) {
      const hue = hueFor(node.id);
      graph.addNode(node.id, {
        label: index.label.get(node.id) ?? node.id,
        size: 2,
        color: colourFor(hue, node.kind, theme),
        hue,
        kind: node.kind,
        lang: node.lang,
        members: [node.id],
        x: Math.random(),
        y: Math.random(),
      });
    }
    for (const edge of project.graph.edges) {
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.addDirectedEdge(edge.source, edge.target);
      }
    }
  }

  graph.forEachNode((node, attributes) => {
    graph.setNodeAttribute(
      node,
      "size",
      sizeFor(graph.degree(node), attributes.kind as NodeKind),
    );
  });

  // Iterations traded against size: a 3,000-node layout that takes four
  // seconds reads as a hang, and the extra passes buy very little once the
  // clusters have separated.
  const order = graph.order;
  const iterations = order > 2000 ? 120 : order > 700 ? 200 : 320;
  forceAtlas2.assign(graph, {
    iterations,
    settings: {
      ...forceAtlas2.inferSettings(graph),
      gravity: 0.7,
      barnesHutOptimize: order > 500,
    },
  });

  return graph;
}

/** Recolour in place when the theme changes: no relayout, no jump. */
export function recolour(graph: Graph, theme: CanvasTheme): void {
  graph.forEachNode((node, attributes) => {
    graph.setNodeAttribute(
      node,
      "color",
      colourFor(attributes.hue as number, attributes.kind as NodeKind, theme),
    );
  });
}
