import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTaskStore } from "@/stores/taskStore";
import { useTauriFileDrop } from "@/hooks/useTauriFileDrop";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { FolderOpen, Upload, Loader2, Package, Search } from "lucide-react";

const isTgzFile = (path: string) => path.toLowerCase().endsWith(".tgz");

interface LocalPackage {
  name: string;
  version: string;
  path: string;
}

interface PackageWithStatus extends LocalPackage {
  cached: boolean;
}

interface ScanProgressEvent {
  count: number;
  current: string;
}

export function UploadPage() {
  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-4 text-2xl font-bold">本地上传</h1>
      <Tabs defaultValue="scan" className="flex min-h-0 flex-1 flex-col">
        <TabsList className="w-fit">
          <TabsTrigger value="scan">扫描 node_modules</TabsTrigger>
          <TabsTrigger value="tgz">上传 .tgz</TabsTrigger>
        </TabsList>
        <TabsContent value="scan" className="flex min-h-0 flex-1 flex-col">
          <ScanTab />
        </TabsContent>
        <TabsContent value="tgz" className="flex min-h-0 flex-1 flex-col">
          <TgzTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function ScanTab() {
  const { startCacheTasks } = useTaskStore();

  const [packages, setPackages] = useState<PackageWithStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [dirPath, setDirPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ScanProgressEvent | null>(null);
  const [query, setQuery] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const handleSelectDir = async () => {
    const dir = await open({ directory: true });
    if (!dir) return;

    setLoading(true);
    setPackages([]);
    setSelected(new Set());
    setDirPath(dir);
    setError(null);
    setProgress(null);

    const unlistenPromise = listen<ScanProgressEvent>("scan-progress", (e) => {
      setProgress(e.payload);
    });

    try {
      const scanned = await invoke<LocalPackage[]>("scan_node_modules", {
        dirPath: dir,
      });

      if (scanned.length === 0) {
        setError("未在该目录下发现任何包，请确认已安装依赖");
        return;
      }

      const withStatus: PackageWithStatus[] = scanned.map((pkg) => ({
        ...pkg,
        cached: false,
      }));

      setPackages(withStatus);
      const allIndices = new Set<number>(withStatus.map((_, i) => i));
      setSelected(allIndices);
      setQuery("");
    } catch (e) {
      console.error("扫描失败:", e);
      setError(`扫描失败: ${e}`);
    } finally {
      (await unlistenPromise)();
      setLoading(false);
      setProgress(null);
    }
  };

  const toggleSelect = (i: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  const filteredIndices = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return packages.map((_, i) => i);
    return packages
      .map((p, i) => ({ p, i }))
      .filter(({ p }) => p.name.toLowerCase().includes(q))
      .map(({ i }) => i);
  }, [packages, query]);

  const rowVirtualizer = useVirtualizer({
    count: filteredIndices.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 36,
    overscan: 12,
  });

  const selectAllUncached = () => {
    const next = new Set(selected);
    for (const i of filteredIndices) {
      if (!packages[i].cached) next.add(i);
    }
    setSelected(next);
  };

  const deselectAll = () => setSelected(new Set());

  const handleUpload = async () => {
    const pkgs = Array.from(selected).map((i) => ({
      package_name: packages[i].name,
      version: packages[i].version,
      tarball_url: `dir://${packages[i].path}`,
    }));
    if (pkgs.length === 0) return;
    await startCacheTasks(pkgs);
    setSelected(new Set());
  };

  const uncachedCount = packages.filter((p) => !p.cached).length;

  if (loading) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2">
        <div className="flex items-center">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" />
          <span>
            扫描 node_modules 中
            {progress ? `... 已发现 ${progress.count} 个包` : "..."}
          </span>
        </div>
        {progress?.current && (
          <span className="max-w-md truncate font-mono text-xs text-muted-foreground">
            {progress.current}
          </span>
        )}
      </div>
    );
  }

  if (packages.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center">
        <Package className="mb-4 h-12 w-12 text-muted-foreground" />
        <p className="mb-2 text-lg font-medium">选择项目目录</p>
        <p className="mb-4 text-sm text-muted-foreground">
          扫描 node_modules 中的包并上传到 Verdaccio
        </p>
        {error && (
          <p className="mb-4 max-w-md text-sm text-destructive">{error}</p>
        )}
        <Button variant="outline" onClick={handleSelectDir}>
          <FolderOpen className="mr-2 h-4 w-4" />
          选择目录
        </Button>
      </div>
    );
  }

  const filtering = query.trim().length > 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col pt-3">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <Badge variant="outline" className="max-w-[24rem] truncate">
            {dirPath}
          </Badge>
          <span className="shrink-0 text-sm text-muted-foreground">
            共 {packages.length} 个包，{uncachedCount} 个未缓存
            {filtering && `（已筛选 ${filteredIndices.length}）`}
          </span>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button variant="ghost" size="sm" onClick={selectAllUncached}>
            {filtering ? "全选当前未缓存" : "全选未缓存"}
          </Button>
          <Button variant="ghost" size="sm" onClick={deselectAll}>
            取消全选
          </Button>
          <Button variant="outline" size="sm" onClick={handleSelectDir}>
            重新扫描
          </Button>
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

      <div
        ref={listRef}
        className="min-h-0 flex-1 overflow-auto rounded-md border"
      >
        {filteredIndices.length === 0 ? (
          <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
            未匹配到任何包
          </div>
        ) : (
          <div
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
              position: "relative",
            }}
          >
            {rowVirtualizer.getVirtualItems().map((row) => {
              const idx = filteredIndices[row.index];
              const pkg = packages[idx];
              return (
                <div
                  key={`${pkg.name}@${pkg.version}-${idx}`}
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
                  {!pkg.cached ? (
                    <Checkbox
                      checked={selected.has(idx)}
                      onCheckedChange={() => toggleSelect(idx)}
                    />
                  ) : (
                    <span className="h-4 w-4" />
                  )}
                  <span className="min-w-0 flex-1 truncate font-mono text-sm">
                    {pkg.name}
                  </span>
                  <span className="text-sm text-muted-foreground">
                    {pkg.version}
                  </span>
                  {pkg.cached ? (
                    <Badge
                      variant="outline"
                      className="border-green-300 text-green-600"
                    >
                      已缓存
                    </Badge>
                  ) : (
                    <Badge variant="secondary">未缓存</Badge>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {selected.size > 0 && (
        <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
          <span className="text-sm">
            已选择 <strong>{selected.size}</strong> 个包
          </span>
          <Button onClick={handleUpload}>上传到私服</Button>
        </div>
      )}
    </div>
  );
}

type UploadStatus = "idle" | "Pending" | "Downloading" | "Uploading" | "Success" | "Failed" | "Skipped";

interface TgzFileWithStatus extends LocalPackage {
  uploadStatus: UploadStatus;
  error?: string;
}

function statusBadge(status: UploadStatus, error?: string) {
  switch (status) {
    case "Pending":
      return <Badge variant="secondary">等待中</Badge>;
    case "Downloading":
      return (
        <Badge variant="secondary" className="animate-pulse">
          下载中
        </Badge>
      );
    case "Uploading":
      return (
        <Badge variant="secondary" className="animate-pulse">
          上传中
        </Badge>
      );
    case "Success":
      return (
        <Badge variant="outline" className="border-green-300 text-green-600">
          成功
        </Badge>
      );
    case "Skipped":
      return (
        <Badge variant="outline" className="border-yellow-300 text-yellow-600">
          已存在
        </Badge>
      );
    case "Failed":
      return (
        <Badge variant="outline" className="border-red-300 text-red-600" title={error}>
          失败
        </Badge>
      );
    default:
      return null;
  }
}

function TgzTab() {
  const { startCacheTasks } = useTaskStore();
  const [files, setFiles] = useState<TgzFileWithStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [uploading, setUploading] = useState(false);
  const unlistenRef = useRef<(() => void) | null>(null);
  const dropZoneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!uploading) return;
    const terminal = ["Success", "Failed", "Skipped"];
    const allDone = files.length > 0 && files.every((f) => terminal.includes(f.uploadStatus));
    if (allDone) {
      unlistenRef.current?.();
      unlistenRef.current = null;
      setUploading(false);
    }
  }, [files, uploading]);

  const handleSelectFiles = async () => {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Tarball", extensions: ["tgz"] }],
    });
    if (!selected) return;

    const paths = Array.isArray(selected) ? selected : [selected];
    await parseTgzFiles(paths);
  };

  const parseTgzFiles = async (paths: string[]) => {
    setLoading(true);
    try {
      const parsed: TgzFileWithStatus[] = [];
      for (const p of paths) {
        const pkg = await invoke<LocalPackage>("parse_tgz", { filePath: p });
        parsed.push({ ...pkg, uploadStatus: "idle" });
      }
      setFiles(parsed);
    } catch (e) {
      console.error("解析 tgz 失败:", e);
    } finally {
      setLoading(false);
    }
  };

  const handleUpload = async () => {
    setUploading(true);
    setFiles((prev) =>
      prev.map((f) => ({ ...f, uploadStatus: "Pending" as UploadStatus }))
    );

    const packages = files.map((f) => ({
      package_name: f.name,
      version: f.version,
      tarball_url: `file://${f.path}`,
    }));

    const unlisten = await listen<{
      id: string;
      package_name: string;
      version: string;
      status: UploadStatus;
      error: string | null;
    }>("task-progress", (event) => {
      const { package_name, version, status, error } = event.payload;
      setFiles((prev) =>
        prev.map((f) =>
          f.name === package_name && f.version === version
            ? { ...f, uploadStatus: status, error: error ?? undefined }
            : f
        )
      );
    });

    unlistenRef.current = unlisten;
    await startCacheTasks(packages);
  };

  const { isOver } = useTauriFileDrop({
    zoneRef: dropZoneRef,
    enabled: files.length === 0 && !loading,
    filter: isTgzFile,
    onDrop: async (paths) => {
      if (paths.length > 0) await parseTgzFiles(paths);
    },
  });

  const handleReset = () => {
    setFiles([]);
    setUploading(false);
  };

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Loader2 className="mr-2 h-5 w-5 animate-spin" />
        <span>解析 tarball 中...</span>
      </div>
    );
  }

  if (files.length === 0) {
    return (
      <div
        ref={dropZoneRef}
        className={`flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 transition-colors ${
          isOver ? "border-primary bg-primary/5" : ""
        }`}
      >
        <Upload className="mb-4 h-12 w-12 text-muted-foreground" />
        <p className="mb-2 text-lg font-medium">拖入 .tgz 文件</p>
        <p className="mb-4 text-sm text-muted-foreground">
          或点击选择一个或多个 tarball 文件
        </p>
        <Button variant="outline" onClick={handleSelectFiles}>
          <FolderOpen className="mr-2 h-4 w-4" />
          选择文件
        </Button>
      </div>
    );
  }

  const doneCount = files.filter(
    (f) => f.uploadStatus === "Success" || f.uploadStatus === "Skipped"
  ).length;
  const allDone = uploading && doneCount === files.length;

  return (
    <div className="flex min-h-0 flex-1 flex-col pt-3">
      <div className="mb-3 flex items-center justify-between">
        <span className="text-sm text-muted-foreground">
          已解析 {files.length} 个 tarball
          {uploading && ` · 已完成 ${doneCount}/${files.length}`}
        </span>
        {!uploading && (
          <Button variant="outline" size="sm" onClick={handleSelectFiles}>
            添加更多
          </Button>
        )}
      </div>

      <ScrollArea className="min-h-0 flex-1 rounded-md border">
        <div className="divide-y">
          {files.map((f) => (
            <div
              key={`${f.name}@${f.version}`}
              className="flex items-center gap-3 px-4 py-2"
            >
              <Package className="h-4 w-4 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate font-mono text-sm">
                {f.name}
              </span>
              <span className="text-sm text-muted-foreground">{f.version}</span>
              {statusBadge(f.uploadStatus, f.error)}
            </div>
          ))}
        </div>
      </ScrollArea>

      <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
        <span className="text-sm">
          共 <strong>{files.length}</strong> 个包
        </span>
        <div className="flex gap-2">
          {allDone && (
            <Button variant="outline" onClick={handleReset}>
              重新选择
            </Button>
          )}
          <Button onClick={handleUpload} disabled={uploading}>
            {uploading ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                上传中...
              </>
            ) : (
              "上传到私服"
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
