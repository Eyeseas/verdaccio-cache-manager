import { toast } from "sonner";

export interface DownloadFailure {
  package: string;
  reason: string;
}

export interface DownloadSummary {
  success: number;
  failed: number;
  failures: DownloadFailure[];
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
