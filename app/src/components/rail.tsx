import { Moon, Search, Sun, TriangleAlert } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { CoverageMeter } from "@/components/meter";
import { GapsPanel, type Filters } from "@/components/panels";
import type { EdgeMode, LabelMode } from "@/graph/graph-canvas";
import type { ColourBy } from "@/graph/build";
import type { Communities } from "@/graph/communities";
import type {
  KogProject,
  KogWorkspace,
  NodeKind,
  ProjectIndex,
} from "@/lib/kog";
import { KIND_LABEL, formatCount, formatRate, sourceCoverage } from "@/lib/kog";
import { communityColour, languageColour, type Theme } from "@/lib/palette";

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

function Section({
  title,
  right,
  children,
}: {
  title: string;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-border px-3 py-3">
      <header className="mb-2 flex items-center justify-between gap-2">
        <h2 className="eyebrow">{title}</h2>
        {right}
      </header>
      {children}
    </section>
  );
}

/**
 * A row of buttons, not a select.
 *
 * Every option is on screen and one click away. A dropdown hides the choices
 * you did not make, which on a control you change constantly — how many
 * labels, what the colour means — turns a glance into two clicks and a memory
 * test.
 */
function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: readonly T[];
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="flex rounded-md border border-border p-0.5"
    >
      {options.map((option) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={value === option}
          onClick={() => onChange(option)}
          className={cn(
            "flex-1 rounded px-1.5 py-1 text-[11px] transition-colors",
            value === option
              ? "bg-accent text-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {option}
        </button>
      ))}
    </div>
  );
}

/** A checkbox that shows the colour it controls. */
function Check({
  checked,
  onChange,
  swatch,
  children,
  count,
  note,
}: {
  checked: boolean;
  onChange: () => void;
  swatch?: string;
  children: React.ReactNode;
  count?: number;
  note?: string;
}) {
  return (
    <label className="row flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1">
      <input
        type="checkbox"
        checked={checked}
        onChange={onChange}
        className="size-3 shrink-0 cursor-pointer accent-[--color-focus]"
      />
      {swatch && (
        <span
          className="size-2.5 shrink-0 rounded-full"
          style={{ background: swatch }}
        />
      )}
      <span
        className={cn(
          "flex-1 truncate text-[12px]",
          !checked && "text-muted-foreground",
        )}
      >
        {children}
      </span>
      {note && (
        <span className="text-[10px] text-muted-foreground">{note}</span>
      )}
      {count !== undefined && (
        <span className="num text-[11px] text-muted-foreground">
          {formatCount(count)}
        </span>
      )}
    </label>
  );
}

const LABEL_MODES = ["none", "hubs", "more", "all"] as const;
const EDGE_MODES = ["none", "linked", "all"] as const;
const COLOUR_MODES = ["community", "language"] as const;

/**
 * The instrument column.
 *
 * Everything that changes what is on the canvas is on screen: the colour
 * mode, how many labels, which kinds of file, which communities, which
 * languages. Nothing that filters the graph hides behind a menu.
 */
