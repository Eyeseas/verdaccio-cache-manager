import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  checkForUpdateOnStartup,
  installUpdate,
  resetAppUpdateStateForTest,
} from "./appUpdate";

const { check, relaunch, toast } = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() },
}));

vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("sonner", () => ({ toast }));

function fakeUpdate() {
  return {
    version: "0.2.0",
    currentVersion: "0.1.45",
    downloadAndInstall: vi.fn().mockResolvedValue(undefined),
  };
}

describe("appUpdate", () => {
  beforeEach(() => {
    resetAppUpdateStateForTest();
    vi.clearAllMocks();
  });

  it("prompts with an install action when a startup check finds an update", async () => {
    const update = fakeUpdate();
    check.mockResolvedValueOnce(update);

    await checkForUpdateOnStartup();

    expect(check).toHaveBeenCalledWith({ timeout: 10_000 });
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    const [title, options] = toast.info.mock.calls[0];
    expect(title).toBe("发现新版本 v0.2.0");
    expect(options.action.label).toBe("立即更新");

    options.action.onClick();
    await vi.waitFor(() => expect(relaunch).toHaveBeenCalled());
    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
  });

  it("stays silent when no update or the check fails offline", async () => {
    check.mockResolvedValueOnce(null);
    await checkForUpdateOnStartup();
    resetAppUpdateStateForTest();
    check.mockRejectedValueOnce(new Error("network unreachable"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    await checkForUpdateOnStartup();

    expect(toast.info).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("checks only once per app session", async () => {
    check.mockResolvedValue(null);
    await checkForUpdateOnStartup();
    await checkForUpdateOnStartup();
    expect(check).toHaveBeenCalledOnce();
  });

  it("ignores a second install while one is in progress and reports failures", async () => {
    const update = fakeUpdate();
    update.downloadAndInstall.mockRejectedValueOnce(new Error("bad signature"));

    await Promise.all([installUpdate(update as never), installUpdate(update as never)]);

    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(relaunch).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith(
      "更新失败",
      expect.objectContaining({ description: "Error: bad signature" }),
    );
  });
});
