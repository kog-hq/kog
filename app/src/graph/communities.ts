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

/**
 * The group for files with no import in either direction.
 *
 * Louvain has nothing to say about a node with no edges, so it puts each one
 * in a community of its own — which on a real repository turned 10 communities
 * into 161, of which 151 held a single file and 97 carried the *same* name.
 * A list like that is not a finding, it is the algorithm's shrug rendered as
 * a hundred checkboxes. Files that connect to nothing are one honest group.
 *
 * Negative on purpose: it is not a rank among the communities, and it takes no
 * colour slot.
 */
export const UNCONNECTED = -1;

export type Community = {
  id: number;
  /** The directory most of its files sit in, or `mixed` when none dominates. */
  name: string;
  size: number;
  /** Members, so a filter can hide the whole group at once. */
  members: string[];
};

export type Communities = {
  /** Total: every node in the project has an entry. */
  byNode: Map<string, number>;
  list: Community[];
};

/**
 * The directory a file is attributed to, cut at `depth` segments.
 *
 * Never the file's own name: a community named after one file describes
 * nothing that the file's label does not already say.
 */
function areaOf(id: string, depth: number): string {
  const directories = id.split("/").slice(0, -1);
  if (directories.length === 0) return ".";
  return directories.slice(0, Math.max(1, Math.min(depth, directories.length)))
    .join("/");
}

/** The directory most of a group's files sit in, and how much of it that is. */
function dominantArea(
  members: string[],
  depth: number,
): { area: string; share: number } {
  const areas = new Map<string, number>();
  for (const id of members) {
    const area = areaOf(id, depth);
    areas.set(area, (areas.get(area) ?? 0) + 1);
  }
  const [area, count] = [...areas.entries()].sort(
    (a, b) => b[1] - a[1] || a[0].localeCompare(b[0]),
  )[0];
  return { area, share: count / members.length };
}

/** How far into the tree a name may reach before it is more path than label. */
const MAX_NAME_DEPTH = 5;

/**
 * Name each community after where its files live, deepening the path until
 * the name is one no other community has already taken.
 *
 * Two communities both called `apps/backend` are indistinguishable in a list,
 * and the reader's only conclusion is that the tool is repeating itself. They
 * are genuinely different sets of files, so the name has to say where they
 * differ — `apps/backend/modules` and `apps/backend/common` — and only fall
 * back to a number when they really do live in the same folder.
 */
function nameCommunities(list: Community[]): void {
  const taken = new Set<string>();
  for (const community of list) {
    // A community whose files are scattered across the tree is named `mixed`
    // rather than after whichever folder happened to win by one file — that
    // is the interesting case, and mislabelling it hides it.
    let name = "mixed";
    for (let depth = 2; depth <= MAX_NAME_DEPTH; depth++) {
      const { area, share } = dominantArea(community.members, depth);
      // Below half, deepening only finds a smaller plurality. Stop and keep
      // whatever the shallower pass earned.
      if (share < 0.5) break;
      name = area;
      if (!taken.has(name)) break;
    }
    if (taken.has(name)) {
      // Parenthesised, so `apps/frontend/src (2)` cannot be misread as a
      // folder called `src 2`.
      let suffix = 2;
      while (taken.has(`${name} (${suffix})`)) suffix += 1;
      name = `${name} (${suffix})`;
    }
    taken.add(name);
    community.name = name;
  }
}

export function detectCommunities(project: KogProject): Communities {
  const degree = new Map<string, number>();
  for (const edge of project.graph.edges) {
    if (edge.source === edge.target) continue;
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
  }

  const byNode = new Map<string, number>();
  const unconnected: string[] = [];
  const connected: string[] = [];
  for (const node of project.graph.nodes) {
    if (degree.has(node.id)) connected.push(node.id);
    else unconnected.push(node.id);
  }

  const list: Community[] = [];

  if (connected.length > 0) {
    // Louvain runs on an undirected copy: a mutual dependency and a one-way
    // one are the same evidence of belonging together, and direction only
    // makes the partition less stable.
    const graph = new Graph({ type: "undirected" });
    for (const id of connected) graph.addNode(id);
    for (const edge of project.graph.edges) {
      if (edge.source === edge.target) continue;
      if (graph.hasNode(edge.source) && graph.hasNode(edge.target)) {
        graph.mergeUndirectedEdge(edge.source, edge.target);
      }
    }

    const assignment = louvain(graph, { resolution: 1 });
    const members = new Map<number, string[]>();
    for (const [node, community] of Object.entries(assignment)) {
      const group = members.get(community);
      if (group) group.push(node);
      else members.set(community, [node]);
    }

    // Ranked by size, so the colour a community gets is stable and the
    // biggest groups take the most distinct hues. The name tiebreak keeps two
    // equal-sized groups in the same order between runs.
    const groups = [...members.values()].sort(
      (a, b) => b.length - a.length || a[0].localeCompare(b[0]),
    );
    groups.forEach((group, rank) => {
      for (const id of group) byNode.set(id, rank);
      list.push({ id: rank, name: "", size: group.length, members: group });
    });
    nameCommunities(list);
  }

  if (unconnected.length > 0) {
    for (const id of unconnected) byNode.set(id, UNCONNECTED);
    // Last whatever its size: it is a shelf, not the largest community.
    list.push({
      id: UNCONNECTED,
      name: "unconnected",
      size: unconnected.length,
      members: unconnected,
    });
  }

  return { byNode, list };
}
