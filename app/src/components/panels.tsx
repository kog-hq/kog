import { EyeOff } from "lucide-react";
import { CoverageMeter, Figure } from "@/components/meter";
import type { KogDiagnostic, KogProject, NodeKind } from "@/lib/kog";
import { formatCount, formatRate } from "@/lib/kog";

export type Filters = {
  /** `null` means every language; a set means only these. */
  languages: Set<string> | null;
  kinds: Set<NodeKind>;
  hideIsolated: boolean;
  onlyUnresolved: boolean;
};

export const NO_FILTERS: Filters = {
  languages: null,
  kinds: new Set<NodeKind>(["source", "unread_source", "asset"]),
  hideIsolated: false,
  onlyUnresolved: false,
};

/** How many of the four filters are doing something. */
export function activeFilterCount(filters: Filters): number {
  let count = 0;
  if (filters.languages) count += 1;
  if (filters.kinds.size < NO_FILTERS.kinds.size) count += 1;
  if (filters.hideIsolated) count += 1;
  if (filters.onlyUnresolved) count += 1;
  return count;
}

function Group({
  title,
  count,
  children,
}: {
  title: string;
  count?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="border-b border-border px-4 py-3.5 last:border-b-0">
      <header className="mb-2.5 flex items-baseline justify-between">
        <h3 className="eyebrow">{title}</h3>
        {count && (
          <span className="num text-[11px] text-muted-foreground">{count}</span>
        )}
      </header>
      {children}
    </section>
  );
}

/** What the scan measured, and what it never opened. */
export function MeasuresPanel({ project }: { project: KogProject }) {
  const { stats } = project.graph;
  const gaps = stats.coverage.extensions.filter(
    (e) => e.status === "unsupported_language",
  );

  return (
    <>
      <Group title="Measured">
        <div className="mb-3.5 flex items-start justify-between gap-3">
          <Figure
            label="resolution"
            value={formatRate(stats.resolution_rate)}
            title="Resolved over in-scope internal specifiers"
          />
          <Figure
            label="unresolved"
            value={formatCount(stats.unresolved)}
            title="Imports naming a file KOG could not find"
          />
          <Figure
            label="edges"
            value={formatCount(project.graph.edges.length)}
          />
        </div>
        <CoverageMeter coverage={stats.coverage} showLegend />
      </Group>

      {gaps.length > 0 && (
        <Group
          title="Not read"
          count={formatCount(stats.coverage.files_unsupported)}
        >
          <ul className="flex flex-col gap-0.5">
            {gaps.map((gap) => (
              <li
                key={`${gap.extension}-${gap.status}`}
                className="flex items-center gap-2 px-1.5 py-0.5"
              >
                <span className="text-[12px]">{gap.label}</span>
                <span className="flex-1 truncate text-[11px] text-muted-foreground">
                  {gap.lang ?? "unrecognised"}
                </span>
                <span className="num text-[11px] text-muted-foreground">
                  {formatCount(gap.count)}
                </span>
              </li>
            ))}
          </ul>
          <p className="mt-2.5 px-1.5 text-[11px] leading-relaxed text-muted-foreground">
            These files are in the graph, but their own imports are not: KOG has
            no extractor for them yet.
          </p>
        </Group>
      )}

      {stats.coverage.skipped_directories.length > 0 && (
        <Group title="Never entered">
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
        </Group>
      )}
    </>
  );
}

/** Every specifier that did not become an edge, and where it was written. */
export function GapsPanel({
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
  if (diagnostics.length === 0) {
    return (
      <Group title="Gaps">
        <p className="px-1.5 text-[12px] leading-relaxed text-muted-foreground">
          Every import in this project resolved to a file in the graph. Nothing
          to explain.
        </p>
      </Group>
    );
  }

  return (
    <Group
      title="Gaps"
      count={
        diagnostics.length < total
          ? `${formatCount(diagnostics.length)} of ${formatCount(total)}`
          : formatCount(total)
      }
    >
      <ul className="flex flex-col">
        {diagnostics.map((diagnostic, position) => (
          <li key={`${diagnostic.path}:${diagnostic.line}:${position}`}>
            <button
              type="button"
              onClick={() => onSelect(diagnostic.path)}
              onMouseEnter={() => onHover(diagnostic.path)}
              onMouseLeave={() => onHover(null)}
              className="row w-full rounded-md px-1.5 py-1.5 text-left"
            >
              <span className="block truncate text-[12px]">
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
    </Group>
  );
}
