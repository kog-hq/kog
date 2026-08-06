import { Moon, Search, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { CoverageMeter } from "@/components/Meter";
import type { LabelMode } from "@/graph/GraphCanvas";
import type { KogProject, KogWorkspace } from "@/lib/kog";
import { formatCount, formatRate, sourceCoverage } from "@/lib/kog";

/** The mark from `assets/logo.svg`: a hub and what depends on it. */
function Mark({ className }: { className?: string }) {
  const spokes = Array.from({ length: 8 }, (_, i) => {
    const angle = (i * Math.PI) / 4;
    return { x: 32 + Math.cos(angle) * 21, y: 32 + Math.sin(angle) * 21 };
  });
  return (
    <svg viewBox="0 0 64 64" className={className} role="img" aria-label="KOG">
      <g stroke="currentColor" strokeWidth="2.75" strokeLinecap="round" opacity="0.55">
        {spokes.map((spoke) => (
          <path key={`${spoke.x}-${spoke.y}`} d={`M32 32 ${spoke.x} ${spoke.y}`} />
        ))}
      </g>
      <g fill="currentColor">
        {spokes.map((spoke) => (
          <circle key={`c-${spoke.x}-${spoke.y}`} cx={spoke.x} cy={spoke.y} r="4.6" />
        ))}
      </g>
      <circle cx="32" cy="32" r="9.5" fill="currentColor" />
    </svg>
  );
}

const LABEL_MODES: LabelMode[] = ["none", "hubs", "more", "all"];

export function TopRail({
  workspace,
  project,
  onProject,
  onSearch,
  labelMode,
  onLabelMode,
  theme,
  onTheme,
}: {
  workspace: KogWorkspace;
  project: KogProject;
  onProject: (index: number) => void;
  onSearch: () => void;
  labelMode: LabelMode;
  onLabelMode: (mode: LabelMode) => void;
  theme: "light" | "dark";
  onTheme: () => void;
}) {
  const coverage = project.graph.stats.coverage;
  const root = workspace.root.split("/").filter(Boolean).pop() ?? workspace.root;

  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-border bg-card px-3">
      <div className="flex shrink-0 items-center gap-2">
        <Mark className="size-4 text-foreground" />
        <span className="text-[13px] tracking-tight">KOG</span>
      </div>

      <div className="h-4 w-px bg-border" />

      {/* One graph per project: the picker is how a directory of codebases
          says so, instead of merging them into one shape. */}
      {workspace.projects.length > 1 ? (
        <label className="flex min-w-0 items-center gap-2">
          <span className="sr-only">Project</span>
          <select
            value={workspace.projects.indexOf(project)}
            onChange={(event) => onProject(Number(event.target.value))}
            className="h-7 max-w-[280px] cursor-pointer truncate rounded-md border border-border bg-background px-2 text-[12px] outline-none hover:bg-accent"
          >
            {workspace.projects.map((entry, position) => (
              <option key={entry.id} value={position}>
                {entry.id} · {formatCount(entry.graph.nodes.length)} files
              </option>
            ))}
          </select>
        </label>
      ) : (
        <span className="truncate text-[12px] text-muted-foreground" title={workspace.root}>
          {root}
        </span>
      )}

      <button
        type="button"
        onClick={onSearch}
        className="row flex h-7 min-w-[180px] flex-1 items-center gap-2 rounded-md border border-border px-2 text-left text-[12px] text-muted-foreground"
      >
        <Search className="size-3.5" />
        <span className="flex-1">Find a file</span>
        <kbd className="rounded border border-border px-1 py-0.5 text-[10px]">⌘K</kbd>
      </button>

      {/* The thesis, always on screen: how much of this codebase is really
          on the map. */}
      <div className="hidden w-[190px] shrink-0 md:block">
        <CoverageMeter coverage={coverage} />
        <div className="mt-1 flex items-baseline justify-between text-[10px] text-muted-foreground">
          <span className="eyebrow">coverage</span>
          <span className="num">
            {(sourceCoverage(coverage) * 100).toFixed(1)}% ·{" "}
            {formatRate(project.graph.stats.resolution_rate)}
          </span>
        </div>
      </div>

      <label className="flex shrink-0 items-center gap-1.5">
        <span className="sr-only">Labels</span>
        <select
          value={labelMode}
          onChange={(event) => onLabelMode(event.target.value as LabelMode)}
          className="h-7 cursor-pointer rounded-md border border-border bg-background px-2 text-[12px] outline-none hover:bg-accent"
        >
          {LABEL_MODES.map((mode) => (
            <option key={mode} value={mode}>
              labels: {mode}
            </option>
          ))}
        </select>
      </label>

      <Button
        variant="ghost"
        size="icon"
        onClick={onTheme}
        aria-label={theme === "dark" ? "Switch to light" : "Switch to dark"}
        className={cn("size-7 shrink-0")}
      >
        {theme === "dark" ? <Sun className="size-3.5" /> : <Moon className="size-3.5" />}
      </Button>
    </header>
  );
}
