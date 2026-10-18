import assert from "node:assert/strict";
import { describe, it } from "vitest";
import { summarizeLatest } from "./offlineBundleLogic";

describe("离线包 latest 差异", () => {
  it("把未回退和失败都计入差异", () => {
    assert.equal(summarizeLatest([
      { name: "a", expected_latest: "1.0.0", actual_latest: "1.0.0", action: "promoted", message: null },
      { name: "b", expected_latest: "1.0.0", actual_latest: "2.0.0", action: "kept", message: null },
      { name: "c", expected_latest: "1.0.0", actual_latest: null, action: "failed", message: "超时" },
    ]), "latest：推进 1 个，2 个与期望不同或未确认");
  });
});
