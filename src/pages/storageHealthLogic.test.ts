import assert from "node:assert/strict";
import { describe, it } from "vitest";
import {
  formatMissingList,
  summarizeRepair,
  type PackageHealth,
  type RepairOutcome,
} from "./storageHealthLogic";

const pkg = (name: string, missing: string[]): PackageHealth => ({
  name,
  missing_versions: missing,
  cached_versions: ["1.0.0"],
  orphan_versions: [],
  latest: null,
  expected_latest: null,
  latest_missing: false,
  latest_stale: false,
});

const outcome = (
  name: string,
  action: RepairOutcome["action"],
  removed: string[] = [],
  message: string | null = null,
  newLatest: string | null = null,
): RepairOutcome => ({
  name,
  action,
  removed_versions: removed,
  registered_versions: [],
  previous_latest: null,
  new_latest: newLatest,
  backup_path: null,
  message,
});

describe("formatMissingList", () => {
  it("lists every missing version as name@version", () => {
    assert.equal(
      formatMissingList([pkg("balanced-match", ["4.0.3", "4.0.4"]), pkg("@s/a", ["2.0.0"])]),
      "balanced-match@4.0.3\nbalanced-match@4.0.4\n@s/a@2.0.0",
    );
  });
});

describe("summarizeRepair", () => {
  it("counts pruned versions and latest moves separately from removed packages", () => {
    const summary = summarizeRepair([
      outcome("a", "repaired", ["1.0.1", "1.0.2"], null, "1.0.0"),
      outcome("d", "repaired", [], null, "4.0.4"),
      outcome("b", "removed", ["3.0.0"]),
      outcome("c", "failed", [], "读取 package.json 失败"),
    ]);
    assert.equal(summary.repaired, 2);
    assert.equal(summary.removed, 1);
    assert.equal(summary.failed, 1);
    assert.equal(
      summary.text,
      "修复 2 个包（剔除 2 个版本，修正 2 个 latest），移除 1 个无 tarball 的包，失败 1 个（c: 读取 package.json 失败）",
    );
  });

  it("mentions registered orphan versions only when there are any", () => {
    const summary = summarizeRepair([
      { ...outcome("ansi-styles", "repaired", [], null, "4.3.0"), registered_versions: ["4.3.0"] },
    ]);
    assert.equal(summary.text, "修复 1 个包（登记 1 个孤儿版本，剔除 0 个版本，修正 1 个 latest）");
  });
});
