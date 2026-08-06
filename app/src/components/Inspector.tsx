import { ArrowDownLeft, ArrowUpRight, Package, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import type { KogNode, ProjectIndex } from "@/lib/kog";
import { KIND_LABEL, formatBytes, formatCount } from "@/lib/kog";

function Block({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: React.ReactNode;
}) {
  return (
    <section className="border-t border-border px-3 py-3">
      <header className="mb-2 flex items-baseline justify-between">
        <h3 className="eyebrow">{title}</h3>
        {count !== undefined && (
          <span className="num text-[11px] text-muted-foreground">{formatCount(count)}</span>
        )}
      </header>
      {children}
    </section>
  );
}

function FileList({
  ids,
  index,
  onSelect,
  icon,
}: {
  ids: string[];
  index: ProjectIndex;
  onSelect: (id: string) => void;
  icon: React.ReactNode;
}) {
  if (ids.length === 0) {
    return <p className="px-1.5 text-[11px] text-muted-foreground">None.</p>;
  }
  return (
    <ul className="flex flex-col">
      {ids.map((id) => (
        <li key={id}>
          <button
            type="button"
            onClick={() => onSelect(id)}
            title={id}
            className="row flex w-full items-center gap-1.5 rounded-md px-1.5 py-1 text-left"
          >
            <span className="text-muted-foreground">{icon}</span>
            <span className="truncate text-[12px]">{index.label.get(id) ?? id}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

export function Inspector({
  node,
  index,
  onSelect,
  onClose,
}: {
  node: KogNode;
  index: ProjectIndex;
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const dependents = index.dependents.get(node.id) ?? [];
  const dependencies = index.dependencies.get(node.id) ?? [];
  const diagnostics = index.diagnosticsByFile.get(node.id) ?? [];

  return (
    <aside className="scrollbar-slim flex h-full w-[300px] shrink-0 flex-col overflow-y-auto border-l border-border bg-card">
      <header className="flex items-start gap-2 px-3 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="break-all text-[13px] leading-snug text-foreground">{node.id}</h2>
          <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
            <span
              className={cn(
                "rounded border border-border px-1.5 py-0.5",
                node.kind === "unread_source" && "border-signal/40 text-signal",
              )}
            >
              {node.lang}
            </span>
            <span className="rounded border border-border px-1.5 py-0.5">
              {KIND_LABEL[node.kind]}
            </span>
          </div>
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={onClose}
          aria-label="Close inspector"
          className="size-6 shrink-0"
        >
          <X className="size-3.5" />
        </Button>
      </header>

      <div className="grid grid-cols-3 gap-2 border-t border-border px-3 py-2.5 text-[11px]">
        <div>
          <div className="num text-[13px] text-foreground">{formatCount(dependents.length)}</div>
          <div className="eyebrow mt-1">used by</div>
        </div>
        <div>
          <div className="num text-[13px] text-foreground">
            {formatCount(dependencies.length)}
          </div>
          <div className="eyebrow mt-1">uses</div>
        </div>
        <div>
          <div className="num text-[13px] text-foreground">
            {node.loc > 0 ? formatCount(node.loc) : formatBytes(node.bytes)}
          </div>
          <div className="eyebrow mt-1">{node.loc > 0 ? "lines" : "size"}</div>
        </div>
      </div>

      {node.kind === "unread_source" && (
        <p className="border-t border-border bg-signal-soft px-3 py-2.5 text-[11px] leading-relaxed text-signal">
          KOG has no extractor for {node.lang} yet. This file is in the graph and other files
          can point at it, but its own imports are missing from the map.
        </p>
      )}

      {diagnostics.length > 0 && (
        <Block title="Gaps in this file" count={diagnostics.length}>
          <ul className="flex flex-col gap-1.5">
            {diagnostics.map((diagnostic, position) => (
              <li key={`${diagnostic.line}-${position}`} className="px-1.5">
                <div className="truncate text-[12px] text-signal">{diagnostic.specifier}</div>
                <div className="text-[11px] text-muted-foreground">
                  line {diagnostic.line} · {diagnostic.reason.replace(/_/g, " ")}
                </div>
              </li>
            ))}
          </ul>
        </Block>
      )}

      <Block title="Used by" count={dependents.length}>
        <FileList
          ids={dependents}
          index={index}
          onSelect={onSelect}
          icon={<ArrowDownLeft className="size-3" />}
        />
      </Block>

      <Block title="Uses" count={dependencies.length}>
        <FileList
          ids={dependencies}
          index={index}
          onSelect={onSelect}
          icon={<ArrowUpRight className="size-3" />}
        />
      </Block>

      {node.external_deps.length > 0 && (
        <Block title="Packages" count={node.external_deps.length}>
          <ul className="flex flex-col">
            {node.external_deps.map((dep) => (
              <li
                key={dep}
                className="flex items-center gap-1.5 px-1.5 py-0.5 text-[12px] text-muted-foreground"
              >
                <Package className="size-3 shrink-0" />
                <span className="truncate" title={dep}>
                  {dep}
                </span>
              </li>
            ))}
          </ul>
        </Block>
      )}
    </aside>
  );
}
