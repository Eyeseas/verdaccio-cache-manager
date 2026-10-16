export interface PackageHealth {
  name: string;
  missing_versions: string[];
  cached_versions: string[];
  latest: string | null;
  expected_latest: string | null;
  latest_missing: boolean;
  latest_stale: boolean;
}

export interface HealthReport {
  scanned: number;
  unhealthy: PackageHealth[];
  unreadable: string[];
  plugin_outdated: boolean;
}

export type RepairAction = "repaired" | "removed" | "skipped" | "failed";

export interface RepairOutcome {
  name: string;
  action: RepairAction;
  removed_versions: string[];
  previous_latest: string | null;
  new_latest: string | null;
  backup_path: string | null;
  message: string | null;
}

/** 生成 `name@version` 每行一条的缺失清单，便于到外网补包。 */
export function formatMissingList(packages: PackageHealth[]): string {
  return packages
    .flatMap((p) => p.missing_versions.map((v) => `${p.name}@${v}`))
    .join("\n");
}

export function summarizeRepair(outcomes: RepairOutcome[]) {
  const count = (action: RepairAction) =>
    outcomes.filter((o) => o.action === action).length;
  const repaired = count("repaired");
  const removed = count("removed");
  const skipped = count("skipped");
  const failed = count("failed");
  const fixed = outcomes.filter((o) => o.action === "repaired");
  const versions = fixed.reduce((n, o) => n + o.removed_versions.length, 0);
  const latestMoved = fixed.filter((o) => o.new_latest).length;

  const parts = [`修复 ${repaired} 个包（剔除 ${versions} 个版本，修正 ${latestMoved} 个 latest）`];
  if (removed > 0) parts.push(`移除 ${removed} 个无 tarball 的包`);
  if (skipped > 0) parts.push(`跳过 ${skipped} 个`);
  if (failed > 0) {
    const first = outcomes.find((o) => o.action === "failed");
    parts.push(`失败 ${failed} 个（${first?.name}: ${first?.message}）`);
  }
  return { repaired, removed, skipped, failed, text: parts.join("，") };
}
