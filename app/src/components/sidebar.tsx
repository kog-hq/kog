import { AlertTriangle, EyeOff, Layers, Unplug } from "lucide-react";
import { cn } from "@/lib/utils";
import { Switch } from "@/components/ui/switch";
import { CoverageMeter, Figure } from "@/components/meter";
import type {
  KogDiagnostic,
  KogProject,
  NodeKind,
  ProjectIndex,
} from "@/lib/kog";
import { KIND_LABEL, formatCount, formatRate } from "@/lib/kog";
import { languageColour } from "@/lib/palette";

export type Filters = {
  /** `null` means every language; a set means only these. */
  languages: Set<string> | null;
  kinds: Set<NodeKind>;
  hideIsolated: boolean;
  onlyUnresolved: boolean;
};

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-border px-3 py-3 last:border-b-0">
      <header className="mb-2.5 flex items-baseline justify-between">
        <h2 className="eyebrow">{title}</h2>
        {count && (
          <span className="num text-[11px] text-muted-foreground">{count}</span>
        )}
      </header>
      {children}
    </section>
  );
}

function Toggle({
  checked,
  onChange,
  icon,
  children,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <label className="row flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1">
      <span className="text-muted-foreground">{icon}</span>
      <span className="flex-1 text-[12px]">{children}</span>
      <Switch
        checked={checked}
        onCheckedChange={onChange}
        className="scale-75"
      />
    </label>
  );
}

