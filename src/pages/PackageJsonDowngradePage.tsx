import { useCallback, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  FileDown,
  FileJson,
  FolderOpen,
  Loader2,
  RefreshCw,
  Save,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { useTauriFileDrop } from "@/hooks/useTauriFileDrop";
import { useSyncStore } from "@/stores/syncStore";
import {
  createDowngradeRequestId,
  filterDowngradeItems,
  isPackageJsonPath,
  isStaleAnalysis,
  statusLabel,
  statusVariant,
  type DowngradeAnalysis,
  type DowngradeFilter,
  type DowngradeItem,
} from "./packageJsonDowngradeLogic";

interface SaveResult {
  output_path: string;
}

interface OverwriteResult {
  file_path: string;
  backup_path: string;
}

const filterLabels: Record<DowngradeFilter, string> = {
  all: "全部",
  changed: "将修改",
  missing: "无缓存/跳过",
};

export function PackageJsonDowngradePage() {
  const startSync = useSyncStore((s) => s.startSync);
  const syncStatus = useSyncStore((s) => s.status);

  const [analysis, setAnalysis] = useState<DowngradeAnalysis | null>(null);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [allowMajorDowngrade, setAllowMajorDowngrade] = useState(false);
  const [filter, setFilter] = useState<DowngradeFilter>("all");
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [overwriting, setOverwriting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const requestIdRef = useRef<string | null>(null);
  const dropZoneRef = useRef<HTMLDivElement>(null);

  const analyzePackage = useCallback(
    async (path: string, allowMajor: boolean) => {
      if (!isPackageJsonPath(path)) {
        setError("请选择 package.json 文件");
        return;
      }

      const requestId = createDowngradeRequestId();
      requestIdRef.current = requestId;
      setLoading(true);
      setError(null);

      try {
        const next = await invoke<DowngradeAnalysis>(
          "analyze_package_json_downgrade",
          {
            filePath: path,
            allowMajorDowngrade: allowMajor,
            requestId,
          }
        );
        if (isStaleAnalysis(requestIdRef.current, next)) return;
        setAnalysis(next);
        setFilePath(path);
        setFilter("all");
      } catch (e) {
        setError(`分析失败: ${e}`);
        toast.error("分析 package.json 失败", { description: String(e) });
      } finally {
        if (requestIdRef.current === requestId) setLoading(false);
      }
    },
    []
  );

  const handleSelectFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "package.json", extensions: ["json"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    await analyzePackage(selected, allowMajorDowngrade);
  };

  const handleToggleMajor = async (checked: boolean) => {
    setAllowMajorDowngrade(checked);
    if (filePath) {
      await analyzePackage(filePath, checked);
    }
  };

  const handleSave = async () => {
    if (!analysis) return;
    setSaving(true);
    try {
      const outputPath = await save({
        defaultPath: "package.downgraded.json",
        filters: [{ name: "package.json", extensions: ["json"] }],
      });
      if (!outputPath) return;
      const result = await invoke<SaveResult>("save_downgraded_package_json", {
        outputPath,
        content: analysis.updated_content,
      });
      toast.success("已生成 package.json", {
        description: result.output_path,
      });
    } catch (e) {
      toast.error("保存失败", { description: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const handleOverwrite = async () => {
    if (!analysis) return;
    setOverwriting(true);
    try {
      const result = await invoke<OverwriteResult>("overwrite_package_json", {
        filePath: analysis.file_path,
        content: analysis.updated_content,
      });
      setConfirmOpen(false);
      toast.success("已覆盖 package.json", {
        description: `备份: ${result.backup_path}`,
      });
    } catch (e) {
      toast.error("覆盖失败", { description: String(e) });
    } finally {
      setOverwriting(false);
    }
  };

  const { isOver } = useTauriFileDrop({
    zoneRef: dropZoneRef,
    filter: isPackageJsonPath,
    enabled: !analysis,
    onDrop: async (paths) => {
      if (paths[0]) await analyzePackage(paths[0], allowMajorDowngrade);
    },
  });

  const visibleItems = useMemo(
    () => filterDowngradeItems(analysis?.items ?? [], filter),
    [analysis?.items, filter]
  );

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">package.json 降级</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            根据已缓存版本生成内网可安装的 package.json
          </p>
        </div>
        <div className="flex items-center gap-2">
          <label className="flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-sm">
            <Checkbox
              checked={allowMajorDowngrade}
              disabled={loading}
              onCheckedChange={(value) => void handleToggleMajor(Boolean(value))}
            />
            <span>允许跨 major 降级</span>
          </label>
          {analysis && (
            <Button variant="outline" onClick={handleSelectFile} disabled={loading}>
              <FolderOpen className="h-4 w-4" />
              重新选择
            </Button>
          )}
        </div>
      </div>

      {!analysis && !loading && (
        <div
          ref={dropZoneRef}
          className={`flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center transition-colors ${
            isOver ? "border-primary bg-primary/5" : ""
          }`}
        >
          <FileJson className="mb-4 h-12 w-12 text-muted-foreground" />
          <p className="mb-2 text-lg font-medium">拖入 package.json 或点击选择</p>
          <p className="mb-4 text-sm text-muted-foreground">
            只会分析 dependencies、devDependencies、optionalDependencies
          </p>
          {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
          <Button variant="outline" onClick={handleSelectFile}>
            <FolderOpen className="h-4 w-4" />
            选择 package.json
          </Button>
        </div>
      )}

      {loading && (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" />
          <span>分析中...</span>
        </div>
      )}

      {analysis && !loading && (
        <>
          <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-3">
              <Badge variant="outline">{analysis.file_name}</Badge>
              <span className="text-sm text-muted-foreground">
                {allowMajorDowngrade ? "允许跨 major" : "同 major 优先"}
              </span>
            </div>
            {analysis.cache_index_empty && (
              <Button
                variant="outline"
                size="sm"
                onClick={startSync}
                disabled={syncStatus === "syncing"}
              >
                <RefreshCw className={syncStatus === "syncing" ? "animate-spin" : ""} />
                同步缓存索引
              </Button>
            )}
          </div>

          {analysis.cache_index_empty && (
            <div className="mb-3 flex items-center gap-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              <AlertTriangle className="h-4 w-4" />
              本地缓存索引为空，生成结果可能仍无法完成 npm i。
            </div>
          )}

          <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_360px] gap-4">
            <div className="flex min-h-0 flex-col gap-3">
              <SummaryGrid analysis={analysis} />
              <div className="flex gap-2">
                {(Object.keys(filterLabels) as DowngradeFilter[]).map((key) => (
                  <Button
                    key={key}
                    variant={filter === key ? "secondary" : "outline"}
                    size="sm"
                    onClick={() => setFilter(key)}
                  >
                    {filterLabels[key]}
                  </Button>
                ))}
              </div>
              <div className="min-h-0 flex-1 overflow-auto rounded-md border">
                <div className="grid grid-cols-[minmax(0,1.2fr)_140px_120px_120px_100px] gap-3 border-b bg-muted/40 px-3 py-2 text-xs font-medium text-muted-foreground">
                  <span>包名</span>
                  <span>字段</span>
                  <span>原版本</span>
                  <span>目标版本</span>
                  <span>状态</span>
                </div>
                {visibleItems.map((item) => (
                  <DowngradeRow key={`${item.section}:${item.name}`} item={item} />
                ))}
              </div>
            </div>

            <aside className="flex min-h-0 flex-col rounded-md border bg-muted/20">
              <div className="border-b px-3 py-2 text-sm font-medium">改写预览</div>
              <pre className="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-relaxed">
                {analysis.updated_content}
              </pre>
              <div className="flex flex-col gap-2 border-t p-3">
                <Button onClick={handleSave} disabled={saving || overwriting}>
                  {saving ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Save className="h-4 w-4" />
                  )}
                  另存为新 package.json
                </Button>
                <Button
                  variant="destructive"
                  onClick={() => setConfirmOpen(true)}
                  disabled={saving || overwriting}
                >
                  <FileDown className="h-4 w-4" />
                  确认后覆盖原文件
                </Button>
              </div>
            </aside>
          </div>
        </>
      )}

      <OverwriteDialog
        open={confirmOpen}
        loading={overwriting}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={handleOverwrite}
      />
    </div>
  );
}

function SummaryGrid({ analysis }: { analysis: DowngradeAnalysis }) {
  const cards = [
    ["依赖总数", analysis.summary.total],
    ["将修改", analysis.summary.changed],
    ["已缓存", analysis.summary.unchanged_cached],
    ["无缓存", analysis.summary.missing_cache],
    ["跨 major", analysis.summary.major_downgraded],
  ] as const;

  return (
    <div className="grid grid-cols-5 gap-2">
      {cards.map(([label, value]) => (
        <div key={label} className="rounded-md border px-3 py-2">
          <div className="text-xl font-semibold">{value}</div>
          <div className="text-xs text-muted-foreground">{label}</div>
        </div>
      ))}
    </div>
  );
}

function DowngradeRow({ item }: { item: DowngradeItem }) {
  return (
    <div className="grid grid-cols-[minmax(0,1.2fr)_140px_120px_120px_100px] gap-3 border-b px-3 py-2 text-sm">
      <span className="truncate font-mono">{item.name}</span>
      <span className="truncate text-muted-foreground">{item.section}</span>
      <span className="truncate font-mono">{item.original_spec}</span>
      <span className="truncate font-mono">{item.target_version ?? "-"}</span>
      <span>
        <Badge variant={statusVariant(item.status)}>{statusLabel(item.status)}</Badge>
      </span>
    </div>
  );
}

function OverwriteDialog({
  open,
  loading,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  loading: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="mx-4 w-full max-w-md rounded-lg bg-popover p-5 text-popover-foreground shadow-lg ring-1 ring-foreground/10">
        <div className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="h-5 w-5" />
          <h2 className="text-lg font-semibold">覆盖原 package.json</h2>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">
          此操作会覆盖原 package.json。覆盖前会在同目录创建备份文件。
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel} disabled={loading}>
            取消
          </Button>
          <Button variant="destructive" onClick={onConfirm} disabled={loading}>
            {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            确认覆盖
          </Button>
        </div>
      </div>
    </div>
  );
}
