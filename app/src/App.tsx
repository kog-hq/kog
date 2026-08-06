import { useCallback, useEffect, useMemo, useState } from "react";
import { GraphCanvas, defaultLabelMode, type LabelMode } from "@/graph/GraphCanvas";
import { FindFile } from "@/components/FindFile";
import { Inspector } from "@/components/Inspector";
import { Sidebar, type Filters } from "@/components/Sidebar";
import { TopRail } from "@/components/TopRail";
import {
  indexProject,
  formatCount,
  formatRate,
  type KogWorkspace,
  type NodeKind,
} from "@/lib/kog";

type Theme = "light" | "dark";

const THEME_KEY = "kog:theme";

function storedTheme(): Theme {
  const stored = localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

const ALL_KINDS: NodeKind[] = ["source", "unread_source", "asset"];

export function App({ workspace }: { workspace: KogWorkspace }) {
  const [projectIndex, setProjectIndex] = useState(0);
  const [theme, setTheme] = useState<Theme>(storedTheme);
  const [selected, setSelected] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [groupByFolder, setGroupByFolder] = useState(false);
  const [filters, setFilters] = useState<Filters>({
    languages: null,
    kinds: new Set(ALL_KINDS),
    hideIsolated: false,
    onlyUnresolved: false,
  });

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
      } else if (event.key === "/" && !(event.target instanceof HTMLInputElement)) {
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
      filters.kinds.size === ALL_KINDS.length &&
      !filters.hideIsolated &&
      !filters.onlyUnresolved;
    if (everything) return null;

    const keep = new Set<string>();
    for (const node of project.graph.nodes) {
      if (!filters.kinds.has(node.kind)) continue;
      if (filters.languages && !filters.languages.has(node.lang)) continue;
      if (filters.hideIsolated && (index.degree.get(node.id) ?? 0) === 0) continue;
      if (filters.onlyUnresolved && !index.diagnosticsByFile.has(node.id)) continue;
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

  const stats = project.graph.stats;

  return (
    <div className="flex h-full flex-col">
      <TopRail
        workspace={workspace}
        project={project}
        onProject={setProjectIndex}
        onSearch={() => setSearching(true)}
        labelMode={labelMode}
        onLabelMode={setLabelMode}
        theme={theme}
        onTheme={() => setTheme(theme === "dark" ? "light" : "dark")}
      />

      <div className="flex min-h-0 flex-1">
        <Sidebar
          project={project}
          index={index}
          filters={filters}
          onFilters={setFilters}
          groupByFolder={groupByFolder}
          onGroupByFolder={setGroupByFolder}
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
            <div className="pointer-events-none absolute inset-x-0 top-1/2 mx-auto max-w-md -translate-y-1/2 px-6 text-center text-[12px] leading-relaxed text-muted-foreground">
              No file in this project imports another. Every dot is real — there is simply
              nothing connecting them, which is worth knowing.
            </div>
          )}
        </main>

        {selectedNode && (
          <Inspector
            node={selectedNode}
            index={index}
            onSelect={setSelected}
            onHover={setHovered}
            onClose={() => setSelected(null)}
          />
        )}
      </div>

      <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-card px-3 text-[11px] text-muted-foreground">
        <span className="num">
          <span className="text-foreground">{formatCount(stats.coverage.files_seen)}</span> files
        </span>
        <span className="num">
          <span className="text-foreground">{formatCount(project.graph.edges.length)}</span> edges
        </span>
        <span className="num">
          <span className="text-foreground">{formatRate(stats.resolution_rate)}</span> resolution
        </span>
        {stats.coverage.files_unsupported > 0 && (
          <span className="num text-signal">
            {formatCount(stats.coverage.files_unsupported)} files not read
          </span>
        )}
        {visible && (
          <span className="num">
            showing {formatCount(visible.size)} of{" "}
            {formatCount(project.graph.nodes.length)}
          </span>
        )}
        <span className="ml-auto truncate" title={project.path}>
          {project.path}
        </span>
      </footer>

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
