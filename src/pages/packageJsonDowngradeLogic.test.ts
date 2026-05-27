import { describe, expect, it } from "vitest";
import {
  filterDowngradeItems,
  isStaleAnalysis,
  statusLabel,
  type DowngradeAnalysis,
} from "./packageJsonDowngradeLogic";

const analysis: DowngradeAnalysis = {
  request_id: "req-2",
  file_path: "/tmp/package.json",
  file_name: "package.json",
  allow_major_downgrade: false,
  original_content: "{}",
  updated_content: "{}",
  cache_index_empty: false,
  summary: {
    total: 3,
    changed: 1,
    rewritten_cached: 1,
    unchanged_cached: 1,
    missing_cache: 1,
    unsupported: 0,
    invalid: 0,
    major_downgraded: 0,
  },
  items: [
    {
      name: "react",
      section: "dependencies",
      original_spec: "18.4.0",
      original_resolved_version: "18.4.0",
      target_version: "18.3.0",
      cached_versions: ["18.3.0"],
      status: "downgraded",
      reason: "",
    },
    {
      name: "vite",
      section: "devDependencies",
      original_spec: "^7.0.0",
      original_resolved_version: null,
      target_version: "7.0.4",
      cached_versions: ["7.0.4"],
      status: "rewritten-cached",
      reason: "",
    },
    {
      name: "missing",
      section: "optionalDependencies",
      original_spec: "^1.0.0",
      original_resolved_version: null,
      target_version: null,
      cached_versions: [],
      status: "missing-cache",
      reason: "",
    },
  ],
};

describe("packageJsonDowngradeLogic", () => {
  it("filters changed and missing items", () => {
    expect(filterDowngradeItems(analysis.items, "changed").map((i) => i.name)).toEqual([
      "react",
      "vite",
    ]);
    expect(filterDowngradeItems(analysis.items, "missing").map((i) => i.name)).toEqual([
      "missing",
    ]);
  });

  it("labels statuses for display", () => {
    expect(statusLabel("downgraded")).toBe("降级");
    expect(statusLabel("major-downgraded")).toBe("跨 major");
    expect(statusLabel("missing-cache")).toBe("无缓存");
  });

  it("detects stale analysis responses", () => {
    expect(isStaleAnalysis("req-3", analysis)).toBe(true);
    expect(isStaleAnalysis("req-2", analysis)).toBe(false);
  });
});
