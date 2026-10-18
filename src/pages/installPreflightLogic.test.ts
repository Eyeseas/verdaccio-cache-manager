import assert from "node:assert/strict";
import { describe, it } from "vitest";
import { filterPreflightItems, formatPreflightMissing, type PreflightItem } from "./installPreflightLogic";

const item = (name: string, status: PreflightItem["status"], version: string | null = "1.0.0"): PreflightItem => ({
  name, status, version, requested: "^1", message: null, latest_warning: null, optional: false,
});

describe("安装预检结果", () => {
  it("缺失清单去重且不把网络错误当成缺包", () => {
    assert.equal(formatPreflightMissing([
      item("a", "missing_tarball"), item("a", "missing_tarball"),
      item("b", "available"), item("c", "check_failed"), item("d", "missing_package", null),
    ]), "a@1.0.0\nd@^1");
  });
  it("按状态和包名过滤", () => {
    const items = [item("@scope/A", "available"), item("b", "missing_version")];
    assert.deepEqual(filterPreflightItems(items, "available", " SCOPE/ "), [items[0]]);
    assert.deepEqual(filterPreflightItems(items, "missing_version", ""), [items[1]]);
  });
});
