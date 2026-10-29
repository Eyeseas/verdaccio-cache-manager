import { useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import type { CacheTask } from "@/stores/taskStore";
import { availabilityLabels, type PreflightReport } from "./installPreflightLogic";
import { summarizeVerification, type ReplenishmentList } from "./replenishmentLogic";
import { summarizeLatest, type LatestOutcome } from "./offlineBundleLogic";

interface BundleFile { name: string; version: string; path: string; error: string | null }
interface Inspection { files: BundleFile[]; replenishment: ReplenishmentList | null;
  manifest: { entries: unknown[]; failures: { name: string; version: string; reason: string }[] } | null }
interface ImportResult { inspection: Inspection; tasks: CacheTask[]; latest: LatestOutcome[]; verification: PreflightReport | null; verification_error: string | null }

const uploadLabels: Record<string, string> = { Pending: "等待", Downloading: "读取中", Uploading: "上传中", Success: "成功", Skipped: "已存在", Failed: "失败" };
const latestLabels: Record<string, string> = { promoted: "已推进", kept: "已保留", failed: "未推进" };

export function OfflineBundleTab() {
  const navigate = useNavigate();
  const [directory, setDirectory] = useState("");
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [live, setLive] = useState<Map<string, CacheTask>>(new Map());
  const listRef = useRef<HTMLDivElement>(null);
  const tasks = useMemo(() => result ? new Map(result.tasks.map((t) => [`${t.package_name}@${t.version}`, t])) : live, [result, live]);
  const finished = [...live.values()].filter((t) => ["Success", "Skipped", "Failed"].includes(t.status)).length;
  const rows = useMemo(() => [
    ...(inspection?.files.map((file) => {
      const task = tasks.get(`${file.name}@${file.version}`);
      return { name: `${file.name}@${file.version}`, status: file.error ? "校验失败，不上传" : uploadLabels[task?.status ?? ""] ?? "待上传", message: file.error ?? task?.error };
    }) ?? []),
    ...(result?.verification?.items.map((item) => ({ name: `清单核对：${item.name}@${item.version ?? item.requested}`, status: availabilityLabels[item.status], message: item.message })) ?? []),
    ...(inspection?.manifest?.failures.map((failure) => ({ name: `外网导出失败：${failure.name}@${failure.version}`, status: "缺文件", message: failure.reason })) ?? []),
    ...(result?.latest.map((item) => ({ name: `latest：${item.name} · 期望 ${item.expected_latest} → 实际 ${item.actual_latest ?? "未确认"}`, status: latestLabels[item.action] ?? item.action, message: item.message })) ?? []),
  ], [inspection, result, tasks]);
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => listRef.current, estimateSize: () => 55, overscan: 12 });

  const select = async () => {
    const path = await open({ directory: true, multiple: false });
    if (typeof path !== "string") return;
    setBusy(true); setResult(null); setInspection(null); setLive(new Map()); setDirectory(path);
    try { setInspection(await invoke<Inspection>("inspect_offline_bundle", { directory: path })); }
    catch (e) { toast.error("读取离线包失败", { description: String(e) }); }
    finally { setBusy(false); }
  };

  const upload = async () => {
    setBusy(true); setResult(null); setLive(new Map()); setProgress("准备上传…");
    let unlisten: (() => void) | undefined;
    // 事件可能很密集，先攒到缓冲区，定时批量刷新到列表
    const pending = new Map<string, CacheTask>();
    const flush = () => {
      if (pending.size === 0) return;
      const updates = [...pending.entries()]; pending.clear();
      setLive((prev) => { const next = new Map(prev); for (const [key, task] of updates) next.set(key, task); return next; });
    };
    const timer = setInterval(flush, 150);
    try {
      unlisten = await listen<CacheTask>("task-progress", ({ payload }) => {
        if (!payload.batch_id.startsWith("offline-")) return;
        pending.set(`${payload.package_name}@${payload.version}`, payload);
        setProgress(`${payload.package_name}@${payload.version} · ${uploadLabels[payload.status] ?? payload.status}`);
      });
      const report = await invoke<ImportResult>("import_offline_bundle", { directory });
      setResult(report);
      setInspection(report.inspection);
      if (report.verification) toast.info(summarizeVerification(report.verification.items));
    } catch (e) { toast.error("导入失败", { description: String(e) }); }
    finally { clearInterval(timer); flush(); unlisten?.(); setBusy(false); setProgress(""); }
  };

  return <div className="flex min-h-0 flex-1 flex-col gap-3 pt-3">
    <p className="text-sm text-muted-foreground">选择外网导出的 tarball 目录。附带 manifest 时先校验完整性，上传后按清单推进 latest；附带补包清单时逐项检查内网可下载状态。旧目录也可导入。</p>
    <div className="flex items-center gap-3"><Button variant="outline" onClick={select} disabled={busy}>选择离线包目录</Button>
      <span className="truncate text-sm text-muted-foreground" title={directory}>{directory}</span></div>
    {inspection && <p className="text-sm">共 {inspection.files.length} 个文件，{inspection.files.filter((f) => f.error).length} 个校验失败{inspection.replenishment && `，附带 ${inspection.replenishment.entries.length} 条补包清单`}</p>}
    {inspection?.manifest && <p className="text-sm text-muted-foreground">已读取 manifest：{inspection.manifest.entries.length} 个文件，{inspection.manifest.failures.length} 个外网导出失败项。完整性校验失败的文件不会上传。</p>}
    {busy && <p role="status" className="text-sm">{live.size > 0 && `已完成 ${finished}/${inspection?.files.filter((f) => !f.error).length ?? 0} · `}{progress || "读取中…"}</p>}
    {result && <div className="flex flex-wrap items-center gap-3 text-sm">
      <span>上传成功或已存在 {result.tasks.filter((t) => ["Success", "Skipped"].includes(t.status)).length}/{result.tasks.length}</span>
      {result.latest.length > 0 && <span>{summarizeLatest(result.latest)}</span>}
      {result.verification && <span>{summarizeVerification(result.verification.items)}</span>}
      {result.verification_error && <span className="text-destructive">核对失败：{result.verification_error}</span>}
      <Button size="sm" variant="outline" onClick={() => navigate("/health")}>前往仓库体检</Button>
    </div>}
    <div ref={listRef} className="min-h-0 flex-1 overflow-auto rounded-md border">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((row) => { const value = rows[row.index]; return <div key={row.index} data-index={row.index} ref={virtualizer.measureElement}
          style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }} className="flex items-center gap-3 border-b px-4 py-2">
          <div className="min-w-0 flex-1"><p className="truncate font-mono text-sm">{value.name}</p>{value.message && <p className="text-xs text-destructive">{value.message}</p>}</div><Badge variant="outline">{value.status}</Badge></div>; })}
      </div>
    </div>
    <Button onClick={upload} disabled={busy || !inspection?.files.some((file) => !file.error)}>{busy ? "处理中…" : "上传并核对补包清单"}</Button>
  </div>;
}
