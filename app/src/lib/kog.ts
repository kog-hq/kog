/**
 * The shape KOG's CLI serves at `/graph.json`, and the indexes the interface
 * needs on top of it.
 *
 * Everything derived here is derived once per project: the graph is static
 * for the life of a scan, so a filter or a selection must never cost a
 * traversal of 2,800 nodes.
 */

export type NodeKind = "source" | "unread_source" | "asset";

export type KogNode = {
  id: string;
  path: string;
  lang: string;
  kind: NodeKind;
  loc: number;
  bytes: number;
  external_deps: string[];
};

export type KogEdge = { source: string; target: string; kind: string };

export type KogFailure = { path: string; reason: string };

export type DiagnosticKind = "unresolved" | "excluded";

export type KogDiagnostic = {
  path: string;
  line: number;
  specifier: string;
  kind: DiagnosticKind;
  reason: string;
  lang: string;
};

export type KogLangStats = {
  files: number;
  specifiers_total: number;
  specifiers_internal: number;
  resolved: number;
  unresolved: number;
  excluded: number;
  external_specifiers: number;
  resolution_rate: number;
  edges: number;
};

export type FileStatus =
  "analysed" | "unsupported_language" | "not_source" | "failed";

export type KogExtensionCoverage = {
  extension: string;
  label: string;
  count: number;
  status: FileStatus;
  lang: string | null;
  note: string | null;
};

export type KogCoverage = {
  files_seen: number;
  files_analysed: number;
  files_unsupported: number;
  files_not_source: number;
  extensions: KogExtensionCoverage[];
  skipped_directories: { name: string; count: number; rule: string }[];
};

export type KogStats = {
  files_discovered: number;
  files_parsed: number;
  specifiers_total: number;
  specifiers_internal: number;
  resolved: number;
  unresolved: number;
  excluded: number;
  resolution_rate: number;
  external_specifiers: number;
  external_packages_distinct: number;
  failures: KogFailure[];
  diagnostics: KogDiagnostic[];
  by_lang: Record<string, KogLangStats>;
  coverage: KogCoverage;
};

export type KogGraph = { nodes: KogNode[]; edges: KogEdge[]; stats: KogStats };

export type KogProject = {
  id: string;
  name: string;
  path: string;
  kinds: string[];
  graph: KogGraph;
};

export type KogWorkspace = {
  root: string;
  split: boolean;
  projects: KogProject[];
  totals: {
    projects: number;
    nodes: number;
    edges: number;
    files_seen: number;
    files_analysed: number;
    files_unsupported: number;
    resolution_rate: number;
    source_coverage: number;
  };
  unassigned_files: number;
};

/** One language, with the numbers the sidebar shows for it. */
export type LanguageRow = {
  lang: string;
  files: number;
  rate: number;
  edges: number;
  /** True for a language KOG cannot read: it has files but no extractor. */
  unread: boolean;
};

export type ProjectIndex = {
  byId: Map<string, KogNode>;
  /** Files this file imports. */
  dependencies: Map<string, string[]>;
  /** Files that import this file. */
  dependents: Map<string, string[]>;
  /** Short label, made unique: `client.ts` becomes `api/client.ts` when a
   *  second `client.ts` exists elsewhere in the project. */
  label: Map<string, string>;
  degree: Map<string, number>;
  diagnosticsByFile: Map<string, KogDiagnostic[]>;
  languages: LanguageRow[];
  kinds: { kind: NodeKind; count: number }[];
};

function parentOf(id: string): string {
  const cut = id.lastIndexOf("/");
  return cut === -1 ? "" : id.slice(0, cut);
}

export function baseName(id: string): string {
  const cut = id.lastIndexOf("/");
  return cut === -1 ? id : id.slice(cut + 1);
}

export function dirName(id: string): string {
  const parent = parentOf(id);
  return parent === "" ? "." : parent;
}

/**
 * A label that identifies the file rather than just naming it.
 *
 * A Go service can hold nine files called `client.go`; nine identical labels
 * on a graph are worse than none, because they look like the same file drawn
 * nine times. Each ambiguous name grows leftwards, one path segment at a
 * time, until it is unique.
 */
