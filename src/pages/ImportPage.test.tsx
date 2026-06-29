import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ImportPage } from "./ImportPage";

const {
  invoke,
  open,
  listen,
  toast,
  startCacheTasks,
  resolveDependencies,
} = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  listen: vi.fn().mockResolvedValue(vi.fn()),
  toast: {
    error: vi.fn(),
    warning: vi.fn(),
  },
  startCacheTasks: vi.fn(),
  resolveDependencies: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast }));
vi.mock("@/hooks/useTauriFileDrop", () => ({
  useTauriFileDrop: () => ({ isOver: false }),
}));
vi.mock("@/stores/taskStore", () => ({
  useTaskStore: () => ({
    startCacheTasks,
    resolveDependencies,
  }),
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
    measureElement: vi.fn(),
  }),
}));

describe("ImportPage install command mode", () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
    listen.mockClear();
    toast.error.mockReset();
    toast.warning.mockReset();
    startCacheTasks.mockReset();
    resolveDependencies.mockReset();
  });

  it("parses an install command, shows node version, and marks root packages", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_node_version") return Promise.resolve("v22.12.0");
      if (command === "check_cached_status") {
        return Promise.resolve([
          { name: "@oh-my-pi/pi-coding-agent", version: "latest", cached: false },
          { name: "react", version: "^19", cached: true },
        ]);
      }
      return Promise.resolve([]);
    });

    render(<ImportPage />);
    fireEvent.click(screen.getByRole("tab", { name: "安装命令" }));
    fireEvent.change(screen.getByLabelText("安装命令"), {
      target: {
        value: "bun install -g @oh-my-pi/pi-coding-agent react@^19",
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "解析命令" }));

    expect(await screen.findByText("@oh-my-pi/pi-coding-agent")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("check_cached_status", {
      packages: [
        ["@oh-my-pi/pi-coding-agent", "latest"],
        ["react", "^19"],
      ],
    });
    expect(screen.getByText("react")).toBeInTheDocument();
    expect(screen.getByText("Node v22.12.0")).toBeInTheDocument();
    expect(screen.getAllByText("根包")).toHaveLength(2);
  });

  it("resolves full dependencies on explicit action", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_node_version") return Promise.resolve("v22.12.0");
      if (command === "check_cached_status") {
        return Promise.resolve([
          { name: "react", version: "latest", cached: false },
          { name: "loose-envify", version: "1.4.0", cached: false },
        ]);
      }
      if (command === "resolve_package_versions") {
        return Promise.resolve([
          {
            name: "react",
            raw_range: "latest",
            version: "19.1.0",
            tarball_url: null,
            cached: false,
          },
        ]);
      }
      return Promise.resolve([]);
    });
    resolveDependencies.mockResolvedValueOnce([
      { package_name: "react", version: "19.1.0" },
      { package_name: "loose-envify", version: "1.4.0" },
    ]);

    render(<ImportPage />);
    fireEvent.click(screen.getByRole("tab", { name: "安装命令" }));
    fireEvent.change(screen.getByLabelText("安装命令"), {
      target: { value: "npm install react" },
    });
    fireEvent.click(screen.getByRole("button", { name: "解析命令" }));

    await screen.findByText("react");
    fireEvent.click(screen.getByRole("button", { name: "解析完整依赖" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        "resolve_package_versions",
        expect.objectContaining({
          packages: [
            {
              package_name: "react",
              version: "latest",
              tarball_url: undefined,
            },
          ],
        })
      );
      expect(resolveDependencies).toHaveBeenCalledWith([
        { package_name: "react", version: "19.1.0" },
      ]);
    });
    expect(await screen.findByText("loose-envify")).toBeInTheDocument();
    expect(screen.getAllByText("根包")).toHaveLength(1);
  });

  it("shows node fallback when node detection fails", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_node_version") return Promise.reject("missing node");
      if (command === "check_cached_status") {
        return Promise.resolve([
          { name: "react", version: "latest", cached: false },
        ]);
      }
      return Promise.resolve([]);
    });

    render(<ImportPage />);
    fireEvent.click(screen.getByRole("tab", { name: "安装命令" }));
    fireEvent.change(screen.getByLabelText("安装命令"), {
      target: { value: "npm install react" },
    });
    fireEvent.click(screen.getByRole("button", { name: "解析命令" }));

    expect(await screen.findByText("Node 未检测到")).toBeInTheDocument();
    expect(screen.getByText("react")).toBeInTheDocument();
  });
});
