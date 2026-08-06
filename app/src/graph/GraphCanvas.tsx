import Sigma from "sigma";
import type { Settings } from "sigma/settings";
import type { NodeDisplayData, PartialButFor } from "sigma/types";
import { useEffect, useMemo, useRef } from "react";
import type Graph from "graphology";
import { buildGraph, readCanvasTheme, recolour, type CanvasTheme } from "./build";
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

export type CanvasState = {
  /** Ids that pass the current filters. Everything else is hidden. */
  visible: Set<string> | null;
  selected: string | null;
  /** The selection's immediate neighbourhood, selection included. */
  neighbourhood: Set<string> | null;
  labelMode: LabelMode;
  groupByFolder: boolean;
  /** Repaint trigger for a theme change. */
  theme: string;
};

type Props = CanvasState & {
  project: KogProject;
  index: ProjectIndex;
  onSelect: (id: string | null) => void;
  onHover: (id: string | null) => void;
  onReady?: (graph: Graph) => void;
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

export function GraphCanvas(props: Props) {
  const {
    project,
    index,
    visible,
    selected,
    neighbourhood,
    labelMode,
    groupByFolder,
    theme,
    onSelect,
    onHover,
    onReady,
  } = props;

  const container = useRef<HTMLDivElement>(null);
  const sigma = useRef<Sigma | null>(null);
  const hovered = useRef<string | null>(null);

  // The layout is the expensive part, so it is tied to what actually changes
  // its shape: the project and whether folders are collapsed. Filters and
  // selection never reach here.
  const graph = useMemo(
    () => buildGraph(project, index, readCanvasTheme(), { groupByFolder }),
    [project, index, groupByFolder],
  );

  useEffect(() => {
    if (!container.current) return;
    const canvasTheme = readCanvasTheme();
    const renderer = new Sigma(graph, container.current, {
      renderEdgeLabels: false,
      defaultEdgeColor: canvasTheme.edge,
      labelFont: '"JetBrains Mono Variable", ui-monospace, monospace',
      labelSize: 11.5,
      labelWeight: "500",
      labelGridCellSize: 72,
      defaultDrawNodeLabel: labelDrawer(canvasTheme),
      defaultDrawNodeHover: labelDrawer(canvasTheme, true),
      zIndex: true,
    });
    sigma.current = renderer;
    onReady?.(graph);

    renderer.on("clickNode", ({ node }) => onSelect(node));
    renderer.on("clickStage", () => onSelect(null));
    renderer.on("enterNode", ({ node }) => {
      hovered.current = node;
      onHover(node);
      renderer.refresh({ skipIndexation: true });
    });
    renderer.on("leaveNode", () => {
      hovered.current = null;
      onHover(null);
      renderer.refresh({ skipIndexation: true });
    });

    return () => {
      renderer.kill();
      sigma.current = null;
    };
  }, [graph, onSelect, onHover, onReady]);

  // Filtering, selection and hover are all one repaint: sigma asks these
  // reducers what each node and edge looks like right now.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const canvasTheme = readCanvasTheme();
    recolour(graph, canvasTheme);

    const highlight = neighbourhood;

    renderer.setSetting("nodeReducer", (node, data) => {
      if (visible && !visible.has(node)) return { ...data, hidden: true };
      if (node === selected) {
        return {
          ...data,
          color: canvasTheme.focus,
          size: data.size * 1.5,
          zIndex: 2,
          forceLabel: true,
        };
      }
      if (highlight) {
        if (highlight.has(node)) return { ...data, zIndex: 1, forceLabel: true };
        // Dimmed and shrunk rather than hidden: the shape of the whole is
        // the context that makes a neighbourhood mean anything, but it has
        // to stop competing for attention while you read one.
        return {
          ...data,
          color: canvasTheme.edge,
          size: data.size * 0.55,
          label: "",
          zIndex: 0,
        };
      }
      if (node === hovered.current) return { ...data, forceLabel: true, zIndex: 2 };
      return data;
    });

    renderer.setSetting("edgeReducer", (edge, data) => {
      const [source, target] = graph.extremities(edge);
      if (visible && (!visible.has(source) || !visible.has(target))) {
        return { ...data, hidden: true };
      }
      if (highlight) {
        if (highlight.has(source) && highlight.has(target)) {
          return { ...data, color: canvasTheme.focus, size: 1.2, zIndex: 1 };
        }
        return { ...data, color: canvasTheme.edge, size: 0.2 };
      }
      return { ...data, color: canvasTheme.edge, size: 0.45 };
    });

    renderer.setSetting("defaultDrawNodeLabel", labelDrawer(canvasTheme));
    renderer.setSetting("defaultDrawNodeHover", labelDrawer(canvasTheme, true));
    renderer.setSetting("defaultEdgeColor", canvasTheme.edge);
    renderer.refresh();
  }, [graph, visible, selected, neighbourhood, theme]);

  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer) return;
    const mode = LABEL_MODES[labelMode];
    renderer.setSetting("renderLabels", labelMode !== "none");
    renderer.setSetting("labelRenderedSizeThreshold", mode.threshold);
    renderer.setSetting("labelDensity", mode.density);
    renderer.refresh({ skipIndexation: true });
  }, [labelMode]);

  // Selecting from the search dialog or the diagnostics list must bring the
  // node into view, or the selection is invisible and reads as a no-op.
  //
  // The camera frames the whole neighbourhood, not the node: zooming to a
  // fixed ratio puts a hub's 35 dependents off-screen, which is exactly the
  // answer the reader asked for.
  useEffect(() => {
    const renderer = sigma.current;
    if (!renderer || !selected || !graph.hasNode(selected)) return;

    const points = [...(neighbourhood ?? new Set([selected]))]
      .map((id) => renderer.getNodeDisplayData(id))
      .filter((point) => point !== undefined);
    if (points.length === 0) return;

    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const width = Math.max(...xs) - Math.min(...xs);
    const height = Math.max(...ys) - Math.min(...ys);
    const span = Math.max(width, height, 0.06);

    renderer.getCamera().animate(
      {
        x: (Math.min(...xs) + Math.max(...xs)) / 2,
        y: (Math.min(...ys) + Math.max(...ys)) / 2,
        ratio: Math.min(Math.max(span * 1.35, 0.08), 1.2),
      },
      { duration: 340 },
    );
  }, [selected, neighbourhood, graph]);

  return <div ref={container} className="absolute inset-0" />;
}
