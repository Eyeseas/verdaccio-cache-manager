import assert from "node:assert/strict";
import { describe, it } from "vitest";
import { allowDependencyExpansion } from "./importPageLogic";
import { createReplenishmentList, replenishmentFromPreflight, summarizeVerification } from "./replenishmentLogic";
import type { PreflightItem } from "./installPreflightLogic";

describe("补包闭环", () => {
  it("精确闭包不再展开，并按包版本去重", () => {
    const entry = { name: "@s/a", version: "1.2.3", reason: "missing_tarball" };
    const list = createReplenishmentList([entry, entry], "http://intranet", "2026-10-19T00:00:00Z");
    assert.equal(list.entries.length, 1);
    assert.equal(list.schema_version, 1);
    assert.equal(allowDependencyExpansion("file", "replenishment"), false);
  });
  it("无法确定版本的范围明确计数，不写成精确清单", () => {
    const base: PreflightItem = { name: "a", requested: "^1", version: null, status: "missing_package", message: null, latest_warning: null, optional: false };
    const result = replenishmentFromPreflight([base, { ...base, name: "b", version: "1.2.3", status: "missing_tarball" }], "http://intranet");
    assert.equal(result.unresolved, 1);
    assert.equal(result.list.entries[0].name, "b");
    assert.equal(summarizeVerification([base]), "补包核对：0/1 项可下载，1 项仍缺失或检查失败");
  });
});
