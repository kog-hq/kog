import { useCallback, useEffect, useMemo, useState } from "react";
import {
  GraphCanvas,
  defaultLabelMode,
  type LabelMode,
} from "@/graph/graph-canvas";
import { FindFile } from "@/components/find-file";
import { Inspector } from "@/components/inspector";
import { Rail } from "@/components/rail";
import { NO_FILTERS, type Filters } from "@/components/panels";
import { indexProject, type KogWorkspace } from "@/lib/kog";

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
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);

  const project = workspace.projects[projectIndex];
  const index = useMemo(() => indexProject(project), [project]);
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
      filters.languages === null &&
      filters.kinds.size === NO_FILTERS.kinds.size &&
      !filters.hideIsolated &&
      !filters.onlyUnresolved;
    if (everything) return null;

    const keep = new Set<string>();
    for (const node of project.graph.nodes) {
      if (!filters.kinds.has(node.kind)) continue;
      if (filters.languages && !filters.languages.has(node.lang)) continue;
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
  }, [filters, project, index, groupByFolder]);

  const selectedNode = selected ? index.byId.get(selected) : undefined;
  const onSelect = useCallback((id: string | null) => setSelected(id), []);
  const onHover = useCallback((id: string | null) => setHovered(id), []);

  return (
    // The canvas runs the full window and nothing sits on it by default. The
    // instrument reading a map should not be bigger than the map.
    <div className="flex h-full overflow-hidden">
      <Rail
        workspace={workspace}
        project={project}
        index={index}
        onProject={setProjectIndex}
        onSearch={() => setSearching(true)}
        filters={filters}
        onFilters={setFilters}
        groupByFolder={groupByFolder}
        onGroupByFolder={setGroupByFolder}
        labelMode={labelMode}
        onLabelMode={setLabelMode}
        theme={theme}
        onTheme={() => setTheme(theme === "dark" ? "light" : "dark")}
        onSelect={setSelected}
        onHover={setHovered}
      />

      <main className="relative min-w-0 flex-1">
        <GraphCanvas
          project={project}
          index={index}
          visible={visible}
          selected={selected}
          hovered={hovered}
          labelMode={labelMode}
          groupByFolder={groupByFolder}
          theme={theme}
          onSelect={onSelect}
          onHover={onHover}
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
