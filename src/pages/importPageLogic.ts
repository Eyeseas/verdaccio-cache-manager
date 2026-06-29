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

export interface ExportPackageInput {
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

export type PackageManager = "npm" | "pnpm" | "yarn" | "bun";

export interface InstallCommandParseResult {
  manager: PackageManager;
  command: "install" | "i" | "add";
  global: boolean;
  packages: ParsedDependency[];
  warnings: string[];
}

export const rowKey = (name: string, rawRange: string) => `${name}::${rawRange}`;

const packageManagers = new Set<PackageManager>(["npm", "pnpm", "yarn", "bun"]);
const commandsByManager: Record<PackageManager, Set<string>> = {
  npm: new Set(["install", "i"]),
  pnpm: new Set(["install", "add"]),
  yarn: new Set(["add"]),
  bun: new Set(["install", "add"]),
};

const flagsWithValues = new Set([
  "--registry",
  "--cache",
  "--prefix",
  "--tag",
  "--userconfig",
]);

const ignoredFlags = new Set([
  "-D",
  "--save-dev",
  "-P",
  "--save-prod",
  "-O",
  "--save-optional",
  "-E",
  "--save-exact",
  "--ignore-scripts",
  "--frozen-lockfile",
  "--lockfile-only",
  "--no-save",
]);

const tokenizeInstallCommandLine = (input: string) => {
  const tokens: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;

  for (let i = 0; i < input.length; i += 1) {
    const ch = input[i];
    if (quote) {
      if (ch === quote) {
        quote = null;
      } else {
        current += ch;
      }
      continue;
    }

    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }

    if (/\s/.test(ch)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
      continue;
    }

    if (ch === ";" || ch === "&" || ch === "|") {
      if (current) {
        tokens.push(current);
        current = "";
      }
      tokens.push(ch);
      continue;
    }

    current += ch;
  }

  if (current) tokens.push(current);
  return tokens;
};

const isCommandSeparator = (token: string) =>
  token === ";" || token === "&" || token === "|";

interface InstallCommandMatch {
  tokens: string[];
  manager: PackageManager;
  command: "install" | "i" | "add";
}

const findInstallCommands = (input: string): InstallCommandMatch[] => {
  const matches: InstallCommandMatch[] = [];
  for (const line of input.split(/\r?\n/)) {
    const tokens = tokenizeInstallCommandLine(line.trim());
    if (tokens.length < 2) continue;
    const maybeManager = tokens[0] as PackageManager;
    const maybeCommand = tokens[1] as "install" | "i" | "add";
    if (!packageManagers.has(maybeManager)) continue;
    if (!commandsByManager[maybeManager].has(maybeCommand)) continue;
    matches.push({ tokens, manager: maybeManager, command: maybeCommand });
  }
  return matches;
};

const isUnsupportedPackageSpec = (spec: string) =>
  spec.startsWith("npm:") ||
  spec.startsWith("git+") ||
  spec.startsWith("github:") ||
  spec.startsWith("file:") ||
  spec.startsWith("link:") ||
  spec.startsWith("./") ||
  spec.startsWith("../") ||
  /^https?:\/\//.test(spec) ||
  spec.endsWith(".tgz");

const parsePackageSpec = (spec: string): ParsedDependency | null => {
  if (!spec || isUnsupportedPackageSpec(spec)) return null;

  if (spec.startsWith("@")) {
    const slash = spec.indexOf("/");
    if (slash <= 1) return null;
    const versionSep = spec.indexOf("@", slash + 1);
    if (versionSep === -1) {
      return { name: spec, version: "latest", tarball_url: null };
    }
    return {
      name: spec.slice(0, versionSep),
      version: spec.slice(versionSep + 1) || "latest",
      tarball_url: null,
    };
  }

  const versionSep = spec.indexOf("@");
  if (versionSep === 0) return null;
  if (versionSep === -1) {
    return { name: spec, version: "latest", tarball_url: null };
  }
  return {
    name: spec.slice(0, versionSep),
    version: spec.slice(versionSep + 1) || "latest",
    tarball_url: null,
  };
};

const isOptionToken = (token: string) => token.startsWith("-");

const optionName = (token: string) => token.split("=")[0];

