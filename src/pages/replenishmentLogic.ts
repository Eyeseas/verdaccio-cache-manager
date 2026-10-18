import type { ParsedDependency, ImportFileKind } from "./importPageLogic";
import type { PreflightItem } from "./installPreflightLogic";
import { missingItems } from "./installPreflightLogic";
import type { PackageHealth } from "./storageHealthLogic";

export interface ReplenishmentEntry { name: string; version: string; reason: string }
export interface ReplenishmentList {
  kind: "verdaccio-replenishment";
  schema_version: 1;
  generated_at: string;
  source_registry: string;
  entries: ReplenishmentEntry[];
}
export interface DependencyFile {
  kind: ImportFileKind;
  dependencies: ParsedDependency[];
  replenishment: ReplenishmentList | null;
}

export function createReplenishmentList(entries: ReplenishmentEntry[], registry: string, generatedAt = new Date().toISOString()): ReplenishmentList {
  const unique = new Map(entries.map((entry) => [`${entry.name}@${entry.version}`, entry]));
  return { kind: "verdaccio-replenishment", schema_version: 1, generated_at: generatedAt,
    source_registry: registry, entries: [...unique.values()] };
}

export function replenishmentFromPreflight(items: PreflightItem[], registry: string) {
  const missing = missingItems(items);
  const exact = missing.filter((item) => item.version !== null);
  return { list: createReplenishmentList(exact.map((item) => ({
    name: item.name, version: item.version!, reason: item.status,
  })), registry), unresolved: missing.length - exact.length };
}

export function replenishmentFromHealth(packages: PackageHealth[], registry: string) {
  return createReplenishmentList(packages.flatMap((pkg) => pkg.missing_versions.map((version) => ({
    name: pkg.name, version, reason: "missing_tarball",
  }))), registry);
}

export function summarizeVerification(items: PreflightItem[]) {
  const available = items.filter((item) => item.status === "available").length;
  return `补包核对：${available}/${items.length} 项可下载，${items.length - available} 项仍缺失或检查失败`;
}
