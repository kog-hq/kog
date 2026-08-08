import { Moon, PanelLeftOpen, Settings2, Sun } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { DisplayControls } from "@/components/rail";
import type { EdgeMode, LabelMode } from "@/graph/graph-canvas";
import type { ColourBy } from "@/graph/build";
import type { Theme } from "@/lib/palette";

/**
 * The controls that stay reachable when the column is closed.
 *
 * Obsidian keeps a small cluster of round buttons in the corner of the graph
 * pane and nothing else, and the reason is the same one written at the top of
 * `app.tsx`: the instrument reading a map should not be bigger than the map.
 * A 268 px column is a fifth of a laptop window, and there are long stretches
 * of reading a graph where none of it is being touched.
 *
 * So the cluster is not a duplicate of the column, and it is not a menu of
 * everything either. It carries what you reach for *while* looking rather than
 * before looking: how the graph is drawn, which way round the theme is, and
 * the way back to the column. Filters, communities and languages stay behind —
 * they are lists as much as controls, they need height, and choosing one is
 * not something you do mid-glance.
 */
function Round({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick?: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="surface grid size-8 place-items-center rounded-full text-muted-foreground transition-colors hover:text-foreground"
    >
      {children}
    </button>
  );
}

export function FloatingControls({
  railOpen,
  onOpenRail,
  colourBy,
  onColourBy,
  labelMode,
  onLabelMode,
  edgeMode,
  onEdgeMode,
  theme,
  onTheme,
}: {
  railOpen: boolean;
  onOpenRail: () => void;
  colourBy: ColourBy;
  onColourBy: (value: ColourBy) => void;
  labelMode: LabelMode;
  onLabelMode: (value: LabelMode) => void;
  edgeMode: EdgeMode;
  onEdgeMode: (value: EdgeMode) => void;
  theme: Theme;
  onTheme: () => void;
}) {
  return (
    // Top right, clear of the inspector, which arrives from the same edge
    // lower down. `pointer-events-none` on the stack and `auto` on the buttons:
    // the gaps between them are canvas, and a graph you cannot drag through
    // because an invisible column is in the way is worse than no buttons.
    <div className="pointer-events-none absolute right-3 top-3 z-10 flex flex-col items-end gap-1.5">
      {!railOpen && (
        <div className="pointer-events-auto">
          <Round label="Show the panel" onClick={onOpenRail}>
            <PanelLeftOpen className="size-3.5" />
          </Round>
        </div>
      )}

      <Popover>
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label="Graph settings"
            title="Graph settings"
            className="surface pointer-events-auto grid size-8 place-items-center rounded-full text-muted-foreground transition-colors hover:text-foreground"
          >
            <Settings2 className="size-3.5" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="left"
          align="start"
          className="w-[248px] overflow-hidden p-0"
        >
          {/* The same component the column renders, not a copy of it: two
              copies of a control set are two things to keep in step, and the
              one nobody is looking at is the one that drifts. */}
          <DisplayControls
            colourBy={colourBy}
            onColourBy={onColourBy}
            labelMode={labelMode}
            onLabelMode={onLabelMode}
            edgeMode={edgeMode}
            onEdgeMode={onEdgeMode}
          />
        </PopoverContent>
      </Popover>

      <div className="pointer-events-auto">
        <Round
          label={theme === "dark" ? "Light theme" : "Dark theme"}
          onClick={onTheme}
        >
          {theme === "dark" ? (
            <Sun className="size-3.5" />
          ) : (
            <Moon className="size-3.5" />
          )}
        </Round>
      </div>
    </div>
  );
}
