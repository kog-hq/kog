import { cn } from "@/lib/utils";
import type { KogCoverage } from "@/lib/kog";
import { formatCount, sourceCoverage } from "@/lib/kog";

/**
 * The one thing this interface is built around.
 *
 * A resolution rate answers "of the imports I read, how many resolved?" and
 * says nothing about the files never opened. This bar shows the second
 * question as a physical proportion: read, not read, not source. The magenta
 * segment is the same colour as the nodes it stands for, so the measure and
 * the map speak one language.
 */
export function CoverageMeter({
  coverage,
  className,
  showLegend = false,
}: {
  coverage: KogCoverage;
  className?: string;
  showLegend?: boolean;
}) {
  const total = Math.max(coverage.files_seen, 1);
  const read = (coverage.files_analysed / total) * 100;
  const unread = (coverage.files_unsupported / total) * 100;
  const other = Math.max(0, 100 - read - unread);
  const rate = sourceCoverage(coverage);

  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <div
        className="flex h-1.5 w-full overflow-hidden rounded-full bg-muted"
        role="img"
        aria-label={`${formatCount(coverage.files_analysed)} files read, ${formatCount(
          coverage.files_unsupported,
        )} not read, ${formatCount(coverage.files_not_source)} not source`}
      >
        <div className="bg-foreground/70" style={{ width: `${read}%` }} />
        <div className="bg-signal" style={{ width: `${unread}%` }} />
        <div className="bg-border" style={{ width: `${other}%` }} />
      </div>
      {showLegend && (
        <div className="flex items-center justify-between text-[11px] text-muted-foreground">
          <span className="num">
            <span className="text-foreground">{formatCount(coverage.files_analysed)}</span> read
          </span>
          {coverage.files_unsupported > 0 && (
            <span className="num text-signal">
              {formatCount(coverage.files_unsupported)} not read
            </span>
          )}
          <span className="num">{(rate * 100).toFixed(1)}%</span>
        </div>
      )}
    </div>
  );
}

/** A number with its name under it, for the few figures that carry the scan. */
export function Figure({
  label,
  value,
  tone = "default",
  title,
}: {
  label: string;
  value: string;
  tone?: "default" | "signal";
  title?: string;
}) {
  return (
    <div className="flex flex-col gap-1" title={title}>
      <span
        className={cn(
          "num text-[19px] leading-none tracking-tight",
          tone === "signal" ? "text-signal" : "text-foreground",
        )}
      >
        {value}
      </span>
      <span className="eyebrow">{label}</span>
    </div>
  );
}
