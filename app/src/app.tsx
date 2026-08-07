import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  GraphCanvas,
  defaultLabelMode,
  type EdgeMode,
  type LabelMode,
} from "@/graph/graph-canvas";
import { FindFile } from "@/components/find-file";
import { Inspector } from "@/components/inspector";
import { Rail } from "@/components/rail";
import { detectCommunities } from "@/graph/communities";
import type { ColourBy } from "@/graph/build";
import { NO_FILTERS, type Filters } from "@/components/panels";
import { indexProject, type KogWorkspace } from "@/lib/kog";
import { graphToPng } from "@/lib/palette";

type Theme = "light" | "dark";

const THEME_KEY = "kog:theme";

function storedTheme(): Theme {
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

export function App({ workspace }: { workspace: KogWorkspace }) {
  const [projectIndex, setProjectIndex] = useState(0);
  const [theme, setTheme] = useState<Theme>(storedTheme);
  const [selected, setSelected] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [groupByFolder, setGroupByFolder] = useState(false);
  // Community by default: on a repository that is 84 % one language, colouring
  // by language paints one colour and says nothing. Communities are what the
  // graph itself found.
  const [colourBy, setColourBy] = useState<ColourBy>("community");
  /** Communities the reader has switched off; empty means all of them. */
  const [hiddenCommunities, setHiddenCommunities] = useState<Set<number>>(
    new Set(),
  );
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const [edgeMode, setEdgeMode] = useState<EdgeMode>("linked");

  const project = workspace.projects[projectIndex];
  const index = useMemo(() => indexProject(project), [project]);
  const communities = useMemo(() => detectCommunities(project), [project]);
  const [labelMode, setLabelMode] = useState<LabelMode>(() =>
    defaultLabelMode(project.graph.nodes.length),
  );

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  // A new project is a new graph: the old selection names a file that is not
  // in it, and its size may call for a different number of labels.
  useEffect(() => {
    setSelected(null);
    setHiddenCommunities(new Set());
    setLabelMode(defaultLabelMode(project.graph.nodes.length));
  }, [project]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key === "k") {
        event.preventDefault();
        setSearching(true);
      } else if (
        event.key === "/" &&
        !(event.target instanceof HTMLInputElement)
      ) {
        event.preventDefault();
        setSearching(true);
      } else if (event.key === "Escape") {
        setSelected(null);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  /** Ids that survive the filters, or `null` when nothing is filtered out. */
  const visible = useMemo(() => {
    const everything =
      hiddenCommunities.size === 0 &&
      filters.languages === null &&
      filters.kinds.size === NO_FILTERS.kinds.size &&
      !filters.hideIsolated &&
      !filters.onlyUnresolved;
    if (everything) return null;

    const keep = new Set<string>();
    for (const node of project.graph.nodes) {
      if (!filters.kinds.has(node.kind)) continue;
      const community = communities.byNode.get(node.id);
      if (community !== undefined && hiddenCommunities.has(community)) continue;
      // Only code answers to the language filter. An asset's `lang` is a file
      // family — `image`, `font`, `documentation` — and none of those are in
      // the language list, so testing them against it silently removed all 71
      // assets the moment any one language was unticked. Assets have their
      // own checkbox, under Show.
      if (
        filters.languages &&
        node.kind !== "asset" &&
        !filters.languages.has(node.lang)
      )
        continue;
      if (filters.hideIsolated && (index.degree.get(node.id) ?? 0) === 0)
        continue;
      if (filters.onlyUnresolved && !index.diagnosticsByFile.has(node.id))
        continue;
      keep.add(node.id);
    }
    if (!groupByFolder) return keep;

    // Folders are drawn, not files: a folder stays as long as one of its
    // files does.
    const folders = new Set<string>();
    for (const id of keep) {
      const cut = id.lastIndexOf("/");
      folders.add(cut === -1 ? "." : id.slice(0, cut));
    }
    return folders;
  }, [filters, project, index, groupByFolder, communities, hiddenCommunities]);

  const canvasArea = useRef<HTMLElement>(null);

  /** Download the graph exactly as it is on screen. */
  const onExport = useCallback(() => {
    if (!canvasArea.current) return;
    const png = graphToPng(canvasArea.current, theme);
    if (!png) return;
    const link = document.createElement("a");
    link.href = png;
    link.download = `${project.name || "graph"}.png`;
    link.click();
  }, [project.name, theme]);

  const selectedNode = selected ? index.byId.get(selected) : undefined;
  const onSelect = useCallback((id: string | null) => setSelected(id), []);

  return (
    // The canvas runs the full window and nothing sits on it by default. The
    // instrument reading a map should not be bigger than the map.
    <div className="flex h-full overflow-hidden">
      <Rail
        workspace={workspace}
        project={project}
        index={index}
        communities={communities}
        hiddenCommunities={hiddenCommunities}
        onHiddenCommunities={setHiddenCommunities}
        colourBy={colourBy}
        onColourBy={setColourBy}
        onProject={setProjectIndex}
        onSearch={() => setSearching(true)}
        filters={filters}
        onFilters={setFilters}
        groupByFolder={groupByFolder}
        onGroupByFolder={setGroupByFolder}
        labelMode={labelMode}
        onLabelMode={setLabelMode}
        edgeMode={edgeMode}
        onEdgeMode={setEdgeMode}
        theme={theme}
        onTheme={() => setTheme(theme === "dark" ? "light" : "dark")}
        onSelect={setSelected}
        onHover={setHovered}
        onExport={onExport}
      />

      <main ref={canvasArea} className="relative min-w-0 flex-1">
        <GraphCanvas
          project={project}
          index={index}
          communities={communities}
          visible={visible}
          selected={selected}
          hovered={hovered}
          labelMode={labelMode}
          edgeMode={edgeMode}
          groupByFolder={groupByFolder}
          colourBy={colourBy}
          theme={theme}
          onSelect={onSelect}
        />

        {project.graph.edges.length === 0 && (
          <p className="pointer-events-none absolute inset-x-0 top-1/2 mx-auto max-w-md -translate-y-1/2 px-6 text-center text-[12px] leading-relaxed text-muted-foreground">
            No file in this project imports another. Every dot is real — there
            is simply nothing connecting them, which is worth knowing.
          </p>
        )}

        {selectedNode && (
          <Inspector
            node={selectedNode}
            index={index}
            onSelect={setSelected}
            onHover={setHovered}
            onClose={() => setSelected(null)}
          />
        )}
      </main>

      <FindFile
        open={searching}
        onOpenChange={setSearching}
        nodes={project.graph.nodes}
        index={index}
        onSelect={setSelected}
      />
    </div>
  );
}