function uniqueLabels(nodes: KogNode[]): Map<string, string> {
  const label = new Map<string, string>();
  const groups = new Map<string, KogNode[]>();
  for (const node of nodes) {
    const name = baseName(node.id);
    const group = groups.get(name);
    if (group) group.push(node);
    else groups.set(name, [node]);
  }

  for (const [name, group] of groups) {
    if (group.length === 1) {
      label.set(group[0].id, name);
      continue;
    }
    for (const node of group) {
      const segments = node.id.split("/");
      // Grow the label leftwards until it is unique within the group, and
      // never past the full path.
      let depth = 2;
      let candidate = segments.slice(-depth).join("/");
      while (depth < segments.length) {
        const collides = group.some(
          (other) => other !== node && other.id.endsWith(`/${candidate}`),
        );
        if (!collides) break;
        depth += 1;
        candidate = segments.slice(-depth).join("/");
      }
      label.set(node.id, candidate);
    }
  }
  return label;
}

export function indexProject(project: KogProject): ProjectIndex {
  const { nodes, edges, stats } = project.graph;

  const byId = new Map(nodes.map((node) => [node.id, node]));
  const dependencies = new Map<string, string[]>();
  const dependents = new Map<string, string[]>();
  const degree = new Map<string, number>();

  for (const edge of edges) {
    const out = dependencies.get(edge.source);
    if (out) out.push(edge.target);
    else dependencies.set(edge.source, [edge.target]);

    const incoming = dependents.get(edge.target);
    if (incoming) incoming.push(edge.source);
    else dependents.set(edge.target, [edge.source]);

    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
    degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
  }

  const diagnosticsByFile = new Map<string, KogDiagnostic[]>();
  for (const diagnostic of stats.diagnostics) {
    const list = diagnosticsByFile.get(diagnostic.path);
    if (list) list.push(diagnostic);
    else diagnosticsByFile.set(diagnostic.path, [diagnostic]);
  }

  // Languages KOG read, with their own rate…
  const languages: LanguageRow[] = Object.entries(stats.by_lang).map(
    ([lang, s]) => ({
      lang,
      files: s.files,
      rate: s.resolution_rate,
      edges: s.edges,
      unread: false,
    }),
  );
  // …and the ones it could not, which belong in the same list or the list is
  // a claim about coverage that quietly omits the gap.
  const unreadByLang = new Map<string, number>();
  for (const node of nodes) {
    if (node.kind !== "unread_source") continue;
    unreadByLang.set(node.lang, (unreadByLang.get(node.lang) ?? 0) + 1);
  }
  for (const [lang, files] of unreadByLang) {
    languages.push({ lang, files, rate: 0, edges: 0, unread: true });
  }
  languages.sort((a, b) => b.files - a.files || a.lang.localeCompare(b.lang));

  const kindCounts = new Map<NodeKind, number>();
  for (const node of nodes) {
    kindCounts.set(node.kind, (kindCounts.get(node.kind) ?? 0) + 1);
  }
  const kinds = (["source", "unread_source", "asset"] as NodeKind[])
    .filter((kind) => kindCounts.has(kind))
    .map((kind) => ({ kind, count: kindCounts.get(kind) ?? 0 }));

  return {
    byId,
    dependencies,
    dependents,
    label: uniqueLabels(nodes),
    degree,
    diagnosticsByFile,
    languages,
    kinds,
  };
}

/** Analysed over source files: the number the resolution rate cannot tell you. */
export function sourceCoverage(coverage: KogCoverage): number {
  const source = coverage.files_analysed + coverage.files_unsupported;
  return source === 0 ? 1 : coverage.files_analysed / source;
}

export function formatRate(rate: number): string {
  return rate.toFixed(4);
}

export function formatCount(count: number): string {
  return count.toLocaleString("en-US");
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export const KIND_LABEL: Record<NodeKind, string> = {
  source: "source",
  unread_source: "not read",
  asset: "asset",
};
