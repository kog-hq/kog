import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { cn } from "@/lib/utils";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import type { KogNode, ProjectIndex } from "@/lib/kog";
import { KIND_LABEL } from "@/lib/kog";

/**
 * Rank a path against a query.
 *
 * Deliberately not a fuzzy matcher: paths are typed, not guessed at, and a
 * subsequence match on 2,800 paths puts `src/components/Header.tsx` above
 * `header.ts` because it happens to contain the letters in order. Exact
 * substring, scored by where it lands — file name beats folder, start beats
 * middle — is both faster and closer to what the reader meant.
 */
function score(node: KogNode, query: string): number {
  const id = node.id.toLowerCase();
  const at = id.indexOf(query);
  if (at === -1) return -1;

  const slash = id.lastIndexOf("/");
  const name = id.slice(slash + 1);
  const inName = name.indexOf(query);

  let points = 100 - Math.min(at, 60);
  if (inName === 0) points += 120;
  else if (inName > 0) points += 60;
  if (name === query) points += 200;
  // Shorter paths first, so `api.ts` outranks `legacy/v1/api.ts`.
  points -= id.length / 20;
  if (node.kind === "asset") points -= 25;
  return points;
}

export function FindFile({
  open,
  onOpenChange,
  nodes,
  index,
  onSelect,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nodes: KogNode[];
  index: ProjectIndex;
  onSelect: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) {
      // With no query, the most-depended-upon files: the ones worth opening
      // first in a codebase you do not know.
      return [...nodes]
        .filter((node) => node.kind !== "asset")
        .sort(
          (a, b) =>
            (index.dependents.get(b.id)?.length ?? 0) -
            (index.dependents.get(a.id)?.length ?? 0),
        )
        .slice(0, 40);
    }
    return nodes
      .map((node) => ({ node, points: score(node, needle) }))
      .filter((entry) => entry.points >= 0)
      .sort((a, b) => b.points - a.points)
      .slice(0, 40)
      .map((entry) => entry.node);
  }, [query, nodes, index]);

  useEffect(() => setActive(0), [query]);

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  useEffect(() => {
    listRef.current?.children[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  function commit(id: string) {
    onSelect(id);
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="top-[18%] max-w-xl translate-y-0 gap-0 overflow-hidden p-0"
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setActive((current) => Math.min(current + 1, results.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive((current) => Math.max(current - 1, 0));
          } else if (event.key === "Enter" && results[active]) {
            event.preventDefault();
            commit(results[active].id);
          }
        }}
      >
        <DialogTitle className="sr-only">Find a file</DialogTitle>
        <div className="flex items-center gap-2 border-b border-border px-3">
          <Search className="size-3.5 shrink-0 text-muted-foreground" />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Find a file by path"
            aria-label="Find a file by path"
            className="h-10 w-full bg-transparent text-[13px] outline-none placeholder:text-muted-foreground"
          />
          <kbd className="shrink-0 rounded border border-border px-1 py-0.5 text-[10px] text-muted-foreground">
            esc
          </kbd>
        </div>

        {results.length === 0 ? (
          <p className="px-3 py-6 text-center text-[12px] text-muted-foreground">
            No file matches “{query}”. Paths are matched as written, so try a
            fragment of the real path.
          </p>
        ) : (
          <ul
            ref={listRef}
            className="scrollbar-slim max-h-[46vh] overflow-y-auto py-1"
          >
            {results.map((node, position) => (
              <li key={node.id}>
                <button
                  type="button"
                  onMouseEnter={() => setActive(position)}
                  onClick={() => commit(node.id)}
                  className={cn(
                    "flex w-full items-center gap-2 px-3 py-1.5 text-left",
                    position === active && "bg-accent",
                  )}
                >
                  <span
                    className={cn(
                      "size-1.5 shrink-0 rounded-full",
                      node.kind === "unread_source"
                        ? "bg-signal"
                        : node.kind === "asset"
                          ? "bg-muted-foreground/50"
                          : "bg-foreground/60",
                    )}
                  />
                  <span className="min-w-0 flex-1 truncate text-[12px]">
                    {node.id}
                  </span>
                  <span className="shrink-0 text-[11px] text-muted-foreground">
                    {node.kind === "source" ? node.lang : KIND_LABEL[node.kind]}
                  </span>
                  <span className="num w-9 shrink-0 text-right text-[11px] text-muted-foreground">
                    {index.dependents.get(node.id)?.length ?? 0}←
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
