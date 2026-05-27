export type DowngradeStatus =
  | "unchanged-cached"
  | "rewritten-cached"
  | "downgraded"
  | "major-downgraded"
  | "missing-cache"
  | "unsupported-spec"
  | "invalid-range";

export type DowngradeSection =
  | "dependencies"
  | "devDependencies"
  | "optionalDependencies";

export interface DowngradeItem {
  name: string;
  section: DowngradeSection;
  original_spec: string;
  original_resolved_version: string | null;
  target_version: string | null;
  cached_versions: string[];
  status: DowngradeStatus;
  reason: string;
}

export interface DowngradeSummary {
  total: number;
  changed: number;
  rewritten_cached: number;
  unchanged_cached: number;
  missing_cache: number;
  unsupported: number;
  invalid: number;
  major_downgraded: number;
}

export interface DowngradeAnalysis {
  request_id?: string | null;
  file_path: string;
  file_name: string;
  allow_major_downgrade: boolean;
  original_content: string;
  updated_content: string;
  items: DowngradeItem[];
  summary: DowngradeSummary;
  cache_index_empty: boolean;
}

export type DowngradeFilter = "all" | "changed" | "missing";

export const createDowngradeRequestId = () =>
  `${Date.now()}-${Math.random().toString(36).slice(2)}`;

export const isPackageJsonPath = (path: string) => {
  const name = path.replace(/\\/g, "/").split("/").pop() ?? path;
  return name === "package.json";
};

export const isChangedStatus = (status: DowngradeStatus) =>
  status === "rewritten-cached" ||
  status === "downgraded" ||
  status === "major-downgraded";

export const isMissingOrSkippedStatus = (status: DowngradeStatus) =>
  status === "missing-cache" ||
  status === "unsupported-spec" ||
  status === "invalid-range";

export const filterDowngradeItems = (
  items: DowngradeItem[],
  filter: DowngradeFilter
) => {
  if (filter === "changed") {
    return items.filter((item) => isChangedStatus(item.status));
  }
  if (filter === "missing") {
    return items.filter((item) => isMissingOrSkippedStatus(item.status));
  }
  return items;
};

export const statusLabel = (status: DowngradeStatus) => {
  switch (status) {
    case "unchanged-cached":
      return "已缓存";
    case "rewritten-cached":
      return "锁定版本";
    case "downgraded":
      return "降级";
    case "major-downgraded":
      return "跨 major";
    case "missing-cache":
      return "无缓存";
    case "unsupported-spec":
      return "已跳过";
    case "invalid-range":
      return "范围无效";
  }
};

export const statusVariant = (status: DowngradeStatus) => {
  switch (status) {
    case "unchanged-cached":
      return "secondary" as const;
    case "rewritten-cached":
    case "downgraded":
      return "outline" as const;
    case "major-downgraded":
    case "missing-cache":
    case "unsupported-spec":
    case "invalid-range":
      return "destructive" as const;
  }
};

export const isStaleAnalysis = (
  currentRequestId: string | null,
  analysis: Pick<DowngradeAnalysis, "request_id">
) => Boolean(analysis.request_id && currentRequestId !== analysis.request_id);