export function Rail({
  workspace,
  project,
  index,
  communities,
  hiddenCommunities,
  onHiddenCommunities,
  colourBy,
  onColourBy,
  onProject,
  onSearch,
  filters,
  onFilters,
  groupByFolder,
  onGroupByFolder,
  labelMode,
  onLabelMode,
  edgeMode,
  onEdgeMode,
  theme,
  onTheme,
  onSelect,
  onHover,
}: {
  workspace: KogWorkspace;
  project: KogProject;
  index: ProjectIndex;
  communities: Communities;
  hiddenCommunities: Set<number>;
  onHiddenCommunities: (next: Set<number>) => void;
  colourBy: ColourBy;
  onColourBy: (mode: ColourBy) => void;
  onProject: (index: number) => void;
  onSearch: () => void;
  filters: Filters;
  onFilters: (next: Filters) => void;
  groupByFolder: boolean;
  onGroupByFolder: (value: boolean) => void;
  labelMode: LabelMode;
  onLabelMode: (mode: LabelMode) => void;
  edgeMode: EdgeMode;
  onEdgeMode: (mode: EdgeMode) => void;
  theme: Theme;
  onTheme: () => void;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}) {
  const { stats } = project.graph;
  const coverage = stats.coverage;
  const gapTotal = stats.unresolved + stats.excluded;
  const allCommunities = hiddenCommunities.size === 0;

  function toggleCommunity(id: number) {
    const next = new Set(hiddenCommunities);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onHiddenCommunities(next);
  }

  function toggleLanguage(lang: string) {
    const current =
      filters.languages ?? new Set(index.languages.map((l) => l.lang));
    const next = new Set(current);
    if (next.has(lang)) next.delete(lang);
    else next.add(lang);
    onFilters({
      ...filters,
      languages: next.size === index.languages.length ? null : next,
    });
  }

  function toggleKind(kind: NodeKind) {
    const next = new Set(filters.kinds);
    if (next.has(kind)) next.delete(kind);
    else next.add(kind);
    onFilters({ ...filters, kinds: next });
  }

  return (
    <aside className="flex h-full w-[268px] shrink-0 flex-col border-r border-border bg-card">
      <header className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2.5">
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
        <div className="num text-[30px] leading-none tracking-tight">
          {formatRate(stats.resolution_rate)}
        </div>
        <div className="eyebrow mt-1.5">resolution rate</div>
        <CoverageMeter coverage={coverage} className="mt-3" />
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

      <div className="scrollbar-slim min-h-0 flex-1 overflow-y-auto">
        <Section title="Colour">
          <Segmented
            label="What colour means"
            value={colourBy}
            options={COLOUR_MODES}
            onChange={onColourBy}
          />
        </Section>

        <Section title="Labels">
          <Segmented
            label="How many labels"
            value={labelMode}
            options={LABEL_MODES}
            onChange={onLabelMode}
          />
        </Section>

        <Section title="Edges">
          <Segmented
            label="How many edges"
            value={edgeMode}
            options={EDGE_MODES}
            onChange={onEdgeMode}
          />
          <p className="mt-2 px-0.5 text-[11px] leading-relaxed text-muted-foreground">
            {edgeMode === "linked"
              ? "Only edges touching a file on screen. Zoom out to see them all."
              : edgeMode === "all"
                ? "Every edge, including those crossing the view between two files you cannot see."
                : "No edges. Position and colour still carry the structure."}
          </p>
        </Section>

        <Section title="Show">
          <div className="flex flex-col">
            {index.kinds.map(({ kind, count }) => (
              <Check
                key={kind}
                checked={filters.kinds.has(kind)}
                onChange={() => toggleKind(kind)}
                count={count}
              >
                {KIND_LABEL[kind]}
              </Check>
            ))}
            <Check
              checked={filters.hideIsolated}
              onChange={() =>
                onFilters({ ...filters, hideIsolated: !filters.hideIsolated })
              }
            >
              hide unconnected
            </Check>
            <Check
              checked={filters.onlyUnresolved}
              onChange={() =>
                onFilters({
                  ...filters,
                  onlyUnresolved: !filters.onlyUnresolved,
                })
              }
            >
              only files with gaps
            </Check>
            <Check
              checked={groupByFolder}
              onChange={() => onGroupByFolder(!groupByFolder)}
            >
              group by folder
            </Check>
          </div>
        </Section>

        {/* What the graph found on its own: the sets of files that import each
            other far more than they import the rest. */}
        <Section
          title="Communities"
          right={
            <button
              type="button"
              onClick={() =>
                onHiddenCommunities(
                  allCommunities
                    ? new Set(communities.list.map((c) => c.id))
                    : new Set(),
                )
              }
              className="text-[10px] text-muted-foreground hover:text-foreground"
            >
              {allCommunities ? "none" : "all"}
            </button>
          }
        >
          <div className="flex flex-col">
            {communities.list.map((community) => (
              <Check
                key={community.id}
                checked={!hiddenCommunities.has(community.id)}
                onChange={() => toggleCommunity(community.id)}
                swatch={communityColour(community.id, theme)}
                count={community.size}
              >
                {community.name}
              </Check>
            ))}
          </div>
        </Section>

        <Section
          title="Languages"
          right={
            <span className="num text-[11px] text-muted-foreground">
              {index.languages.length}
            </span>
          }
        >
          <div className="flex flex-col">
            {index.languages.map((row) => (
              <Check
                key={row.lang}
                checked={!filters.languages || filters.languages.has(row.lang)}
                onChange={() => toggleLanguage(row.lang)}
                swatch={languageColour(row.lang, false, theme)}
                count={row.files}
                note={row.unread ? "not read" : formatRate(row.rate)}
              >
                {row.lang}
              </Check>
            ))}
          </div>
        </Section>
      </div>

      <footer className="flex shrink-0 items-center gap-1 border-t border-border px-2 py-2">
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              className="row flex h-7 flex-1 items-center gap-1.5 rounded-md px-2 text-left text-[12px]"
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
