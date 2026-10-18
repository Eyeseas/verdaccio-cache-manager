export type Availability = "available" | "missing_tarball" | "missing_version" | "missing_package" | "check_failed";

export interface PreflightItem {
  name: string;
  requested: string;
  version: string | null;
  status: Availability;
  message: string | null;
  latest_warning: string | null;
  optional: boolean;
}

export interface PreflightReport {
  registry: string;
  items: PreflightItem[];
  plugin_used: boolean;
}

export const availabilityLabels: Record<Availability, string> = {
  available: "可用", missing_tarball: "缺 tgz", missing_version: "缺版本",
  missing_package: "缺包", check_failed: "检查失败",
};

export const missingItems = (items: PreflightItem[]) => items.filter(
  (item) => ["missing_tarball", "missing_version", "missing_package"].includes(item.status),
);

export const formatPreflightMissing = (items: PreflightItem[]) => [...new Set(
  missingItems(items).map((item) => `${item.name}@${item.version ?? item.requested}`),
)].join("\n");

export const filterPreflightItems = (items: PreflightItem[], status: Availability | "all", query: string) =>
  items.filter((item) => (status === "all" || item.status === status) &&
    item.name.toLowerCase().includes(query.trim().toLowerCase()));
