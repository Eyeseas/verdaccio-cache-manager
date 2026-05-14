export interface ParsedDependency {
  name: string;
  version: string;
  tarball_url: string | null;
}

export type RowStatus =
  | "unknown"
  | "cached"
  | "uncached"
  | "resolving"
  | "resolve-failed"
  | "downloading"
  | "uploading"
  | "failed";

export interface RowState {
  status: RowStatus;
  resolvedVersion?: string;
  error?: string;
}

export interface ResolveProgressPayload {
  request_id: string;
  name: string;
  raw_range: string;
  version: string | null;
  cached: boolean;
  error: string | null;
}

export interface ResolvedImportPackage {
  name: string;
  raw_range: string;
  version: string;
  tarball_url: string | null;
  cached: boolean;
}

export interface CacheTaskInput {
  package_name: string;
  version: string;
  tarball_url?: string;
}

export interface DependencyRootInput {
  package_name: string;
  version: string;
}

export interface ResolvedDependency {
  package_name: string;
  version: string;
}

export interface CachedStatus {
  name: string;
  version: string;
  cached: boolean;
}

export const rowKey = (name: string, rawRange: string) => `${name}::${rawRange}`;

export const isSelectableState = (status: RowStatus) =>
  status !== "cached" &&
  status !== "downloading" &&
  status !== "uploading" &&
  status !== "resolving";

export const shouldShowActionBar = (state: {
  selectedSize: number;
  resolving: boolean;
  caching: boolean;
}) => state.selectedSize > 0 || state.resolving || state.caching;

export const getRowState = (
  states: Map<string, RowState>,
  dep: ParsedDependency
): RowState => states.get(rowKey(dep.name, dep.version)) ?? { status: "unknown" };

export const applyResolveProgress = (
  states: Map<string, RowState>,
  args: {
    currentRequestId: string | null;
    payload: ResolveProgressPayload;
  }
) => {
  const { currentRequestId, payload } = args;
  if (!currentRequestId || payload.request_id !== currentRequestId) {
    return states;
  }

  const key = rowKey(payload.name, payload.raw_range);
  const next = new Map(states);
  const cur = next.get(key) ?? { status: "unknown" };
  const resolvedVersion = payload.version ?? cur.resolvedVersion;

  if (payload.error) {
    next.set(key, {
      ...cur,
      status: "resolve-failed",
      error: payload.error,
      resolvedVersion,
    });
  } else if (payload.cached) {
    next.set(key, {
      ...cur,
      status: "cached",
      error: undefined,
      resolvedVersion,
    });
  } else {
    next.set(key, {
      ...cur,
      status: "uncached",
      error: undefined,
      resolvedVersion,
    });
  }

  return next;
};

export const applyTaskProgress = (
  states: Map<string, RowState>,
  payload: {
    package_name: string;
    version: string;
    status: string;
    error: string | null;
  }
) => {
  let changed = false;
  const next = new Map(states);

  for (const [key, state] of next) {
    if (state.resolvedVersion !== payload.version) continue;
    const sep = key.lastIndexOf("::");
    const name = sep === -1 ? key : key.slice(0, sep);
    if (name !== payload.package_name) continue;

    let status: RowStatus = state.status;
    switch (payload.status) {
      case "Downloading":
        status = "downloading";
        break;
      case "Uploading":
        status = "uploading";
        break;
      case "Success":
      case "Skipped":
        status = "cached";
        break;
      case "Failed":
        status = "failed";
        break;
      default:
        break;
    }

    if (status !== state.status || state.error !== (payload.error ?? undefined)) {
      next.set(key, { ...state, status, error: payload.error ?? undefined });
      changed = true;
    }
  }

  return changed ? next : states;
};

export const pruneSelection = (
  selected: Set<number>,
  deps: ParsedDependency[],
  states: Map<string, RowState>
) => {
  const next = new Set<number>();
  for (const index of selected) {
    const dep = deps[index];
    if (!dep) continue;
    if (isSelectableState(getRowState(states, dep).status)) {
      next.add(index);
    }
  }
  return next;
};

export const areSelectionsEqual = (a: Set<number>, b: Set<number>) => {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
};

export const removeResolvedFromSelection = (
  selected: Set<number>,
  deps: ParsedDependency[],
  resolved: ResolvedImportPackage[]
) => {
  const resolvedKeys = new Set(
    resolved.map((pkg) => rowKey(pkg.name, pkg.raw_range))
  );
  const next = new Set<number>();
  for (const index of selected) {
    const dep = deps[index];
    if (!dep || !resolvedKeys.has(rowKey(dep.name, dep.version))) {
      next.add(index);
    }
  }
  return next;
};

export const cacheTaskInputsFromResolved = (
  resolved: ResolvedImportPackage[]
): CacheTaskInput[] =>
  resolved
    .filter((pkg) => !pkg.cached)
    .map((pkg) => ({
      package_name: pkg.name,
      version: pkg.version,
      tarball_url: pkg.tarball_url || undefined,
    }));

export const dependencyRootsFromResolved = (
  resolved: ResolvedImportPackage[]
): DependencyRootInput[] =>
  resolved.map((pkg) => ({
    package_name: pkg.name,
    version: pkg.version,
  }));

export const cacheTaskInputsFromDependencies = (
  resolved: ResolvedDependency[],
  cachedStatuses: CachedStatus[]
): CacheTaskInput[] => {
  const cached = new Set(
    cachedStatuses
      .filter((status) => status.cached)
      .map((status) => rowKey(status.name, status.version))
  );

  return resolved
    .filter((dep) => !cached.has(rowKey(dep.package_name, dep.version)))
    .map((dep) => ({
      package_name: dep.package_name,
      version: dep.version,
      tarball_url: undefined,
    }));
};

export const applyResolvedPackages = (
  states: Map<string, RowState>,
  resolved: ResolvedImportPackage[]
) => {
  const next = new Map(states);
  for (const pkg of resolved) {
    const key = rowKey(pkg.name, pkg.raw_range);
    const cur = next.get(key) ?? { status: "unknown" };
    next.set(key, {
      ...cur,
      status: pkg.cached ? "cached" : "uncached",
      error: undefined,
      resolvedVersion: pkg.version,
    });
  }
  return next;
};

export const isCurrentResolveRequest = (
  currentRequestId: string | null,
  responseRequestId: string
) => currentRequestId === responseRequestId;

export const createResolveRequestId = () =>
  `import-resolve-${Date.now()}-${Math.random().toString(36).slice(2)}`;
