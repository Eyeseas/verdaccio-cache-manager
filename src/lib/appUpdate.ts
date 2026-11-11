import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { toast } from "sonner";

const STARTUP_CHECK_TIMEOUT_MS = 10_000;
const UPDATE_TOAST_ID = "update-progress";

let startupChecked = false;
let installing = false;

/** 下载安装并重启；重复触发时忽略，避免启动提示与设置页同时安装。 */
export async function installUpdate(update: Update) {
  if (installing) return;
  installing = true;
  try {
    toast.info(`发现新版本 v${update.version}`, {
      description: "正在下载更新...",
      duration: Infinity,
      id: UPDATE_TOAST_ID,
    });
    await update.downloadAndInstall();
    toast.success("更新已下载，即将重启应用", { id: UPDATE_TOAST_ID });
    await relaunch();
  } catch (e) {
    toast.error("更新失败", { description: String(e), id: UPDATE_TOAST_ID });
  } finally {
    installing = false;
  }
}

/**
 * 启动后静默检查一次更新。离线/内网环境访问不到 GitHub 是常态，失败不打扰用户；
 * 发现新版本时只提示，由用户决定是否立即下载重启。
 */
export async function checkForUpdateOnStartup() {
  if (startupChecked) return;
  startupChecked = true;
  let update: Update | null;
  try {
    update = await check({ timeout: STARTUP_CHECK_TIMEOUT_MS });
  } catch (e) {
    console.warn("启动检查更新失败", e);
    return;
  }
  if (!update) return;
  toast.info(`发现新版本 v${update.version}`, {
    description: `当前版本 v${update.currentVersion}，可立即下载并重启应用`,
    duration: Infinity,
    id: UPDATE_TOAST_ID,
    action: { label: "立即更新", onClick: () => void installUpdate(update) },
    cancel: { label: "稍后", onClick: () => {} },
  });
}

/** 仅供测试重置模块状态。 */
export function resetAppUpdateStateForTest() {
  startupChecked = false;
  installing = false;
}
