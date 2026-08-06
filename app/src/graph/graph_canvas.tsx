import Sigma from "sigma";
import { EdgeArrowProgram } from "sigma/rendering";
import type { Settings } from "sigma/settings";
import type { NodeDisplayData, PartialButFor } from "sigma/types";
import { useEffect, useMemo, useRef } from "react";
import { CursorField } from "./cursor_field";
import {
  buildGraph,
  readCanvasTheme,
  recolour,
  ringOf,
  type CanvasTheme,
  type ColourBy,
  type Theme,
} from "./build";
import type { NodeState } from "@/lib/palette";
import type { KogProject, ProjectIndex } from "@/lib/kog";

export type LabelMode = "none" | "hubs" | "more" | "all";

/**
 * How many names are on screen at once. Sigma decides by drawn size, so the
 * threshold is what separates "the five hubs" from "everything".
 */
const LABEL_MODES: Record<LabelMode, { threshold: number; density: number }> = {
  none: { threshold: Infinity, density: 1 },
  hubs: { threshold: 8, density: 1 },
  more: { threshold: 4, density: 3 },
  all: { threshold: 0, density: 20 },
};

/** Every name on a small graph, only the hubs on a large one. */
export function defaultLabelMode(nodeCount: number): LabelMode {
  if (nodeCount <= 150) return "all";
  if (nodeCount <= 700) return "more";
  return "hubs";
}

/** How long the graph takes to arrive, in milliseconds. */
const REVEAL_MS = 620;

/** The cursor's reach, in screen pixels, converted to graph units per frame. */
const FIELD_RADIUS_PX = 120;

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export type CanvasState = {
  /** Ids that pass the current filters. Everything else is hidden. */
  visible: Set<string> | null;
  selected: string | null;
  /** Anything the pointer is over, in the graph or in a side panel. */
  hovered: string | null;
  labelMode: LabelMode;
  groupByFolder: boolean;
  colourBy: ColourBy;
  theme: Theme;
};

type Props = CanvasState & {
  project: KogProject;
  index: ProjectIndex;
  onSelect: (id: string | null) => void;
  onHover: (id: string | null) => void;
};

/**
 * Draw a node's name over a slab of background.
 *
 * Sigma paints label text straight onto the canvas, so a name crossing a
 * dense patch of edges becomes unreadable exactly where the graph is most
 * worth reading. The halo is the whole point of overriding the renderer.
 */
function labelDrawer(theme: CanvasTheme, bold = false) {
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<NodeDisplayData, "x" | "y" | "size" | "label" | "color">,
    settings: Settings,
  ): void => {
    if (!data.label) return;
    const size = settings.labelSize;
    context.font = `${bold ? 600 : settings.labelWeight} ${size}px ${settings.labelFont}`;
    const width = context.measureText(data.label).width;
    const x = data.x + data.size + 5;
    const y = data.y + size / 3;

    context.fillStyle = theme.labelHalo;
    context.beginPath();
    context.roundRect(x - 4, y - size + 1, width + 8, size + 5, 4);
    context.fill();

    context.fillStyle = bold ? theme.focus : theme.label;
    context.fillText(data.label, x, y);
  };
}

/**
 * The hovered node gets a breathing ring on sigma's hover layer.
 *
 * A node that grows on hover would be ambiguous: size already means degree
 * here, so growing it says something false. A ring adds emphasis without
 * touching the channel that already carries a number.
 */
function hoverDrawer(theme: CanvasTheme, phase: () => number) {
  const label = labelDrawer(theme, true);
  return (
    context: CanvasRenderingContext2D,
    data: PartialButFor<NodeDisplayData, "x" | "y" | "size" | "label" | "color">,
    settings: Settings,
  ): void => {
    const breath = phase();
    context.save();
    context.beginPath();
    context.arc(data.x, data.y, data.size + 3 + breath * 3, 0, Math.PI * 2);
    context.strokeStyle = theme.focus;
    context.globalAlpha = 0.8 - breath * 0.45;
    context.lineWidth = 1.5;
    context.stroke();
    context.restore();
    label(context, data, settings);
  };
}

