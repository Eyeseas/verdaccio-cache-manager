import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useConfigStore } from "@/stores/configStore";
import { useHealthStore } from "@/stores/healthStore";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Copy,
  FolderOpen,
  Globe,
  Loader2,
  Search,
  ShieldCheck,
  TriangleAlert,
  Stethoscope,
  Wrench,
} from "lucide-react";
import { toast } from "sonner";
import {
  type PackageHealth,
  type HealthReport,
  type RepairOutcome,
  formatMissingList,
  summarizeRepair,
} from "./storageHealthLogic";

function outcomeBadge(outcome: RepairOutcome) {
  switch (outcome.action) {
    case "repaired":
      return (
        <Badge
          variant="outline"
          className="border-green-300 text-green-600"
          title={outcome.backup_path ?? undefined}
        >
          {[
            outcome.removed_versions.length > 0 && `剔除 ${outcome.removed_versions.length} 个`,
            outcome.new_latest && `latest → ${outcome.new_latest}`,
          ]
            .filter(Boolean)
            .join("，")}
        </Badge>
      );
    case "removed":
      return (
        <Badge variant="outline" className="border-green-300 text-green-600">
          已移除元数据
        </Badge>
      );
    case "skipped":
      return <Badge variant="secondary">无需处理</Badge>;
    case "failed":
      return (
        <Badge
          variant="outline"
          className="border-red-300 text-red-600"
          title={outcome.message ?? undefined}
        >
          失败
        </Badge>
      );
  }
}

function healthBadges(pkg: PackageHealth) {
  if (pkg.cached_versions.length === 0) {
    return (
      <Badge variant="outline" className="border-red-300 text-red-600">
        无 tarball
      </Badge>
    );
  }
  if (pkg.latest_missing) {
    return (
      <Badge variant="outline" className="border-yellow-300 text-yellow-600">
        latest 缺失
      </Badge>
    );
  }
  if (pkg.latest_stale) {
    return (
      <Badge variant="outline" className="border-yellow-300 text-yellow-600">
        latest 过时
      </Badge>
    );
  }
  return null;
}

type Mode = "local" | "remote";

const LOCAL_REPAIR_NOTICE = "原文件会备份为 package.json.<时间戳>.bak。";
const REMOTE_REPAIR_NOTICE =
  "将通过 Registry API 修改（需要 unpublish/publish 权限），不会生成备份；无 tarball 的包会被整包删除。";

function describeProblem(pkg: PackageHealth): string {
  const parts: string[] = [];
  if (pkg.latest_stale) {
    parts.push(`latest ${pkg.latest ?? "无"} → ${pkg.expected_latest}`);
  }
  if (pkg.missing_versions.length > 0) {
    const recent = pkg.missing_versions.slice(-5).reverse().join(", ");
    const more = pkg.missing_versions.length > 5 ? " …" : "";
    parts.push(`缺 ${pkg.missing_versions.length} 个：${recent}${more}`);
  }
  return parts.join("；");
}

import { replenishmentFromHealth } from "./replenishmentLogic";
import { saveReplenishmentList } from "@/lib/replenishment";

