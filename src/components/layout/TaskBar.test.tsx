import { render, screen, fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TaskBar } from "./TaskBar";

const {
  fetchTasks,
  startListening,
  retryFailed,
  clearCompleted,
  exportBatchReport,
} = vi.hoisted(() => ({
  fetchTasks: vi.fn(),
  startListening: vi.fn(),
  retryFailed: vi.fn(),
  clearCompleted: vi.fn(),
  exportBatchReport: vi.fn(),
}));

vi.mock("@/stores/taskStore", () => ({
  useTaskStore: () => ({
    tasks: [
      {
        id: "task-1",
        batch_id: "batch-123456789",
        package_name: "left-pad",
        version: "1.0.0",
        tarball_url: null,
        status: "Failed",
        error: "413 Payload Too Large",
        error_code: "PAYLOAD_TOO_LARGE",
        attempt_count: 3,
        started_at: "1",
        finished_at: "2",
        duration_ms: 1200,
      },
    ],
    currentBatchId: "batch-123456789",
    fetchTasks,
    startListening,
    retryFailed,
    clearCompleted,
    exportBatchReport,
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count, estimateSize }: { count: number; estimateSize: () => number }) => ({
    getTotalSize: () => count * estimateSize(),
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        key: index,
        index,
        size: estimateSize(),
        start: index * estimateSize(),
      })),
  }),
}));

describe("TaskBar batch details", () => {
  beforeEach(() => {
    fetchTasks.mockReset();
    startListening.mockReset();
    retryFailed.mockReset();
    clearCompleted.mockReset();
    exportBatchReport.mockReset();
  });

  it("shows the current batch id and failed error code", () => {
    render(<TaskBar />);

    expect(screen.getByText("batch-123456789")).toBeInTheDocument();
    fireEvent.click(screen.getByText(/任务:/));

    expect(screen.getByText("PAYLOAD_TOO_LARGE")).toBeInTheDocument();
    expect(screen.getByText("413 Payload Too Large")).toBeInTheDocument();
  });
});