export function Sidebar({
  project,
  index,
  filters,
  onFilters,
  groupByFolder,
  onGroupByFolder,
  theme,
  onSelect,
  onHover,
}: {
  project: KogProject;
  index: ProjectIndex;
  filters: Filters;
  onFilters: (next: Filters) => void;
  groupByFolder: boolean;
  onGroupByFolder: (value: boolean) => void;
  theme: "light" | "dark";
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}) {
  const { stats } = project.graph;
  const gaps = stats.coverage.extensions.filter(
    (e) => e.status === "unsupported_language",
  );

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
    <aside className="glass scrollbar-slim absolute bottom-11 left-3 top-[60px] flex w-[252px] flex-col overflow-y-auto">
      <Section title="Measured">
        <div className="mb-3 flex items-start justify-between gap-3">
          <Figure
            label="resolution"
            value={formatRate(stats.resolution_rate)}
            title="Resolved over in-scope internal specifiers"
          />
          <Figure
            label="unresolved"
            value={formatCount(stats.unresolved)}
            tone={stats.unresolved > 0 ? "signal" : "default"}
            title="Imports that name a file KOG could not find"
          />
        </div>
        <CoverageMeter coverage={stats.coverage} showLegend />
      </Section>

      <Section title="Languages" count={`${index.languages.length}`}>
        <ul className="flex flex-col">
          {index.languages.map((row) => {
            const active =
              !filters.languages || filters.languages.has(row.lang);
            return (
              <li key={row.lang}>
                <button
                  type="button"
                  onClick={() => toggleLanguage(row.lang)}
                  aria-pressed={active}
                  className={cn(
                    "row flex w-full items-center gap-2 rounded-md px-1.5 py-1 text-left",
                    !active && "opacity-35",
                  )}
                >
                  {/* The same colour the canvas draws that language in:
                      this list is the legend, so it must not invent one. */}
                  <span
                    className={cn(
                      "size-2 shrink-0 rounded-full",
                      row.unread &&
                        "ring-1 ring-signal ring-offset-1 ring-offset-card",
                    )}
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
                  <span
                    className={cn(
                      "num w-[52px] text-right text-[11px]",
                      row.unread
                        ? "text-signal"
                        : row.rate < 1
                          ? "text-foreground"
                          : "text-muted-foreground",
                    )}
                  >
                    {row.unread ? "not read" : formatRate(row.rate)}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </Section>

      <Section title="Show">
        <div className="flex flex-col gap-0.5">
          {index.kinds.map(({ kind, count }) => (
            <label
              key={kind}
              className="row flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1"
            >
              <span
                className={cn(
                  "size-1.5 shrink-0 rounded-full",
                  kind === "unread_source"
                    ? "bg-signal"
                    : kind === "asset"
                      ? "bg-muted-foreground/50"
                      : "bg-foreground/60",
                )}
              />
              <span className="flex-1 text-[12px]">{KIND_LABEL[kind]}</span>
              <span className="num text-[11px] text-muted-foreground">
                {formatCount(count)}
              </span>
              <Switch
                checked={filters.kinds.has(kind)}
                onCheckedChange={() => toggleKind(kind)}
                className="scale-75"
              />
            </label>
          ))}
          <Toggle
            checked={filters.hideIsolated}
            onChange={(value) => onFilters({ ...filters, hideIsolated: value })}
            icon={<Unplug className="size-3.5" />}
          >
            hide unconnected
          </Toggle>
          <Toggle
            checked={filters.onlyUnresolved}
            onChange={(value) =>
              onFilters({ ...filters, onlyUnresolved: value })
            }
            icon={<AlertTriangle className="size-3.5" />}
          >
            files with gaps
          </Toggle>
          <Toggle
            checked={groupByFolder}
            onChange={onGroupByFolder}
            icon={<Layers className="size-3.5" />}
          >
            group by folder
          </Toggle>
        </div>
      </Section>

      {gaps.length > 0 && (
        <Section
          title="Not read"
          count={formatCount(stats.coverage.files_unsupported)}
        >
          <ul className="flex flex-col gap-0.5">
            {gaps.map((gap) => (
              <li
                key={`${gap.extension}-${gap.status}`}
                className="flex items-center gap-2 px-1.5 py-0.5"
              >
                <span className="text-signal">{gap.label}</span>
                <span className="flex-1 truncate text-[11px] text-muted-foreground">
                  {gap.lang ?? "unrecognised"}
                </span>
                <span className="num text-[11px] text-muted-foreground">
                  {formatCount(gap.count)}
                </span>
              </li>
            ))}
          </ul>
          <p className="mt-2 px-1.5 text-[11px] leading-relaxed text-muted-foreground">
            These files are in the graph, but their own imports are not: KOG has
            no extractor for them yet.
          </p>
        </Section>
      )}

      {stats.diagnostics.length > 0 && (
        <Diagnostics
          diagnostics={stats.diagnostics}
          total={stats.unresolved + stats.excluded}
          onSelect={onSelect}
          onHover={onHover}
        />
      )}

      {stats.coverage.skipped_directories.length > 0 && (
        <Section title="Never entered">
          <ul className="flex flex-col gap-0.5">
            {stats.coverage.skipped_directories.map((dir) => (
              <li
                key={dir.name}
                className="flex items-center gap-2 px-1.5 py-0.5"
              >
                <EyeOff className="size-3 text-muted-foreground" />
                <span className="flex-1 truncate text-[12px]">{dir.name}</span>
                <span className="num text-[11px] text-muted-foreground">
                  ×{dir.count}
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </aside>
  );
}

function Diagnostics({
  diagnostics,
  total,
  onSelect,
  onHover,
}: {
  diagnostics: KogDiagnostic[];
  total: number;
  onSelect: (id: string) => void;
  onHover: (id: string | null) => void;
}) {
  const shown = diagnostics.slice(0, 60);
  return (
    <Section
      title="Gaps"
      count={
        diagnostics.length < total
          ? `${formatCount(diagnostics.length)} of ${formatCount(total)}`
          : formatCount(total)
      }
    >
      <ul className="flex flex-col">
        {shown.map((diagnostic, position) => (
          <li key={`${diagnostic.path}:${diagnostic.line}:${position}`}>
            <button
              type="button"
              onClick={() => onSelect(diagnostic.path)}
              onMouseEnter={() => onHover(diagnostic.path)}
              onMouseLeave={() => onHover(null)}
              className="row w-full rounded-md px-1.5 py-1 text-left"
              title={`${diagnostic.path}:${diagnostic.line} — ${diagnostic.reason.replace(/_/g, " ")}`}
            >
              <span className="block truncate text-[12px] text-signal">
                {diagnostic.specifier}
              </span>
              <span className="block truncate text-[11px] text-muted-foreground">
                {diagnostic.path}:{diagnostic.line} ·{" "}
                {diagnostic.reason.replace(/_/g, " ")}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {diagnostics.length > shown.length && (
        <p className="mt-2 px-1.5 text-[11px] text-muted-foreground">
          {formatCount(diagnostics.length - shown.length)} more in the JSON
        </p>
      )}
    </Section>
  );
}
