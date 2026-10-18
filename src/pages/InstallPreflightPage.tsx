import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import { toast } from "sonner";
import { useConfigStore } from "@/stores/configStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { detectImportFileKind, parseInstallCommand, type ParsedDependency } from "./importPageLogic";
import { availabilityLabels, filterPreflightItems, formatPreflightMissing, type Availability, type PreflightReport } from "./installPreflightLogic";

interface Progress { request_id: string; completed: number; total: number; current: string }

export function InstallPreflightPage() {
  const { config, loadConfig } = useConfigStore();
  const [registry, setRegistry] = useState("");
  const [command, setCommand] = useState("");
  const [packages, setPackages] = useState<ParsedDependency[]>([]);
  const [expand, setExpand] = useState(true);
  const [source, setSource] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<PreflightReport | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [status, setStatus] = useState<Availability | "all">("all");
  const [query, setQuery] = useState("");
  const requestRef = useRef<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => { void loadConfig(); }, [loadConfig]);
  useEffect(() => { setRegistry((value) => value || config.registry_url); }, [config.registry_url]);
  const items = useMemo(() => filterPreflightItems(report?.items ?? [], status, query), [report, status, query]);
  const virtualizer = useVirtualizer({ count: items.length, getScrollElement: () => listRef.current, estimateSize: () => 76, overscan: 12 });

  const selectFile = async () => {
    const path = await open({ multiple: false, filters: [{ name: "依赖文件", extensions: ["json", "yaml"] }] });
    if (typeof path !== "string") return;
    setBusy(true);
    try {
      const deps = await invoke<ParsedDependency[]>("parse_file", { filePath: path });
      setPackages(deps); setExpand(detectImportFileKind(path) !== "lockfile"); setSource(path); setReport(null);
      if (!deps.length) toast.warning("未解析到依赖");
    } catch (e) { toast.error("解析失败", { description: String(e) }); }
    finally { setBusy(false); }
  };

  const parseCommand = () => {
    try {
      const result = parseInstallCommand(command);
      result.warnings.forEach((warning) => toast.warning(warning));
      setPackages(result.packages); setExpand(true); setSource("安装命令"); setReport(null);
    } catch (e) { toast.error("解析失败", { description: String(e) }); }
  };

  const run = async () => {
    const requestId = `preflight-${Date.now()}`;
    requestRef.current = requestId;
    setBusy(true); setProgress(null); setReport(null);
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<Progress>("install-preflight-progress", ({ payload }) => {
        if (payload.request_id === requestRef.current) setProgress(payload);
      });
      const result = await invoke<PreflightReport>("check_install_preflight", {
        packages, registryUrl: registry, expandDependencies: expand, requestId,
      });
      setReport(result);
    } catch (e) { toast.error("预检失败", { description: String(e) }); }
    finally { unlisten?.(); requestRef.current = null; setBusy(false); }
  };

  const copyMissing = async () => {
    try {
      await navigator.clipboard.writeText(formatPreflightMissing(report?.items ?? []));
      toast.success("已复制缺失清单");
    } catch (e) { toast.error("复制失败", { description: String(e) }); }
  };

  return <div className="flex h-full flex-col gap-3 p-6">
    <div><h1 className="text-2xl font-bold">安装预检</h1>
      <p className="mt-1 text-sm text-muted-foreground">按内网仓库的 npm 选版本规则检查元数据与 tgz。lockfile 检查精确闭包；package.json 和命令继续展开依赖（含可选依赖及 peer）。</p></div>
    <div className="flex gap-2"><Input aria-label="目标 Registry" value={registry} onChange={(e) => { setRegistry(e.target.value); setReport(null); }} disabled={busy} placeholder="内网 Registry" />
      <Button onClick={run} disabled={busy || !packages.length || !registry.trim()}>{busy ? "检查中…" : "开始预检"}</Button></div>
    <div className="flex gap-2"><Button variant="outline" disabled={busy} onClick={selectFile}>选择 lockfile / package.json</Button>
      <Input aria-label="安装命令" placeholder="npm install react@^19" value={command} onChange={(e) => setCommand(e.target.value)} disabled={busy} />
      <Button variant="outline" onClick={parseCommand} disabled={busy || !command.trim()}>解析命令</Button></div>
    <p className="truncate text-sm text-muted-foreground" title={source}>{source ? `${source} · ${packages.length} 个输入` : "请选择依赖文件或解析安装命令"}</p>
    {busy && <p role="status" className="text-sm">{progress ? `已检查 ${progress.completed}/${progress.total} · ${progress.current}` : "准备检查…"}</p>}
    {report && <>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant={status === "all" ? "secondary" : "ghost"} onClick={() => setStatus("all")}>全部 {report.items.length}</Button>
        {(Object.keys(availabilityLabels) as Availability[]).map((key) => <Button key={key} size="sm" variant={status === key ? "secondary" : "ghost"} onClick={() => setStatus(key)}>{availabilityLabels[key]} {report.items.filter((i) => i.status === key).length}</Button>)}
        <Button size="sm" variant="outline" onClick={copyMissing}>复制缺失清单</Button>
      </div>
      <p className="text-xs text-muted-foreground">{report.plugin_used ? "优先使用插件的本地 tarball 索引，索引外包使用轻量探测。" : "使用 HEAD / Range GET 探测下载地址。"}范围未能解析时仍显示原始范围；检查失败需重试。可选项是否实际安装取决于目标平台。</p>
      <Input aria-label="筛选包名" placeholder="筛选包名" value={query} onChange={(e) => setQuery(e.target.value)} />
    </>}
    <div ref={listRef} className="min-h-0 flex-1 overflow-auto rounded-md border">
      {!report ? <p className="p-8 text-center text-muted-foreground">预检结果将在这里显示</p> : <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((row) => { const item = items[row.index]; return <div key={`${item.name}::${item.requested}`} data-index={row.index} ref={virtualizer.measureElement}
          style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${row.start}px)` }} className="flex items-center gap-3 border-b px-4 py-3">
          <div className="min-w-0 flex-1"><p className="truncate font-mono text-sm">{item.name}@{item.requested}{item.version && ` → ${item.version}`}{item.optional && "（可选）"}</p>
            {(item.latest_warning || item.message) && <p className="mt-1 text-xs text-muted-foreground">{item.latest_warning ?? item.message}</p>}</div>
          <Badge variant={item.status === "available" ? "secondary" : "outline"}>{availabilityLabels[item.status]}</Badge></div>; })}
      </div>}
    </div>
  </div>;
}
