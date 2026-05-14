import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useTaskStore } from "@/stores/taskStore";
import { useTauriFileDrop } from "@/hooks/useTauriFileDrop";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { FileInput, Loader2, FolderOpen, PackageCheck, ChevronDown, X } from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ExportDropdown } from "@/components/ExportDropdown";

const SUPPORTED_FILES = [
  "package.json",
  "pnpm-lock.yaml",
  "package-lock.json",
];
const isDependencyFile = (path: string) => {
  const name = path.replace(/\\/g, "/").split("/").pop() ?? path;
  return SUPPORTED_FILES.includes(name);
};

interface ParsedDependency {
  name: string;
  version: string;
  tarball_url: string | null;
}

interface CachedStatus {
  name: string;
  version: string;
  cached: boolean;
}

interface DependencyWithStatus extends ParsedDependency {
  cached: boolean | undefined;
}

export function ImportPage() {
  const { startCacheTasks, resolveDependencies } = useTaskStore();

  const [parsedDeps, setParsedDeps] = useState<ParsedDependency[]>([]);
  const [cachedMap, setCachedMap] = useState<Map<string, Set<string>>>(
    new Map()
  );
  const [checking, setChecking] = useState(false);
  const [loading, setLoading] = useState(false);
  const [fileName, setFileName] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const dropZoneRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const deps: DependencyWithStatus[] = useMemo(
    () =>
      parsedDeps.map((dep) => ({
        ...dep,
        cached: checking
          ? undefined
          : (cachedMap.get(dep.name)?.has(dep.version) ?? false),
      })),
    [parsedDeps, cachedMap, checking]
  );

  const rowVirtualizer = useVirtualizer({
    count: deps.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 37,
    overscan: 12,
  });

  const checkCachedStatus = useCallback(async (parsed: ParsedDependency[]) => {
    setChecking(true);
    try {
      const pairs: [string, string][] = parsed.map((d) => [d.name, d.version]);
      const results = await invoke<CachedStatus[]>("check_cached_status", {
        packages: pairs,
      });
      const map = new Map<string, Set<string>>();
      for (const r of results) {
        if (r.cached) {
          if (!map.has(r.name)) map.set(r.name, new Set());
          map.get(r.name)!.add(r.version);
        }
      }
      setCachedMap(map);
    } catch (_) {
      setCachedMap(new Map());
    } finally {
      setChecking(false);
    }
  }, []);

  const handleSelectFile = async () => {
    const file = await open({
      multiple: false,
      filters: [
        {
          name: "依赖文件",
          extensions: ["json", "yaml"],
        },
      ],
    });
    if (!file) return;
    await parseFile(file);
  };

  const parseFile = useCallback(
    async (filePath: string) => {
      setLoading(true);
      setParsedDeps([]);
      setSelected(new Set());
      setCachedMap(new Map());
      setError(null);
      setFileName(filePath.split("/").pop() || filePath);

      try {
        const parsed = await invoke<ParsedDependency[]>("parse_file", {
          filePath,
        });

        if (parsed.length === 0) {
          setError("未解析到任何依赖，请确认文件格式正确");
          setLoading(false);
          return;
        }

        setParsedDeps(parsed);
        checkCachedStatus(parsed);
      } catch (e) {
        setError(`解析失败: ${e}`);
      } finally {
        setLoading(false);
      }
    },
    [checkCachedStatus]
  );

  useEffect(() => {
    if (deps.length === 0 || checking) return;
    const uncached = new Set<number>();
    deps.forEach((d, i) => {
      if (!d.cached) uncached.add(i);
    });
    setSelected(uncached);
  }, [deps, checking]);

  const toggleSelect = (index: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) {
        next.delete(index);
      } else {
        next.add(index);
      }
      return next;
    });
  };

  const selectAllUncached = () => {
    const uncached = new Set<number>();
    deps.forEach((d, i) => {
      if (!d.cached) uncached.add(i);
    });
    setSelected(uncached);
  };

  const deselectAll = () => setSelected(new Set());

  const handleCache = async () => {
    const packages = Array.from(selected).map((i) => ({
      package_name: deps[i].name,
      version: deps[i].version,
      tarball_url: deps[i].tarball_url || undefined,
    }));
    if (packages.length === 0) return;
    await startCacheTasks(packages);
    setSelected(new Set());
  };

  const handleCacheWithDeps = async () => {
    const packages = Array.from(selected).map((i) => ({
      package_name: deps[i].name,
      version: deps[i].version,
    }));
    if (packages.length === 0) return;
    setResolving(true);
    try {
      const resolved = await resolveDependencies(packages);
      await startCacheTasks(
        resolved.map((r) => ({ package_name: r.package_name, version: r.version }))
      );
      setSelected(new Set());
    } catch (e) {
      console.error("依赖解析失败:", e);
    } finally {
      setResolving(false);
    }
  };

  const uncachedCount = deps.filter((d) => d.cached === false).length;

  const { isOver: dropIsOver } = useTauriFileDrop({
    zoneRef: dropZoneRef,
    enabled: deps.length === 0 && !loading,
    filter: isDependencyFile,
    onDrop: async (paths) => {
      if (paths[0]) await parseFile(paths[0]);
    },
  });

  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-4 text-2xl font-bold">导入</h1>

      {deps.length === 0 && !loading && (
        <div
          ref={dropZoneRef}
          className={`flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center transition-colors ${
            dropIsOver ? "border-primary bg-primary/5" : ""
          }`}
        >
          <FileInput className="mb-4 h-12 w-12 text-muted-foreground" />
          <p className="mb-2 text-lg font-medium">
            拖入文件或点击选择
          </p>
          <p className="mb-4 text-sm text-muted-foreground">
            支持 package.json、pnpm-lock.yaml、package-lock.json
          </p>
          {error && (
            <p className="mb-4 text-sm text-destructive">{error}</p>
          )}
          <Button variant="outline" onClick={handleSelectFile}>
            <FolderOpen className="mr-2 h-4 w-4" />
            选择文件
          </Button>
        </div>
      )}

      {loading && (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="mr-2 h-5 w-5 animate-spin" />
          <span>解析中...</span>
        </div>
      )}

      {deps.length > 0 && !loading && (
        <>
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <Badge variant="outline">{fileName}</Badge>
              <span className="text-sm text-muted-foreground">
                共 {deps.length} 个依赖，{uncachedCount} 个未缓存
              </span>
            </div>
            <div className="flex gap-2">
              <Button variant="ghost" size="sm" onClick={selectAllUncached}>
                全选未缓存
              </Button>
              <Button variant="ghost" size="sm" onClick={deselectAll}>
                取消全选
              </Button>
              <Button variant="outline" size="sm" onClick={handleSelectFile}>
                重新选择
              </Button>
            </div>
          </div>

          <div
            ref={listRef}
            className="min-h-0 flex-1 overflow-auto rounded-md border"
          >
            <div
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
                position: "relative",
              }}
            >
              {rowVirtualizer.getVirtualItems().map((row) => {
                const i = row.index;
                const dep = deps[i];
                return (
                  <div
                    key={`${dep.name}@${dep.version}-${i}`}
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
                    {!dep.cached && dep.cached !== undefined ? (
                      <Checkbox
                        checked={selected.has(i)}
                        onCheckedChange={() => toggleSelect(i)}
                      />
                    ) : (
                      <span className="h-4 w-4" />
                    )}
                    <span className="min-w-0 flex-1 truncate font-mono text-sm">
                      {dep.name}
                    </span>
                    <span className="text-sm text-muted-foreground">
                      {dep.version}
                    </span>
                    {dep.cached === undefined ? (
                      <Badge variant="secondary" className="animate-pulse">
                        检查中
                      </Badge>
                    ) : dep.cached ? (
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
          </div>

          {selected.size > 0 && (
            <div className="-mx-6 -mb-6 mt-4 flex items-center justify-between border-t bg-background px-6 py-3">
              <Popover>
                <PopoverTrigger className="flex items-center gap-2 rounded-md px-2 py-1 text-sm transition-colors hover:bg-muted">
                  <PackageCheck className="h-4 w-4" />
                  已选择 <strong>{selected.size}</strong> 个包
                  <ChevronDown className="h-3 w-3 text-muted-foreground" />
                </PopoverTrigger>
                <PopoverContent
                  align="start"
                  side="top"
                  className="w-80 p-0"
                >
                  <div className="flex items-center justify-between border-b px-3 py-2">
                    <span className="text-sm font-medium">
                      已选择 {selected.size} 个包
                    </span>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 px-2 text-xs text-muted-foreground"
                      onClick={() => setSelected(new Set())}
                    >
                      清空
                    </Button>
                  </div>
                  <div className="max-h-64 overflow-y-auto p-2">
                    {Array.from(selected).map((i) => (
                      <div
                        key={i}
                        className="group flex items-center justify-between rounded-md px-2 py-1 text-sm hover:bg-muted"
                      >
                        <span className="min-w-0 flex-1 truncate">
                          {deps[i].name}@{deps[i].version}
                        </span>
                        <X
                          className="h-3 w-3 shrink-0 cursor-pointer text-muted-foreground opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
                          onClick={() =>
                            setSelected((prev) => {
                              const next = new Set(prev);
                              next.delete(i);
                              return next;
                            })
                          }
                        />
                      </div>
                    ))}
                  </div>
                </PopoverContent>
              </Popover>
              <div className="flex gap-2">
                <ExportDropdown
                  getSelectedPackages={() =>
                    Array.from(selected).map((i) => ({
                      package_name: deps[i].name,
                      version: deps[i].version,
                    }))
                  }
                />
                <Button
                  variant="outline"
                  onClick={handleCacheWithDeps}
                  disabled={resolving}
                >
                  {resolving ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      解析依赖中...
                    </>
                  ) : (
                    "缓存包及依赖"
                  )}
                </Button>
                <Button onClick={handleCache}>缓存到私服</Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