export function GraphCanvas(props: Props) {
  const {
    project,
    index,
    visible,
    selected,
    hovered,
    labelMode,
    groupByFolder,
    colourBy,
    theme,
    onSelect,
    onHover,
  } = props;

  const container = useRef<HTMLDivElement>(null);
  const sigma = useRef<Sigma | null>(null);
  const field = useRef<CursorField | null>(null);

  // Animation state lives in refs: a frame loop must not re-render React 60
  // times a second to move a highlight by two pixels.
  const reveal = useRef(1);
  const breath = useRef(0);
  const rings = useRef(new Map<string, string>());

  // The layout is the expensive part, so it is tied to what actually changes
  // its shape: the project and whether folders are collapsed. Filters,
  // selection, colour mode and hover never reach here.
  const graph = useMemo(
    () => buildGraph(project, index, { groupByFolder }),
    [project, index, groupByFolder],
  );

  /** The anchor for highlighting, and the two directions around it. */
  const focus = useMemo(() => {
    const anchor = hovered ?? selected;
    if (!anchor || groupByFolder) return null;
    return {
      anchor,
      // What this file uses, and what uses it. Direction is the whole
      // question a dependency graph answers, so the two are drawn
      // differently rather than merged into one blob of "related".
      uses: new Set(index.dependencies.get(anchor) ?? []),
      usedBy: new Set(index.dependents.get(anchor) ?? []),
    };
  }, [hovered, selected, index, groupByFolder]);

  useEffect(() => {
    if (!container.current) return;
    const canvasTheme = readCanvasTheme(theme);
    const renderer = new Sigma(graph, container.current, {
      renderEdgeLabels: false,
      defaultEdgeColor: canvasTheme.edge,
      // Arrows are registered but never the default: 3,000 arrowheads at low
      // zoom is noise. The reducer promotes an edge to an arrow only while it
      // is being read.
      edgeProgramClasses: { arrow: EdgeArrowProgram },
      labelFont: '"JetBrains Mono Variable", ui-monospace, monospace',
      labelSize: 11.5,
      labelWeight: "500",
      labelGridCellSize: 72,
      defaultDrawNodeLabel: labelDrawer(canvasTheme),
      defaultDrawNodeHover: hoverDrawer(canvasTheme, () => breath.current),
      zIndex: true,
    });
    sigma.current = renderer;
    field.current = new CursorField(graph);

    renderer.on("clickNode", ({ node }) => onSelect(node));
    renderer.on("clickStage", () => onSelect(null));
    renderer.on("enterNode", ({ node }) => onHover(node));
    renderer.on("leaveNode", () => onHover(null));

    return () => {
      renderer.kill();
      sigma.current = null;
      field.current = null;
    };
  }, [graph, theme, onSelect, onHover]);

  /**
   * The graph parts under the pointer, and closes behind it.
   *
   * The loop only runs while something is actually moving: a cursor over the
   * canvas, or nodes still springing back. It stops on its own the moment the
   * graph is at rest, so an idle window costs nothing.
   */
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer || prefersReducedMotion()) return;

    let cursor: { x: number; y: number } | null = null;
    let raf = 0;
    let running = false;

    const tick = () => {
      const active = field.current?.step(cursor, {
        // Both are read from the camera, so the effect is the same size on
        // screen whatever the zoom.
        radius: FIELD_RADIUS_PX * renderer.getCamera().ratio * scale(),
        strength: 2.2 * renderer.getCamera().ratio * scale(),
      });
      renderer.refresh({ skipIndexation: true });
      if (cursor || active) raf = requestAnimationFrame(tick);
      else running = false;
    };

    // One graph unit in screen pixels, so the field can be specified in
    // pixels and behave identically at every zoom level.
    const scale = () => {
      const a = renderer.viewportToGraph({ x: 0, y: 0 });
      const b = renderer.viewportToGraph({ x: 1, y: 0 });
      return Math.abs(b.x - a.x) / Math.max(renderer.getCamera().ratio, 1e-6);
    };

    const start = () => {
      if (running) return;
      running = true;
      raf = requestAnimationFrame(tick);
    };

    const mouse = renderer.getMouseCaptor();
    const onMove = (event: { x: number; y: number }) => {
      cursor = renderer.viewportToGraph(event);
      start();
    };
    const onLeave = () => {
      cursor = null;
      start();
    };

    mouse.on("mousemovebody", onMove);
    mouse.on("mouseleave", onLeave);

    return () => {
      mouse.off("mousemovebody", onMove);
      mouse.off("mouseleave", onLeave);
      cancelAnimationFrame(raf);
      field.current?.reset();
    };
  }, [graph]);

  // The graph arrives rather than appearing: nodes scale up over half a
  // second, which turns a hard cut into a moment where the eye can follow the
  // shape forming.
  useEffect(() => {
    if (prefersReducedMotion()) {
      reveal.current = 1;
      return;
    }
    reveal.current = 0;
    const started = performance.now();
    let raf = 0;
    const step = () => {
      const elapsed = performance.now() - started;
      reveal.current = Math.min(1, 1 - Math.pow(2, (-10 * elapsed) / REVEAL_MS));
      sigma.current?.refresh({ skipIndexation: true });
      if (elapsed < REVEAL_MS) raf = requestAnimationFrame(step);
      else reveal.current = 1;
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [graph]);

  // A slow pulse on whatever is being read, and nothing at all otherwise: an
  // idle canvas that never stops moving is exhausting to sit in front of.
  useEffect(() => {
    if (!focus || prefersReducedMotion()) {
      breath.current = 0;
      sigma.current?.refresh({ skipIndexation: true });
      return;
    }
    const started = performance.now();
    let raf = 0;
    const step = () => {
      breath.current = (1 - Math.cos(((performance.now() - started) / 1500) * Math.PI * 2)) / 2;
      sigma.current?.refresh({ skipIndexation: true });
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [focus]);

  // Filtering, selection, hover and colour mode are all one repaint: sigma
  // asks these reducers what each node and edge looks like right now.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const canvasTheme = readCanvasTheme(theme);
    recolour(graph, index, colourBy, theme);

    // The ring is the second channel that keeps "not read" and "has a gap"
    // legible without relying on hue — including for a reader who cannot
    // separate magenta from teal.
    rings.current = new Map();
    graph.forEachNode((node, attributes) => {
      const ring = ringOf(attributes.state as NodeState, canvasTheme);
      if (ring) rings.current.set(node, ring);
    });

    renderer.setSetting("nodeReducer", (node, data) => {
      if (visible && !visible.has(node)) return { ...data, hidden: true };
      const size = data.size * reveal.current;

      if (node === selected) {
        return {
          ...data,
          color: canvasTheme.focus,
          size: size * (1.45 + breath.current * 0.12),
          zIndex: 3,
          forceLabel: true,
        };
      }
      if (focus) {
        if (node === focus.anchor) {
          return { ...data, size: size * 1.3, zIndex: 3, forceLabel: true };
        }
        if (focus.uses.has(node) || focus.usedBy.has(node)) {
          return { ...data, size, zIndex: 2, forceLabel: true };
        }
        // Dimmed and shrunk rather than hidden: the shape of the whole is
        // the context that makes a neighbourhood mean anything, but it has
        // to stop competing while you read one.
        return { ...data, color: canvasTheme.edge, size: size * 0.55, label: "", zIndex: 0 };
      }
      return { ...data, size };
    });

    renderer.setSetting("edgeReducer", (edge, data) => {
      const [source, target] = graph.extremities(edge);
      if (visible && (!visible.has(source) || !visible.has(target))) {
        return { ...data, hidden: true };
      }
      if (focus) {
        // What the anchor uses: blue, the colour of the thing you are
        // following. What uses the anchor: the plain foreground. The two
        // directions never have to be guessed from arrowheads alone.
        if (source === focus.anchor && focus.uses.has(target)) {
          return {
            ...data,
            type: "arrow",
            color: canvasTheme.focus,
            size: 1.4 + breath.current * 0.5,
            zIndex: 2,
          };
        }
        if (target === focus.anchor && focus.usedBy.has(source)) {
          return {
            ...data,
            type: "arrow",
            color: canvasTheme.label,
            size: 1 + breath.current * 0.4,
            zIndex: 2,
          };
        }
        return { ...data, color: canvasTheme.edge, size: 0.18 };
      }
      return { ...data, color: canvasTheme.edge, size: 0.45 };
    });

    renderer.setSetting("defaultDrawNodeLabel", labelDrawer(canvasTheme));
    renderer.setSetting("defaultDrawNodeHover", hoverDrawer(canvasTheme, () => breath.current));
    renderer.setSetting("defaultEdgeColor", canvasTheme.edge);
    renderer.refresh();
  }, [graph, index, visible, selected, focus, colourBy, theme]);

  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const mode = LABEL_MODES[labelMode];
    renderer.setSetting("renderLabels", labelMode !== "none");
    renderer.setSetting("labelRenderedSizeThreshold", mode.threshold);
    renderer.setSetting("labelDensity", mode.density);
    renderer.refresh({ skipIndexation: true });
  }, [labelMode]);

  // Selecting from the search dialog, the inspector or the gap list has to
  // bring the node into view, or the selection is invisible and reads as a
  // no-op. The camera frames the whole neighbourhood rather than the node:
  // zooming to a fixed ratio puts a hub's 35 dependents off-screen, which is
  // exactly the answer the reader asked for.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer || !selected || !graph.hasNode(selected)) return;

    const ids = [
      selected,
      ...(index.dependents.get(selected) ?? []),
      ...(index.dependencies.get(selected) ?? []),
    ];
    const points = ids
      .filter((id) => graph.hasNode(id) && (!visible || visible.has(id)))
      .map((id) => renderer.getNodeDisplayData(id))
      .filter((point) => point !== undefined);
    if (points.length === 0) return;

    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const span = Math.max(
      Math.max(...xs) - Math.min(...xs),
      Math.max(...ys) - Math.min(...ys),
      0.06,
    );

    renderer.getCamera().animate(
      {
        x: (Math.min(...xs) + Math.max(...xs)) / 2,
        y: (Math.min(...ys) + Math.max(...ys)) / 2,
        ratio: Math.min(Math.max(span * 1.35, 0.08), 1.2),
      },
      { duration: 380, easing: "quadraticInOut" },
    );
  }, [selected, graph, index, visible]);

  return <div ref={container} className="absolute inset-0" />;
}
