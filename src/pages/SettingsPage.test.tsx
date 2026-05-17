import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "./SettingsPage";

const {
  invoke,
  open,
  save,
  check,
  relaunch,
  toast,
  loadConfig,
  saveConfig,
  testConnection,
  startSync,
  clearIndex,
  config,
} = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
  loadConfig: vi.fn(),
  saveConfig: vi.fn(),
  testConnection: vi.fn(),
  startSync: vi.fn(),
  clearIndex: vi.fn(),
  config: {
    registry_url: "http://localhost:4873",
    concurrency: 5,
    retry_count: 3,
    timeout_secs: 60,
    verdaccio_storage_path: null,
  },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.24"),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open, save }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("sonner", () => ({ toast }));

vi.mock("@/stores/configStore", () => ({
  useConfigStore: () => ({
    config,
    loadConfig,
    saveConfig,
    testConnection,
  }),
}));

vi.mock("@/stores/syncStore", () => ({
  useSyncStore: () => ({
    status: "idle",
    lastSyncAt: null,
    startSync,
    clearIndex,
  }),
}));

describe("SettingsPage Verdaccio plugin export", () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
    save.mockReset();
    check.mockReset();
    relaunch.mockReset();
    toast.success.mockReset();
    toast.error.mockReset();
    toast.info.mockReset();
    loadConfig.mockReset();
    saveConfig.mockReset();
    testConnection.mockReset();
    startSync.mockReset();
    clearIndex.mockReset();
  });

  it("opens a save dialog with the bundled plugin tarball name", async () => {
    invoke.mockResolvedValueOnce({
      name: "verdaccio-cached-list",
      version: "0.2.0",
      filename: "verdaccio-cached-list-0.2.0.tgz",
    });
    save.mockResolvedValueOnce(null);

    render(<SettingsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导出插件包" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("get_verdaccio_plugin_info");
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({
          defaultPath: "verdaccio-cached-list-0.2.0.tgz",
        })
      );
    });
  });

  it("exports the plugin to the selected path", async () => {
    invoke.mockResolvedValueOnce({
      name: "verdaccio-cached-list",
      version: "0.1.0",
      filename: "verdaccio-cached-list-0.1.0.tgz",
    });
    save.mockResolvedValueOnce("/tmp/verdaccio-cached-list-0.1.0.tgz");
    invoke.mockResolvedValueOnce("/tmp/verdaccio-cached-list-0.1.0.tgz");

    render(<SettingsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导出插件包" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("get_verdaccio_plugin_info");
      expect(invoke).toHaveBeenCalledWith("export_verdaccio_plugin", {
        outputPath: "/tmp/verdaccio-cached-list-0.1.0.tgz",
      });
    });
    expect(toast.success).toHaveBeenCalledWith(
      "插件包已导出",
      expect.objectContaining({
        description: expect.stringContaining(
          "npm install -g /tmp/verdaccio-cached-list-0.1.0.tgz"
        ),
      })
    );
  });

  it("does not invoke export when the save dialog is canceled", async () => {
    invoke.mockResolvedValueOnce({
      name: "verdaccio-cached-list",
      version: "0.1.0",
      filename: "verdaccio-cached-list-0.1.0.tgz",
    });
    save.mockResolvedValueOnce(null);

    render(<SettingsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导出插件包" }));

    await waitFor(() => {
      expect(save).toHaveBeenCalled();
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "export_verdaccio_plugin",
      expect.anything()
    );
  });

  it("shows an error toast when export fails", async () => {
    invoke.mockResolvedValueOnce({
      name: "verdaccio-cached-list",
      version: "0.1.0",
      filename: "verdaccio-cached-list-0.1.0.tgz",
    });
    save.mockResolvedValueOnce("/tmp/verdaccio-cached-list-0.1.0.tgz");
    invoke.mockRejectedValueOnce("missing resource");

    render(<SettingsPage />);
    fireEvent.click(screen.getByRole("button", { name: "导出插件包" }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("导出插件包失败", {
        description: "missing resource",
      });
    });
  });
});
