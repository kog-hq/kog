import Graph from "graphology";
import louvain from "graphology-communities-louvain";
import type { KogProject } from "@/lib/kog";

/**
 * The parts of a codebase that talk mostly to each other.
 *
 * A community is not a folder and not a language: it is a set of files whose
 * imports point at each other far more than at the rest of the graph. Louvain
 * finds them by maximising modularity — the gap between how many edges stay
 * inside a group and how many you would expect if the same edges had been
 * wired at random.
 *
 * They matter here for two reasons, and only the first is visible:
 *
 * 1. **The layout.** Nodes seeded near their own community, then relaxed,
 *    separate into distinct clusters. Without it a 3,000-edge graph settles
 *    into one disc and no amount of colour rescues it — that was the actual
 *    cause of "the links are messy", not the lines.
 * 2. **The reading.** A community that does not match any folder is worth
 *    knowing about: it means the code is organised one way and depends
 *    another way.
 *
 * A community is named after the directory most of its files live in, which
 * is honest and free. It is a description of where the files are, never a
 * claim about what they do.
 */

export type Community = {
  id: number;
  /** The directory most of its files sit in, or `mixed` when none dominates. */
  name: string;
  size: number;
  /** Members, so a filter can hide the whole group at once. */
  members: string[];
};

export type Communities = {
  byNode: Map<string, number>;
  list: Community[];
};

/** The folder a file is attributed to when naming a community. */
function areaOf(id: string): string {
  const segments = id.split("/");
  if (segments.length <= 1) return ".";
  return segments.slice(0, Math.min(2, segments.length - 1)).join("/");
}

export function detectCommunities(project: KogProject): Communities {
  // Louvain runs on an undirected copy: a mutual dependency and a one-way one
  // are the same evidence of belonging together, and direction only makes the
  // partition less stable.
  const graph = new Graph({ type: "undirected" });
  for (const node of project.graph.nodes) graph.addNode(node.id);
  for (const edge of project.graph.edges) {
    if (edge.source === edge.target) continue;
    if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
      graph.mergeUndirectedEdge(edge.source, edge.target);
    }
  }

  const byNode = new Map<string, number>();
  if (graph.size === 0) {
    // No edges at all: every file is its own island, and calling that 863
    // communities would be a fiction. One group, honestly named.
    for (const node of project.graph.nodes) byNode.set(node.id, 0);
    return {
      byNode,
      list: [
        {
          id: 0,
          name: "unconnected",
          size: project.graph.nodes.length,
          members: project.graph.nodes.map((node) => node.id),
        },
      ],
    };
  }

  const assignment = louvain(graph, { resolution: 1 });
  const members = new Map<number, string[]>();
  for (const [node, community] of Object.entries(assignment)) {
    byNode.set(node, community);
    const group = members.get(community);
    if (group) group.push(node);
    else members.set(community, [node]);
  }

  const list: Community[] = [...members.entries()]
    .map(([id, group]) => {
      const areas = new Map<string, number>();
      for (const node of group) {
        const area = areaOf(node);
        areas.set(area, (areas.get(area) ?? 0) + 1);
      }
      const [area, count] = [...areas.entries()].sort((a, b) => b[1] - a[1])[0];
      return {
        id,
        // A community whose files are scattered across the tree is named
        // `mixed` rather than after whichever folder happened to win by one
        // file — that is the interesting case, and mislabelling it hides it.
        name: count / group.length >= 0.5 ? area : "mixed",
        size: group.length,
        members: group,
      };
    })
    .sort((a, b) => b.size - a.size || a.name.localeCompare(b.name));

  // Renumber by size so the colour a community gets is stable and the biggest
  // groups take the most distinct hues.
  const renumbered = new Map<number, number>();
  list.forEach((community, position) => renumbered.set(community.id, position));
  for (const [node, community] of byNode) {
    byNode.set(node, renumbered.get(community) ?? 0);
  }
  return {
    byNode,
    list: list.map((community, position) => ({ ...community, id: position })),
  };
}