export function StorageHealthPage() {
  const { config, loadConfig } = useConfigStore();
  const { latest, busy, runCheck, beginRepair, endRepair } = useHealthStore();
  const [mode, setMode] = useState<Mode>(latest?.mode ?? config.health_check?.mode ?? "local");
  const [storagePath, setStoragePath] = useState(latest?.mode === "local" ? latest.value : config.verdaccio_storage_path ?? "");
  const [registryUrl, setRegistryUrl] = useState(latest?.mode === "remote" ? latest.value : config.registry_url);
  const [report, setReport] = useState<HealthReport | null>(latest?.report ?? null);
  const [repairing, setRepairing] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [outcomes, setOutcomes] = useState<Map<string, RepairOutcome>>(new Map());
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);
  const registryDefaultRef = useRef(config.registry_url);
  const scanning = busy && !repairing;

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  useEffect(() => {
    if (!useHealthStore.getState().latest) setMode(config.health_check?.mode ?? "local");
  }, [config.health_check?.mode]);

  useEffect(() => {
    if (!storagePath && config.verdaccio_storage_path) {
      setStoragePath(config.verdaccio_storage_path);
    }
  }, [config.verdaccio_storage_path, storagePath]);

  useEffect(() => {
    const previousDefault = registryDefaultRef.current;
    registryDefaultRef.current = config.registry_url;
    setRegistryUrl((current) => !current || current === previousDefault ? config.registry_url : current);
  }, [config.registry_url]);

  const target = mode === "local" ? storagePath : registryUrl;
  const currentSnapshot = latest?.mode === mode && latest.value === target.trim() ? latest : null;

  useEffect(() => {
    setReport(currentSnapshot?.report ?? null);
    setSelected(new Set());
    setOutcomes(new Map());
  }, [currentSnapshot]);

  const switchMode = (next: Mode) => {
    if (next === mode) return;
    setMode(next);
    setReport(null);
    setSelected(new Set());
    setOutcomes(new Map());
  };

  const packages = useMemo(() => {
    const list = report?.unhealthy ?? [];
    const q = query.trim().toLowerCase();
    return q ? list.filter((p) => p.name.toLowerCase().includes(q)) : list;
  }, [report, query]);

  const rowVirtualizer = useVirtualizer({
    count: packages.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 52,
    overscan: 12,
  });

  const runScan = async (value = target) => {
    if (!value) {
      toast.error(mode === "local" ? "请先填写或选择 Verdaccio storage 目录" : "请先填写 Registry 地址");
      return;
    }
    const result = await runCheck({ mode, value }, "manual");
    if (result?.error) {
      toast.error("体检失败", { description: result.error });
    }
  };

  const handleSelectDir = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") {
      setStoragePath(dir);
      await runScan(dir);
    }
  };

  const toggleSelect = (name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const selectAll = () =>
    setSelected(
      new Set(packages.filter((p) => !outcomes.has(p.name)).map((p) => p.name)),
    );

  const handleCopy = async () => {
    const source = selected.size > 0
      ? packages.filter((p) => selected.has(p.name))
      : packages;
    const text = formatMissingList(source);
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.success(`已复制 ${text.split("\n").length} 条缺失版本`);
    } catch (e) {
      toast.error("复制失败", { description: String(e) });
    }
  };

  const handleRepair = async () => {
    const names = [...selected];
    if (names.length === 0) return;
    const confirmed = await ask(
      `将修改 ${names.length} 个包的元数据：剔除没有 tarball 的版本，并把 dist-tags.latest 改为本地可用的最高正式版；完全没有 tarball 的包会移除其元数据。\n\n${mode === "local" ? LOCAL_REPAIR_NOTICE : REMOTE_REPAIR_NOTICE}\n\n仅适用于离线 Verdaccio：若 uplink 仍可访问，元数据会被重新合并回来。`,
      { title: "确认修复", kind: "warning", okLabel: "修复", cancelLabel: "取消" },
    );
    if (!confirmed) return;
    if (!beginRepair()) return;

    setRepairing(true);
    try {
      const result = mode === "local"
        ? await invoke<RepairOutcome[]>("repair_storage_packages", { storagePath, names })
        : await invoke<RepairOutcome[]>("repair_remote_storage_packages", {
            registryUrl,
            packages: (report?.unhealthy ?? []).filter((p) => selected.has(p.name)),
          });
      setOutcomes((prev) => {
        const next = new Map(prev);
        result.forEach((o) => next.set(o.name, o));
        return next;
      });
      setSelected(new Set());
      const summary = summarizeRepair(result);
      if (summary.failed > 0) {
        toast.warning("修复部分完成", { description: summary.text });
      } else {
        toast.success("修复完成", { description: summary.text });
      }
    } catch (e) {
      toast.error("修复失败", { description: String(e) });
    } finally {
      setRepairing(false);
      endRepair();
    }
    await runCheck({ mode, value: target }, "manual");
  };

  const latestStaleCount = report?.unhealthy.filter((p) => p.latest_stale).length ?? 0;

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4">
        <h1 className="text-2xl font-bold">仓库体检</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          检查 Verdaccio storage 中「元数据有、tarball 无」的版本，以及过时的 dist-tags.latest。npm 只要 latest 满足范围就优先选它，离线环境下选中无 tarball 的版本会返回 500 导致缺包。
        </p>
      </div>

      {currentSnapshot && (
        <p className={`mb-3 text-sm ${currentSnapshot.error ? "text-yellow-600" : "text-muted-foreground"}`}>
          {currentSnapshot.source === "automatic" ? "最近自动体检" : "最近体检"}：{new Date(currentSnapshot.completedAt).toLocaleString("zh-CN")}
          {currentSnapshot.error && `，体检失败：${currentSnapshot.error}`}
        </p>
      )}

      <div className="mb-3 flex gap-2">
        <div className="flex shrink-0 rounded-md border p-0.5">
          <Button
            variant={mode === "local" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => switchMode("local")}
            disabled={scanning || repairing}
          >
            <FolderOpen className="mr-1.5 h-3.5 w-3.5" />
            本地目录
          </Button>
          <Button
            variant={mode === "remote" ? "secondary" : "ghost"}
            size="sm"
            onClick={() => switchMode("remote")}
            disabled={scanning || repairing}
          >
            <Globe className="mr-1.5 h-3.5 w-3.5" />
            远程插件
          </Button>
        </div>
        {mode === "local" ? (
          <>
            <Input
              value={storagePath}
              onChange={(e) => setStoragePath(e.target.value)}
              disabled={busy}
              placeholder="/path/to/verdaccio/storage"
              className="font-mono"
            />
            <Button variant="outline" size="icon" onClick={handleSelectDir} disabled={busy}>
              <FolderOpen className="h-4 w-4" />
            </Button>
          </>
        ) : (
          <Input
            value={registryUrl}
            onChange={(e) => setRegistryUrl(e.target.value)}
            disabled={busy}
            placeholder="http://localhost:4873"
            className="font-mono"
          />
        )}
        <Button onClick={() => runScan()} disabled={busy || !target.trim()}>
          {scanning ? (
            <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
          ) : (
            <Stethoscope className="mr-1.5 h-4 w-4" />
          )}
          {scanning ? "体检中" : "开始体检"}
        </Button>
      </div>

      {!report ? (
        <div className="flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center">
          <Stethoscope className="mb-4 h-12 w-12 text-muted-foreground" />
          {mode === "local" ? (
            <>
              <p className="mb-2 text-lg font-medium">选择 Verdaccio storage 目录</p>
              <p className="text-sm text-muted-foreground">
                需要能直接访问 storage 目录（本机路径或 SMB/NFS 挂载），默认使用设置中的 Storage 路径
              </p>
            </>
          ) : (
            <>
              <p className="mb-2 text-lg font-medium">通过插件远程体检</p>
              <p className="max-w-lg text-sm text-muted-foreground">
                适用于 storage 在远程服务器或 Docker 卷中的情况。需要 Verdaccio 安装 verdaccio-cached-list 插件 0.2.0 及以上（设置页可导出），修复通过 Registry API 完成
              </p>
            </>
          )}
        </div>
      ) : report.unhealthy.length === 0 && report.unreadable.length === 0 && !report.plugin_outdated ? (
        <div className="flex flex-1 flex-col items-center justify-center rounded-lg border p-12 text-center">
          <ShieldCheck className="mb-4 h-12 w-12 text-green-600" />
          <p className="mb-2 text-lg font-medium">一切正常</p>
          <p className="text-sm text-muted-foreground">
            已检查 {report.scanned} 个包，所有版本都有对应 tarball，latest 均指向本地最高正式版
          </p>
        </div>
      ) : (
        <>
          {report.plugin_outdated && (
            <div className="mb-3 flex items-start gap-2 rounded-md border border-yellow-300 bg-yellow-500/10 p-3 text-sm text-yellow-700 dark:text-yellow-500">
              <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                服务器上的 verdaccio-cached-list 插件版本过旧：看不到完全没有 tarball 的包，latest 缺失时的判断也不准确。请在设置页导出 0.2.0 插件包，重新安装后重启 Verdaccio。
              </span>
            </div>
          )}
          <div className="mb-3 flex items-center justify-between gap-3">
            <span className="text-sm text-muted-foreground">
              已检查 {report.scanned} 个包，<strong className="text-foreground">{report.unhealthy.length}</strong> 个存在问题
              {latestStaleCount > 0 && `，其中 ${latestStaleCount} 个 latest 需修正`}
              {report.unreadable.length > 0 && `，${report.unreadable.length} 个元数据无法解析`}
            </span>
            <div className="flex shrink-0 gap-2">
              <Button variant="ghost" size="sm" onClick={selectAll} disabled={repairing}>
                全选
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())} disabled={repairing}>
                取消全选
              </Button>
              <Button variant="outline" size="sm" onClick={handleCopy}>
                <Copy className="mr-1.5 h-3.5 w-3.5" />
                {selected.size > 0 ? "复制选中缺失清单" : "复制缺失清单"}
              </Button>
              <Button variant="outline" size="sm" onClick={async () => {
                try {
                  const source = selected.size ? packages.filter((p) => selected.has(p.name)) : packages;
                  await saveReplenishmentList(replenishmentFromHealth(source, registryUrl || config.registry_url));
                } catch (e) { toast.error("导出失败", { description: String(e) }); }
              }}>导出补包清单</Button>
            </div>
          </div>

          <div className="relative mb-3">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              type="search"
              placeholder="按包名搜索..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              className="h-9 pl-8"
            />
          </div>

          <div ref={listRef} className="min-h-0 flex-1 overflow-auto rounded-md border">
            {packages.length === 0 ? (
              <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
                未匹配到任何包
              </div>
            ) : (
              <div style={{ height: `${rowVirtualizer.getTotalSize()}px`, position: "relative" }}>
                {rowVirtualizer.getVirtualItems().map((row) => {
                  const pkg = packages[row.index];
                  const outcome = outcomes.get(pkg.name);
                  return (
                    <div
                      key={pkg.name}
                      data-index={row.index}
                      ref={rowVirtualizer.measureElement}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        transform: `translateY(${row.start}px)`,
                      }}
                      className="flex items-center gap-3 border-b px-4 py-2"
                    >
                      {outcome ? (
                        <span className="h-4 w-4" />
                      ) : (
                        <Checkbox
                          checked={selected.has(pkg.name)}
                          onCheckedChange={() => toggleSelect(pkg.name)}
                          disabled={repairing}
                        />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="truncate font-mono text-sm">{pkg.name}</div>
                        <div
                          className="truncate text-xs text-muted-foreground"
                          title={pkg.missing_versions.join(", ")}
                        >
                          {describeProblem(pkg)}
                        </div>
                      </div>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        本地 {pkg.cached_versions.length} 个
                      </span>
                      {outcome ? outcomeBadge(outcome) : healthBadges(pkg)}
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
            <span className="text-sm">
              已选 <strong>{selected.size}</strong> 个包
            </span>
            <Button onClick={handleRepair} disabled={busy || selected.size === 0}>
              {repairing ? (
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
              ) : (
                <Wrench className="mr-1.5 h-4 w-4" />
              )}
              {repairing ? "修复中" : "修复选中"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
