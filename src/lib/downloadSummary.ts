import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ReplenishmentList } from "@/pages/replenishmentLogic";

export interface DownloadFailure {
  package: string;
  reason: string;
}

export interface DownloadSummary {
  success: number;
  failed: number;
  failures: DownloadFailure[];
  /** 用户中途取消；已完成的包仍保留在目标目录。 */
  cancelled: boolean;
}

function formatFailures(failures: DownloadFailure[]): string {
  const head = failures
    .slice(0, 3)
    .map((f) => `${f.package} (${f.reason})`)
    .join("；");
  return failures.length > 3
    ? `${head} 等 ${failures.length} 个`
    : head;
}

/** 根据下载汇总弹出全部成功 / 部分失败 / 全部失败三态提醒。 */
export function toastDownloadSummary(summary: DownloadSummary) {
  if (summary.cancelled) {
    const failedPart = summary.failed > 0 ? `，失败 ${summary.failed} 个` : "";
    toast.info("已取消导出", {
      description: `已下载 ${summary.success} 个 tarball${failedPart}，其余未下载`,
    });
    return;
  }

  if (summary.failed === 0) {
    toast.success("导出完成", {
      description: `已下载 ${summary.success} 个 tarball 到目标目录`,
    });
    return;
  }

  if (summary.success === 0) {
    toast.error("导出失败", {
      description: `${summary.failed} 个包全部下载失败：${formatFailures(summary.failures)}`,
    });
    return;
  }

  toast.warning("导出部分完成", {
    description: `成功 ${summary.success} 个，失败 ${summary.failed} 个：${formatFailures(summary.failures)}`,
  });
}

interface DownloadProgressEvent {
  completed: number;
  total: number;
  current: string;
}

export async function downloadWithProgress(
  packages: { package_name: string; version: string }[],
  outputDir: string,
  replenishment?: ReplenishmentList | null,
): Promise<DownloadSummary> {
  const downloadId = crypto.randomUUID();
  let cancelling = false;
  const cancelAction = {
    label: "取消",
    onClick: (event: React.MouseEvent<HTMLButtonElement>) => {
      // 保留提示框，等后端真正停止后再给出汇总。
      event.preventDefault();
      if (cancelling) return;
      cancelling = true;
      toast.loading("正在取消...", { id: toastId, action: undefined });
      invoke("cancel_tarball_download", { downloadId }).catch(() => {});
    },
  };
  const toastId = toast.loading("准备下载...", {
    duration: Infinity,
    action: cancelAction,
  });
  const unlisten = await listen<DownloadProgressEvent>(
    "download-tarball-progress",
    (event) => {
      if (cancelling) return;
      const { completed, total, current } = event.payload;
      toast.loading(`下载中 ${completed}/${total}`, {
        id: toastId,
        description: current,
        action: cancelAction,
      });
    },
  );
  try {
    const summary = await invoke<DownloadSummary>("download_tarballs", {
      downloadId,
      packages,
      outputDir,
      replenishment: replenishment ?? null,
    });
    toast.dismiss(toastId);
    toastDownloadSummary(summary);
    return summary;
  } catch (e) {
    toast.dismiss(toastId);
    toast.error("导出失败", { description: String(e) });
    throw e;
  } finally {
    unlisten();
  }
}
