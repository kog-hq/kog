import Graph from "graphology";
import forceAtlas2 from "graphology-layout-forceatlas2";
import Sigma from "sigma";
import type { Settings } from "sigma/settings";
import type { NodeDisplayData, PartialButFor } from "sigma/types";

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
  label: string;
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

type KogGraph = { nodes: KogNode[]; edges: KogEdge[]; stats: KogStats };

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

// --- Theme ---------------------------------------------------------------

type ThemeName = "light" | "dark";

type Theme = {
  background: string;
  edge: string;
  label: string;
  /** Painted behind label text so a name stays readable over edges. */
  labelHalo: string;
  panel: string;
  panelText: string;
  panelBorder: string;
  /** Lightness and saturation that keep node colours legible on this background. */
  nodeSaturation: number;
  nodeLightness: number;
  /** What a faded node is mixed towards: the background, whichever it is. */
  fadeTowards: number;
};

const THEMES: Record<ThemeName, Theme> = {
  dark: {
    background: "#0d0d0f",
    edge: "#2f2f38",
    label: "#e9e9ee",
    labelHalo: "rgba(13,13,15,0.82)",
    panel: "rgba(22,22,27,0.88)",
    panelText: "#e9e9ee",
    panelBorder: "#33333d",
    nodeSaturation: 65,
    nodeLightness: 57,
    fadeTowards: 0x18,
  },
  light: {
    background: "#fbfbfd",
    edge: "#c7c7d2",
    label: "#15151b",
    labelHalo: "rgba(251,251,253,0.85)",
    panel: "rgba(255,255,255,0.92)",
    panelText: "#15151b",
    panelBorder: "#dcdce4",
    nodeSaturation: 62,
    nodeLightness: 44,
    fadeTowards: 0xff,
  },
};

const THEME_KEY = "kog:theme";

