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

interface BundleFile { name: string; version: string; path: string; error: string | null }
interface Inspection { files: BundleFile[]; replenishment: ReplenishmentList | null }
interface ImportResult { tasks: CacheTask[]; verification: PreflightReport | null; verification_error: string | null }

export function OfflineBundleTab() {
  const navigate = useNavigate();
  const [directory, setDirectory] = useState("");
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const tasks = useMemo(() => new Map(result?.tasks.map((t) => [`${t.package_name}@${t.version}`, t])), [result]);
  const rows = useMemo(() => [
    ...(inspection?.files.map((file) => {
      const task = tasks.get(`${file.name}@${file.version}`);
      return { name: `${file.name}@${file.version}`, status: file.error ? "校验失败，不上传" : task?.status ?? "待上传", message: file.error ?? task?.error };
    }) ?? []),
    ...(result?.verification?.items.map((item) => ({ name: `清单核对：${item.name}@${item.version ?? item.requested}`, status: availabilityLabels[item.status], message: item.message })) ?? []),
  ], [inspection, result, tasks]);
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => listRef.current, estimateSize: () => 55, overscan: 12 });

  const select = async () => {
    const path = await open({ directory: true, multiple: false });
    if (typeof path !== "string") return;
    setBusy(true); setResult(null); setInspection(null); setDirectory(path);
    try { setInspection(await invoke<Inspection>("inspect_offline_bundle", { directory: path })); }
    catch (e) { toast.error("读取离线包失败", { description: String(e) }); }
    finally { setBusy(false); }
  };

  const upload = async () => {
    setBusy(true); setResult(null); setProgress("准备上传…");
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<CacheTask>("task-progress", ({ payload }) => {
        if (payload.batch_id.startsWith("offline-")) setProgress(`${payload.package_name}@${payload.version} · ${payload.status}`);
      });
      const report = await invoke<ImportResult>("import_offline_bundle", { directory });
      setResult(report);
      if (report.verification) toast.info(summarizeVerification(report.verification.items));
    } catch (e) { toast.error("导入失败", { description: String(e) }); }
    finally { unlisten?.(); setBusy(false); setProgress(""); }
  };

  return <div className="flex min-h-0 flex-1 flex-col gap-3 pt-3">
    <p className="text-sm text-muted-foreground">选择外网导出的 tarball 目录。附带 replenishment.json 时，上传后逐项检查内网可下载状态；旧目录也可导入。</p>
    <div className="flex items-center gap-3"><Button variant="outline" onClick={select} disabled={busy}>选择离线包目录</Button>
      <span className="truncate text-sm text-muted-foreground" title={directory}>{directory}</span></div>
    {inspection && <p className="text-sm">共 {inspection.files.length} 个文件，{inspection.files.filter((f) => f.error).length} 个校验失败{inspection.replenishment && `，附带 ${inspection.replenishment.entries.length} 条补包清单`}</p>}
    {busy && <p role="status" className="text-sm">{progress || "读取中…"}</p>}
    {result && <div className="flex flex-wrap items-center gap-3 text-sm">
      <span>上传成功或已存在 {result.tasks.filter((t) => ["Success", "Skipped"].includes(t.status)).length}/{result.tasks.length}</span>
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
