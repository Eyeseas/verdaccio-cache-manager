import { describe, it, expect, vi, beforeEach } from "vitest";
import { useTaskStore } from "./taskStore";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

describe("taskStore batch actions", () => {
  beforeEach(() => {
    invoke.mockReset();
    useTaskStore.setState({
      tasks: [],
      listening: false,
      currentBatchId: null,
      currentBatch: null,
      batches: [],
      selectedBatchItems: [],
    });
  });

  it("stores the current batch id returned by start_cache_tasks", async () => {
    invoke.mockResolvedValueOnce("batch-123");
    invoke.mockResolvedValueOnce([]);

    await useTaskStore.getState().startCacheTasks([
      { package_name: "left-pad", version: "1.0.0" },
    ]);

    expect(invoke).toHaveBeenCalledWith("start_cache_tasks", {
      packages: [{ package_name: "left-pad", version: "1.0.0" }],
    });
    expect(useTaskStore.getState().currentBatchId).toBe("batch-123");
  });

  it("exports a task batch report through Tauri", async () => {
    invoke.mockResolvedValueOnce("D:/reports/batch.md");

    const output = await useTaskStore
      .getState()
      .exportBatchReport("batch-123", "D:/reports/batch.md", "markdown");

    expect(output).toBe("D:/reports/batch.md");
    expect(invoke).toHaveBeenCalledWith("export_task_batch_report", {
      batchId: "batch-123",
      outputPath: "D:/reports/batch.md",
      format: "markdown",
    });
  });
});
