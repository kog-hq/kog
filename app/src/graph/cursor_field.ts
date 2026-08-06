import type Graph from "graphology";

/**
 * Nodes that get out of the way.
 *
 * The pointer pushes the graph aside as it moves through it, and everything
 * springs back behind it. It is not decoration: a dense cluster is a wall of
 * overlapping discs, and parting it under the cursor is how you see what is
 * actually there — the same reason you move papers on a desk rather than
 * squint at the pile.
 *
 * The cost is bounded on both ends. Only nodes inside the cursor's radius are
 * ever pushed, found through a uniform grid built once from the layout, and
 * only nodes still moving are integrated: a node that has settled is removed
 * from the active set and costs nothing until it is touched again. On a
 * 2,800-node graph that is a few dozen nodes a frame, not 2,800.
 */

type Displacement = { ox: number; oy: number; vx: number; vy: number };

export type FieldOptions = {
  /** Radius of influence, in graph units. Set from the camera each frame. */
  radius: number;
  /** How far a node at the very centre is pushed, in graph units. */
  strength: number;
};

/** Below this, a node is settled and leaves the active set. */
const REST = 1e-4;

export class CursorField {
  private readonly base = new Map<string, { x: number; y: number }>();
  private readonly active = new Map<string, Displacement>();
  private readonly cells = new Map<string, string[]>();
  private readonly cell: number;
  private readonly graph: Graph;

  constructor(graph: Graph) {
    this.graph = graph;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    graph.forEachNode((node, attributes) => {
      const x = attributes.x as number;
      const y = attributes.y as number;
      this.base.set(node, { x, y });
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    });

    // Roughly forty cells across the layout: small enough that a query
    // touches few nodes, large enough that the map stays small.
    const extent = Math.max(maxX - minX, maxY - minY, 1);
    this.cell = extent / 40;

    for (const [node, point] of this.base) {
      const key = this.keyOf(point.x, point.y);
      const bucket = this.cells.get(key);
      if (bucket) bucket.push(node);
      else this.cells.set(key, [node]);
    }
  }

  private keyOf(x: number, y: number): string {
    return `${Math.floor(x / this.cell)}:${Math.floor(y / this.cell)}`;
  }

  /**
   * Advance one frame. Returns true while anything is still moving, so the
   * caller can stop its loop the moment the graph is at rest.
   */
  step(cursor: { x: number; y: number } | null, options: FieldOptions): boolean {
    if (cursor) this.push(cursor, options);
    return this.settle(options);
  }

  /** Add velocity to every node the cursor is currently near. */
  private push(cursor: { x: number; y: number }, options: FieldOptions): void {
    const { radius, strength } = options;
    const reach = Math.ceil(radius / this.cell);
    const cx = Math.floor(cursor.x / this.cell);
    const cy = Math.floor(cursor.y / this.cell);

    for (let gx = cx - reach; gx <= cx + reach; gx++) {
      for (let gy = cy - reach; gy <= cy + reach; gy++) {
        const bucket = this.cells.get(`${gx}:${gy}`);
        if (!bucket) continue;
        for (const node of bucket) {
          const point = this.base.get(node);
          if (!point) continue;
          const dx = point.x - cursor.x;
          const dy = point.y - cursor.y;
          const distance = Math.hypot(dx, dy);
          if (distance > radius) continue;

          // Falls off with the square of the distance, so the effect is a
          // soft dome rather than a hard bubble with an edge.
          const falloff = (1 - distance / radius) ** 2;
          // A node exactly under the cursor has no direction to flee in;
          // give it one rather than dividing by zero.
          const angle = distance < 1e-6 ? Math.random() * Math.PI * 2 : Math.atan2(dy, dx);
          const force = strength * falloff;

          const current = this.active.get(node) ?? { ox: 0, oy: 0, vx: 0, vy: 0 };
          current.vx += Math.cos(angle) * force;
          current.vy += Math.sin(angle) * force;
          this.active.set(node, current);
        }
      }
    }
  }

  /** Integrate the spring, and write the result onto the graph. */
  private settle(options: FieldOptions): boolean {
    if (this.active.size === 0) return false;

    // A critically-damped-ish spring: it returns without wobbling, which is
    // what makes the motion read as weight rather than as jelly.
    const stiffness = 0.14;
    const damping = 0.78;
    const limit = options.strength * 6;

    for (const [node, displacement] of this.active) {
      displacement.vx -= displacement.ox * stiffness;
      displacement.vy -= displacement.oy * stiffness;
      displacement.vx *= damping;
      displacement.vy *= damping;
      displacement.ox += displacement.vx;
      displacement.oy += displacement.vy;

      // Clamp, so holding the pointer still on a node cannot fling it off
      // the canvas.
      const reach = Math.hypot(displacement.ox, displacement.oy);
      if (reach > limit) {
        displacement.ox *= limit / reach;
        displacement.oy *= limit / reach;
      }

      const point = this.base.get(node);
      if (!point) {
        this.active.delete(node);
        continue;
      }

      const rest = options.strength * REST;
      if (reach < rest && Math.hypot(displacement.vx, displacement.vy) < rest) {
        this.graph.setNodeAttribute(node, "x", point.x);
        this.graph.setNodeAttribute(node, "y", point.y);
        this.active.delete(node);
        continue;
      }

      this.graph.setNodeAttribute(node, "x", point.x + displacement.ox);
      this.graph.setNodeAttribute(node, "y", point.y + displacement.oy);
    }

    return this.active.size > 0;
  }

  /** Put every node back where the layout left it. */
  reset(): void {
    for (const node of this.active.keys()) {
      const point = this.base.get(node);
      if (!point) continue;
      this.graph.setNodeAttribute(node, "x", point.x);
      this.graph.setNodeAttribute(node, "y", point.y);
    }
    this.active.clear();
  }
}
