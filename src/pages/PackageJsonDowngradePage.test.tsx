import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PackageJsonDowngradePage } from "./PackageJsonDowngradePage";
import type { DowngradeAnalysis } from "./packageJsonDowngradeLogic";

const { invoke, open, save, toast, startSync } = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
  startSync: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open, save }));
vi.mock("sonner", () => ({ toast }));
vi.mock("@/hooks/useTauriFileDrop", () => ({
  useTauriFileDrop: () => ({ isOver: false }),
}));
vi.mock("@/stores/syncStore", () => ({
  useSyncStore: () => ({
    status: "idle",
    startSync,
  }),
}));

const baseAnalysis = (allowMajor = false): DowngradeAnalysis => ({
  request_id: "req-test",
  file_path: "/project/package.json",
  file_name: "package.json",
  allow_major_downgrade: allowMajor,
  original_content: "{}",
  updated_content:
    '{\n  "dependencies": {\n    "react": "18.3.0",\n    "vite": "7.0.4"\n  }\n}',
  cache_index_empty: false,
  summary: {
    total: 3,
    changed: allowMajor ? 2 : 1,
    rewritten_cached: 1,
    unchanged_cached: 0,
    missing_cache: allowMajor ? 0 : 1,
    unsupported: 0,
    invalid: 0,
    major_downgraded: allowMajor ? 1 : 0,
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
      reason: "降级到同 major 已缓存版本",
    },
    {
      name: "vite",
      section: "devDependencies",
      original_spec: "^7.0.0",
      original_resolved_version: null,
      target_version: "7.0.4",
      cached_versions: ["7.0.4"],
      status: "rewritten-cached",
      reason: "改写为已缓存精确版本",
    },
    {
      name: "missing",
      section: "optionalDependencies",
      original_spec: "^1.0.0",
      original_resolved_version: null,
      target_version: allowMajor ? "0.9.0" : null,
      cached_versions: allowMajor ? ["0.9.0"] : [],
      status: allowMajor ? "major-downgraded" : "missing-cache",
      reason: allowMajor ? "跨 major 使用已缓存版本" : "没有可用缓存版本",
    },
  ],
});

function mockAnalyzeOnce(allowMajor = false) {
  invoke.mockImplementationOnce((_command, args) =>
    Promise.resolve({
      ...baseAnalysis(allowMajor),
      request_id: args.requestId,
    })
  );
}

describe("PackageJsonDowngradePage", () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
    save.mockReset();
    toast.success.mockReset();
    toast.error.mockReset();
    startSync.mockReset();
  });

  it("selects a package file and analyzes it", async () => {
    open.mockResolvedValueOnce("/project/package.json");
    mockAnalyzeOnce();

    render(<PackageJsonDowngradePage />);
    fireEvent.click(screen.getByRole("button", { name: "选择 package.json" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        "analyze_package_json_downgrade",
        expect.objectContaining({
          filePath: "/project/package.json",
          allowMajorDowngrade: false,
        })
      );
    });
    expect(await screen.findByText("react")).toBeInTheDocument();
    expect(screen.getAllByText("将修改").length).toBeGreaterThan(0);
  });

  it("reanalyzes when cross major is enabled", async () => {
    open.mockResolvedValueOnce("/project/package.json");
    mockAnalyzeOnce(false);
    mockAnalyzeOnce(true);

    render(<PackageJsonDowngradePage />);
    fireEvent.click(screen.getByRole("button", { name: "选择 package.json" }));
    await screen.findByText("react");

    fireEvent.click(screen.getByRole("checkbox", { name: "允许跨 major 降级" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenLastCalledWith(
        "analyze_package_json_downgrade",
        expect.objectContaining({
          filePath: "/project/package.json",
          allowMajorDowngrade: true,
        })
      );
    });
  });

  it("filters changed rows", async () => {
    open.mockResolvedValueOnce("/project/package.json");
    mockAnalyzeOnce();

    render(<PackageJsonDowngradePage />);
    fireEvent.click(screen.getByRole("button", { name: "选择 package.json" }));
    await screen.findByText("missing");

    fireEvent.click(screen.getByRole("button", { name: "将修改" }));

    expect(screen.getByText("react")).toBeInTheDocument();
    expect(screen.queryByText("missing")).not.toBeInTheDocument();
  });

  it("saves generated package json to a selected path", async () => {
    open.mockResolvedValueOnce("/project/package.json");
    mockAnalyzeOnce();
    save.mockResolvedValueOnce("/project/package.downgraded.json");
    invoke.mockResolvedValueOnce({ output_path: "/project/package.downgraded.json" });

    render(<PackageJsonDowngradePage />);
    fireEvent.click(screen.getByRole("button", { name: "选择 package.json" }));
    await screen.findByText("react");
    fireEvent.click(screen.getByRole("button", { name: "另存为新 package.json" }));

    await waitFor(() => {
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ defaultPath: "package.downgraded.json" })
      );
      expect(invoke).toHaveBeenCalledWith("save_downgraded_package_json", {
        outputPath: "/project/package.downgraded.json",
        content: baseAnalysis().updated_content,
      });
    });
  });

  it("requires confirmation before overwriting original package json", async () => {
    open.mockResolvedValueOnce("/project/package.json");
    mockAnalyzeOnce();
    invoke.mockResolvedValueOnce({
      file_path: "/project/package.json",
      backup_path: "/project/package.json.1.bak",
    });

    render(<PackageJsonDowngradePage />);
    fireEvent.click(screen.getByRole("button", { name: "选择 package.json" }));
    await screen.findByText("react");
    fireEvent.click(screen.getByRole("button", { name: "确认后覆盖原文件" }));

    expect(invoke).not.toHaveBeenCalledWith(
      "overwrite_package_json",
      expect.anything()
    );
    fireEvent.click(screen.getByRole("button", { name: "确认覆盖" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("overwrite_package_json", {
        filePath: "/project/package.json",
        content: baseAnalysis().updated_content,
      });
    });
  });
});