function preferredTheme(): ThemeName {
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

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
 * Stable hue per directory, using the two leading path segments rather than
 * one. On a monorepo every `apps/*` shares a first segment, so keying on it
 * wastes the only visual channel that carries structure: `apps/web` and
 * `apps/api` came out the same colour while being the two halves of the map
 * a reader most needs to tell apart.
 */
function hueFor(id: string): number {
  const segments = id.split("/");
  const key = segments.length > 2 ? `${segments[0]}/${segments[1]}` : segments[0];
  let hash = 0;
  for (let i = 0; i < key.length; i++) {
    hash = (hash * 31 + key.charCodeAt(i)) >>> 0;
  }
  return hash % 360;
}

/** Assets and unreadable languages are drawn back, so code reads first. */
const KIND_STYLE: Record<NodeKind, { alpha: number; scale: number }> = {
  source: { alpha: 1, scale: 1 },
  unread_source: { alpha: 0.75, scale: 0.85 },
  asset: { alpha: 0.4, scale: 0.6 },
};

/** Mix a colour towards the background, so faded means faded in both themes. */
function fade(hex: string, alpha: number, towards: number): string {
  if (alpha >= 1) return hex;
  const value = parseInt(hex.slice(1), 16);
  const mix = (channel: number) => Math.round(channel * alpha + towards * (1 - alpha));
  const r = mix((value >> 16) & 0xff);
  const g = mix((value >> 8) & 0xff);
  const b = mix(value & 0xff);
  return `#${[r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

function nodeColour(hue: number, kind: NodeKind, theme: Theme): string {
  const style = KIND_STYLE[kind] ?? KIND_STYLE.source;
  return fade(
    hslToHex(hue, theme.nodeSaturation, theme.nodeLightness),
    style.alpha,
    theme.fadeTowards,
  );
}

// --- Labels --------------------------------------------------------------

/**
 * How many names are on screen at once.
 *
 * Sigma decides by node size: only nodes drawn larger than the threshold get
 * a label, so on a big graph the default shows a handful of hubs and nothing
 * else. "All" is unreadable on 2,000 nodes and indispensable on 40, which is
 * why this is a control rather than a constant.
 */
const LABEL_MODES = {
  none: { threshold: Infinity, density: 1 },
  hubs: { threshold: 7, density: 1 },
  more: { threshold: 3, density: 3 },
  all: { threshold: 0, density: 20 },
} as const;

type LabelMode = keyof typeof LABEL_MODES;

/**
 * How many names to show before anyone touches the control.
 *
 * A 60-file project can have every name on screen and be the better for it;
 * the same setting on 2,800 files is a grey smear. Sigma's own default shows
 * a handful of hubs whatever the size, which reads as "the labels are
 * broken" on anything small — so the default scales with the graph.
 */
function defaultLabelMode(nodeCount: number): LabelMode {
  if (nodeCount <= 150) return "all";
  if (nodeCount <= 700) return "more";
  return "hubs";
}

/**
 * Draw a node's name with a slab of background behind it.
 *
 * Sigma's own label renderer paints text straight onto the canvas, so a name
 * crossing a dense patch of edges becomes unreadable exactly where the graph
 * is most interesting. The halo is the whole point of overriding it.
 */
function drawLabel(theme: Theme) {
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<NodeDisplayData, "x" | "y" | "size" | "label" | "color">,
    settings: Settings,
  ): void => {
    if (!data.label) return;

    const size = settings.labelSize;
    context.font = `${settings.labelWeight} ${size}px ${settings.labelFont}`;
    const width = context.measureText(data.label).width;

    const x = data.x + data.size + 4;
    const y = data.y + size / 3;

    context.fillStyle = theme.labelHalo;
    context.beginPath();
    context.roundRect(x - 3, y - size + 1, width + 6, size + 4, 3);
    context.fill();

    context.fillStyle = theme.label;
    context.fillText(data.label, x, y);
  };
}

/** The hovered node's name, drawn the same way but never hidden. */
function drawHover(theme: Theme) {
  const draw = drawLabel(theme);
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<NodeDisplayData, "x" | "y" | "size" | "label" | "color">,
    settings: Settings,
  ): void => {
    draw(context, data, { ...settings, labelWeight: "700" });
  };
}

// --- Page ----------------------------------------------------------------

function panelStyle(theme: Theme): string {
  return `position:fixed;font:12px ui-monospace,SFMono-Regular,Menlo,monospace;color:${theme.panelText};background:${theme.panel};border:1px solid ${theme.panelBorder};padding:8px 12px;border-radius:8px;line-height:1.7;backdrop-filter:blur(6px)`;
}

function controlStyle(theme: Theme): string {
  return `font:12px ui-monospace,SFMono-Regular,Menlo,monospace;background:transparent;color:${theme.panelText};border:1px solid ${theme.panelBorder};border-radius:5px;padding:2px 5px;cursor:pointer`;
}

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
    .map((e) => `${e.label} ${e.count}`)
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

function buildGraph(data: KogGraph, showAssets: boolean, theme: Theme): Graph {
  const graph = new Graph();
  for (const node of data.nodes) {
    if (!showAssets && node.kind === "asset") continue;
    const style = KIND_STYLE[node.kind] ?? KIND_STYLE.source;
    const hue = hueFor(node.id);
    graph.addNode(node.id, {
      label: node.id.split("/").pop() ?? node.id,
      size: 2 * style.scale,
      // Kept so a theme change can recolour in place, without relaying out.
      hue,
      kind: node.kind,
      color: nodeColour(hue, node.kind, theme),
      x: Math.random(),
      y: Math.random(),
    });
  }
  for (const edge of data.edges) {
    if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
      graph.mergeEdge(edge.source, edge.target, { size: 0.4 });
    }
  }

  // Size by degree: hubs stand out without inflating the layout — and, since
  // sigma decides which labels to draw by size, this is also what puts the
  // most-depended-upon files' names on screen first.
  graph.forEachNode((node) => {
    const scale = KIND_STYLE[graph.getNodeAttribute(node, "kind") as NodeKind]?.scale ?? 1;
    graph.setNodeAttribute(node, "size", (2 + Math.sqrt(graph.degree(node)) * 1.7) * scale);
  });

  forceAtlas2.assign(graph, {
    iterations: 300,
    settings: { ...forceAtlas2.inferSettings(graph), gravity: 0.6 },
  });
  return graph;
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
  let labelMode: LabelMode = defaultLabelMode(workspace.projects[0].graph.nodes.length);
  let themeName = preferredTheme();
  /** Set once the reader picks a label mode, which then sticks across projects. */
  let labelChosen = false;
  let renderer: Sigma | null = null;

  const badge = document.createElement("div");
  const controls = document.createElement("div");
  controls.style.display = "flex";
  document.body.append(badge, controls);

  const themeButton = document.createElement("button");
  const labelPicker = document.createElement("select");
  for (const mode of Object.keys(LABEL_MODES) as LabelMode[]) {
    const option = document.createElement("option");
    option.value = mode;
    option.textContent = `labels: ${mode}`;
    labelPicker.appendChild(option);
  }
  labelPicker.value = labelMode;

  const assetsToggle = document.createElement("label");
  const assetsBox = document.createElement("input");
  assetsBox.type = "checkbox";
  assetsBox.checked = showAssets;
  assetsToggle.style.cssText = "display:flex;gap:4px;align-items:center;cursor:pointer";
  assetsToggle.append(assetsBox, document.createTextNode("assets"));

  function theme(): Theme {
    return THEMES[themeName];
  }

  /** Restyle everything a theme touches, without rebuilding the layout. */
  function applyTheme(): void {
    const t = theme();
    // On the root element, not the body: the page shell paints it there
    // before first paint to avoid a flash, and the two must not disagree.
    document.documentElement.style.background = t.background;
    badge.style.cssText = `${panelStyle(t)};top:12px;left:12px;white-space:pre;max-width:min(46ch,42vw)`;
    controls.style.cssText = `${panelStyle(t)};top:12px;right:12px;display:flex;gap:8px;align-items:center`;
    themeButton.style.cssText = controlStyle(t);
    labelPicker.style.cssText = controlStyle(t);
    themeButton.textContent = themeName === "dark" ? "☀ light" : "☾ dark";

    if (!renderer) return;
    const graph = renderer.getGraph();
    graph.forEachNode((node, attributes) => {
      graph.setNodeAttribute(
        node,
        "color",
        nodeColour(attributes.hue as number, attributes.kind as NodeKind, t),
      );
    });
    renderer.setSetting("defaultEdgeColor", t.edge);
    renderer.setSetting("defaultDrawNodeLabel", drawLabel(t));
    renderer.setSetting("defaultDrawNodeHover", drawHover(t));
    renderer.refresh();
  }

  function applyLabelMode(): void {
    if (!renderer) return;
    const mode = LABEL_MODES[labelMode];
    renderer.setSetting("renderLabels", labelMode !== "none");
    renderer.setSetting("labelRenderedSizeThreshold", mode.threshold);
    renderer.setSetting("labelDensity", mode.density);
    renderer.refresh();
  }

  function render(): void {
    const t = theme();
    const project = workspace.projects[current];
    renderer?.kill();
    container!.replaceChildren();
    renderer = new Sigma(buildGraph(project.graph, showAssets, t), container as HTMLElement, {
      renderEdgeLabels: false,
      defaultEdgeColor: t.edge,
      // Bigger and heavier than sigma's default 14px normal: a file name is
      // the one piece of text on this page, and it is read at a glance.
      labelFont: "ui-monospace, SFMono-Regular, Menlo, monospace",
      labelSize: 12,
      labelWeight: "500",
      labelGridCellSize: 70,
      defaultDrawNodeLabel: drawLabel(t),
      defaultDrawNodeHover: drawHover(t),
    });
    // Switching to a project of a very different size gets that size's
    // default, unless the reader has already made a choice.
    if (!labelChosen) labelMode = defaultLabelMode(project.graph.nodes.length);
    labelPicker.value = labelMode;
    applyLabelMode();
    // Report what the scan actually measured. A resolution rate alone hides
    // both the excluded specifiers and — the bigger omission on a polyglot
    // repository — the files no extractor could read at all.
    const name = project.id === "." ? workspace.root.split("/").pop() : project.id;
    badge.textContent = `${name}\n${summarise(project)}`;
  }

  themeButton.addEventListener("click", () => {
    themeName = themeName === "dark" ? "light" : "dark";
    localStorage.setItem(THEME_KEY, themeName);
    applyTheme();
  });
  labelPicker.addEventListener("change", () => {
    labelMode = labelPicker.value as LabelMode;
    labelChosen = true;
    applyLabelMode();
  });
  assetsBox.addEventListener("change", () => {
    showAssets = assetsBox.checked;
    render();
    applyTheme();
  });

  // One graph per project: a directory holding several codebases is not one
  // shape, and a picker is how you say so without merging them.
  if (workspace.projects.length > 1) {
    const picker = document.createElement("select");
    for (const [index, project] of workspace.projects.entries()) {
      const option = document.createElement("option");
      option.value = String(index);
      option.textContent = `${project.id} (${project.graph.nodes.length})`;
      picker.appendChild(option);
    }
    picker.addEventListener("change", () => {
      current = Number(picker.value);
      render();
      applyTheme();
    });
    controls.appendChild(picker);
    // Restyled with everything else, so it follows the theme too.
    const restyle = () => (picker.style.cssText = controlStyle(theme()));
    themeButton.addEventListener("click", restyle);
    restyle();
  }
  controls.append(labelPicker, assetsToggle, themeButton);

  render();
  applyTheme();
}

main().catch((error) => {
  const pre = document.createElement("pre");
  pre.style.cssText = "color:#e5484d;padding:24px;font:13px ui-monospace,monospace";
  pre.textContent = String(error);
  document.body.replaceChildren(pre);
});
