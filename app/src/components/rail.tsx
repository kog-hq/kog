import {
  Moon,
  Search,
  SlidersHorizontal,
  Sun,
  TriangleAlert,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { CoverageMeter } from "@/components/meter";
import {
  FiltersPanel,
  GapsPanel,
  activeFilterCount,
  type Filters,
} from "@/components/panels";
import type { LabelMode } from "@/graph/graph-canvas";
import type { KogProject, KogWorkspace, ProjectIndex } from "@/lib/kog";
import { formatCount, formatRate, sourceCoverage } from "@/lib/kog";
import { languageColour, type Theme } from "@/lib/palette";

/** The mark from `assets/logo.svg`: a hub, and what depends on it. */
function Mark({ className }: { className?: string }) {
  const spokes = Array.from({ length: 8 }, (_, i) => {
    const angle = (i * Math.PI) / 4;
    return { x: 32 + Math.cos(angle) * 21, y: 32 + Math.sin(angle) * 21 };
  });
  return (
    <svg viewBox="0 0 64 64" className={className} role="img" aria-label="KOG">
      <g
        stroke="currentColor"
        strokeWidth="2.75"
        strokeLinecap="round"
        opacity="0.5"
      >
        {spokes.map((spoke) => (
          <path
            key={`s${spoke.x}-${spoke.y}`}
            d={`M32 32 ${spoke.x} ${spoke.y}`}
          />
        ))}
      </g>
      <g fill="currentColor">
        {spokes.map((spoke) => (
          <circle
            key={`c${spoke.x}-${spoke.y}`}
            cx={spoke.x}
            cy={spoke.y}
            r="4.6"
          />
        ))}
      </g>
      <circle cx="32" cy="32" r="9.5" fill="currentColor" />
    </svg>
  );
}

const LABEL_MODES: LabelMode[] = ["none", "hubs", "more", "all"];

/**
 * The instrument column.
 *
 * Three fixed zones and one scrolling one. The two numbers the project
 * exists to publish sit at the top and never move; the languages below are
 * the legend for the canvas and the language filter at the same time, and
 * are the only thing that scrolls. Everything occasional — the filters, the
 * list of gaps — hangs off two buttons at the foot.
 *
 * An earlier attempt put all of this behind drawers and left the screen
 * empty. It read as calmer and measured less: a tool whose whole argument is
 * a number it publishes cannot put that number one click away.
 */
export function Rail({
  workspace,
  project,
  index,
  onProject,
  onSearch,
  filters,
  onFilters,
  groupByFolder,
  onGroupByFolder,
  labelMode,
  onLabelMode,
  theme,
  onTheme,
  onSelect,
  onHover,
}: {
  workspace: KogWorkspace;
  project: KogProject;
  index: ProjectIndex;
  onProject: (index: number) => void;
  onSearch: () => void;
  filters: Filters;
  onFilters: (next: Filters) => void;
  groupByFolder: boolean;
  onGroupByFolder: (value: boolean) => void;
  labelMode: LabelMode;
  onLabelMode: (mode: LabelMode) => void;
  theme: Theme;
  onTheme: () => void;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}) {
  const { stats } = project.graph;
  const coverage = stats.coverage;
  const gapTotal = stats.unresolved + stats.excluded;
  const activeFilters = activeFilterCount(filters);

  return (
    <aside className="flex h-full w-[254px] shrink-0 flex-col border-r border-border bg-card">
      <header className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <Mark className="size-4 shrink-0" />
        <span className="shrink-0 text-[13px] tracking-tight">KOG</span>
        {workspace.projects.length > 1 ? (
          <select
            value={workspace.projects.indexOf(project)}
            onChange={(event) => onProject(Number(event.target.value))}
            aria-label="Project"
            className="row ml-auto h-6 min-w-0 flex-1 cursor-pointer truncate rounded-md px-1.5 text-right text-[12px] outline-none"
          >
            {workspace.projects.map((entry, position) => (
              <option key={entry.id} value={position}>
                {entry.id}
              </option>
            ))}
          </select>
        ) : (
          <span
            className="ml-auto truncate text-[12px] text-muted-foreground"
            title={project.path}
          >
            {project.name}
          </span>
        )}
      </header>

      <button
        type="button"
        onClick={onSearch}
        className="row flex shrink-0 items-center gap-2 border-b border-border px-3 py-2 text-left text-[12px] text-muted-foreground"
      >
        <Search className="size-3.5" />
        <span className="flex-1">Find a file</span>
        <kbd className="rounded border border-border px-1 py-0.5 text-[10px]">
          ⌘K
        </kbd>
      </button>

      {/* The thesis, at the top, never scrolled away. */}
      <section className="shrink-0 border-b border-border px-3 py-3.5">
        <div className="num text-[32px] leading-none tracking-tight">
          {formatRate(stats.resolution_rate)}
        </div>
        <div className="eyebrow mt-1.5">resolution rate</div>

        <CoverageMeter coverage={coverage} className="mt-3.5" />
        <div className="mt-2 flex items-baseline justify-between text-[11px] text-muted-foreground">
          <span className="num">
            <span className="text-foreground">
              {formatCount(coverage.files_analysed)}
            </span>{" "}
            read
          </span>
          {coverage.files_unsupported > 0 && (
            <span className="num">
              {formatCount(coverage.files_unsupported)} not read
            </span>
          )}
          <span className="num text-foreground">
            {(sourceCoverage(coverage) * 100).toFixed(1)}%
          </span>
        </div>
      </section>

      {/* The only thing that scrolls: the legend, which is also the filter. */}
      <div className="scrollbar-slim min-h-0 flex-1 overflow-y-auto px-3 py-3">
        <header className="mb-2.5 flex items-baseline justify-between">
          <h2 className="eyebrow">Languages</h2>
          <span className="num text-[11px] text-muted-foreground">
            {index.languages.length}
          </span>
        </header>
        <ul className="flex flex-col">
          {index.languages.map((row) => {
            const active =
              !filters.languages || filters.languages.has(row.lang);
            return (
              <li key={row.lang}>
                <button
                  type="button"
                  aria-pressed={active}
                  onClick={() => {
                    const current =
                      filters.languages ??
                      new Set(index.languages.map((l) => l.lang));
                    const next = new Set(current);
                    if (next.has(row.lang)) next.delete(row.lang);
                    else next.add(row.lang);
                    onFilters({
                      ...filters,
                      languages:
                        next.size === index.languages.length ? null : next,
                    });
                  }}
                  className={cn(
                    "row flex w-full items-center gap-2.5 rounded-md px-1.5 py-1.5 text-left",
                    !active && "opacity-35",
                  )}
                >
                  {/* The same colour the canvas paints that language: this
                      list is the legend, so it must not invent one. */}
                  <span
                    className="size-2.5 shrink-0 rounded-full"
                    style={{
                      background: languageColour(row.lang, false, theme),
                    }}
                  />
                  <span className="flex-1 truncate text-[12px]">
                    {row.lang}
                  </span>
                  <span className="num text-[11px] text-muted-foreground">
                    {row.files}
                  </span>
                  <span className="num w-[56px] shrink-0 text-right text-[11px] text-muted-foreground">
                    {row.unread ? "not read" : formatRate(row.rate)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>

      <footer className="flex shrink-0 items-center gap-1 border-t border-border px-2 py-2">
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="row flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px]"
            >
              <SlidersHorizontal className="size-3.5 text-muted-foreground" />
              <span>Filters</span>
              {activeFilters > 0 && (
                <span className="num rounded bg-muted px-1 text-[10px] text-muted-foreground">
                  {activeFilters}
                </span>
              )}
            </button>
          </PopoverTrigger>
          <PopoverContent side="right" align="end" className="w-[280px] p-0">
            <FiltersPanel
              index={index}
              filters={filters}
              onFilters={onFilters}
              groupByFolder={groupByFolder}
              onGroupByFolder={onGroupByFolder}
              theme={theme}
              showLanguages={false}
            />
          </PopoverContent>
        </Popover>

        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="row flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px]"
            >
              <TriangleAlert className="size-3.5 text-muted-foreground" />
              <span className="num">{formatCount(gapTotal)}</span>
              <span>gaps</span>
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="right"
            align="end"
            className="scrollbar-slim max-h-[70vh] w-[380px] overflow-y-auto p-0"
          >
            <GapsPanel
              diagnostics={stats.diagnostics}
              total={gapTotal}
              onSelect={onSelect}
              onHover={onHover}
            />
          </PopoverContent>
        </Popover>

        <select
          value={labelMode}
          onChange={(event) => onLabelMode(event.target.value as LabelMode)}
          aria-label="Labels"
          className="row ml-auto h-7 cursor-pointer rounded-md px-1 text-[11px] outline-none"
        >
          {LABEL_MODES.map((mode) => (
            <option key={mode} value={mode}>
              {mode}
            </option>
          ))}
        </select>

        <Button
          variant="ghost"
          size="icon"
          onClick={onTheme}
          aria-label={theme === "dark" ? "Switch to light" : "Switch to dark"}
          className="size-7 shrink-0"
        >
          {theme === "dark" ? (
            <Sun className="size-3.5" />
          ) : (
            <Moon className="size-3.5" />
          )}
        </Button>
      </footer>
    </aside>
  );
}