export const parseInstallCommand = (input: string): InstallCommandParseResult => {
  const matches = findInstallCommands(input);
  if (matches.length === 0) {
    throw new Error("未找到支持的 npm/pnpm/yarn/bun 安装命令");
  }

  const first = matches[0];
  const tokens = first.tokens;
  const warnings =
    matches.length > 1 ? ["检测到多条安装命令，当前仅解析第一条"] : [];
  const packages: ParsedDependency[] = [];
  const unsupported: string[] = [];
  let global = false;

  for (let i = 2; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (isCommandSeparator(token)) break;
    if (token === "-g" || token === "--global") {
      global = true;
      continue;
    }
    if (ignoredFlags.has(token)) continue;
    if (isOptionToken(token)) {
      const name = optionName(token);
      if (flagsWithValues.has(name) && !token.includes("=")) {
        i += 1;
      }
      continue;
    }

    const parsed = parsePackageSpec(token);
    if (parsed) {
      packages.push(parsed);
    } else {
      unsupported.push(token);
    }
  }

  if (unsupported.length > 0) {
    throw new Error(`不支持的包规格: ${unsupported.join(", ")}`);
  }
  if (packages.length === 0) {
    throw new Error("安装命令中未包含包名");
  }

  return {
    manager: first.manager,
    command: first.command,
    global,
    packages,
    warnings,
  };
};

export const installRootKeys = (packages: ParsedDependency[]) =>
  new Set(packages.map((pkg) => rowKey(pkg.name, pkg.version)));

export const isSelectableState = (status: RowStatus) =>
  status !== "cached" &&
  status !== "downloading" &&
  status !== "uploading" &&
  status !== "resolving";

export const getContextMenuActionState = (
  status: RowStatus,
  globalBusy: boolean
) => ({
  cacheDisabled: globalBusy || !isSelectableState(status),
  exportDisabled: globalBusy,
});

export const shouldShowActionBar = (state: {
  selectedSize: number;
  resolving: boolean;
  caching: boolean;
  exporting: boolean;
}) =>
  state.selectedSize > 0 || state.resolving || state.caching || state.exporting;

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

export interface MergedDependencyList {
  dependencies: ParsedDependency[];
  rootKeys: Set<string>;
}

export const mergeResolvedDependencyList = (args: {
  currentRoots: Set<string>;
  rootPackages: ResolvedImportPackage[];
  dependencies: ResolvedDependency[];
}): MergedDependencyList => {
  const output: ParsedDependency[] = [];
  const seen = new Set<string>();
  const rootKeys = new Set<string>();

  for (const root of args.rootPackages) {
    const resolvedKey = rowKey(root.name, root.version);
    if (args.currentRoots.has(rowKey(root.name, root.raw_range))) {
      rootKeys.add(resolvedKey);
    }
    if (!seen.has(resolvedKey)) {
      seen.add(resolvedKey);
      output.push({
        name: root.name,
        version: root.version,
        tarball_url: root.tarball_url,
      });
    }
  }

  for (const dep of args.dependencies) {
    const key = rowKey(dep.package_name, dep.version);
    if (seen.has(key)) continue;
    seen.add(key);
    output.push({
      name: dep.package_name,
      version: dep.version,
      tarball_url: null,
    });
  }

  return { dependencies: output, rootKeys };
};

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

export const applyResolvedPackagesForCurrentRequest = (
  states: Map<string, RowState>,
  args: {
    currentRequestId: string | null;
    responseRequestId: string;
    resolved: ResolvedImportPackage[];
  }
) => {
  if (!isCurrentResolveRequest(args.currentRequestId, args.responseRequestId)) {
    return states;
  }
  return applyResolvedPackages(states, args.resolved);
};

export const isCurrentResolveRequest = (
  currentRequestId: string | null,
  responseRequestId: string
) => currentRequestId === responseRequestId;

export const createResolveRequestId = () =>
  `import-resolve-${Date.now()}-${Math.random().toString(36).slice(2)}`;

export const exportPackagesFromResolvedSelection = (args: {
  alreadyResolved: ExportPackageInput[];
  pending: ParsedDependency[];
  resolved: ResolvedImportPackage[];
}): ExportPackageInput[] => {
  const resolvedMap = new Map<string, string>();
  for (const pkg of args.resolved) {
    resolvedMap.set(rowKey(pkg.name, pkg.raw_range), pkg.version);
  }

  const missing: string[] = [];
  const packages = [...args.alreadyResolved];
  for (const dep of args.pending) {
    const version = resolvedMap.get(rowKey(dep.name, dep.version));
    if (version) {
      packages.push({ package_name: dep.name, version });
    } else {
      missing.push(`${dep.name}@${dep.version}`);
    }
  }

  if (missing.length > 0) {
    throw new Error(`以下包无法解析到具体版本: ${missing.join(", ")}`);
  }

  return packages;
};

export const getResolvedVersionOrThrow = (
  name: string,
  rawRange: string,
  resolved: ResolvedImportPackage[]
) => {
  const version = resolved[0]?.version;
  if (!version) {
    throw new Error(`无法解析 ${name}@${rawRange} 到具体版本`);
  }
  return version;
};
