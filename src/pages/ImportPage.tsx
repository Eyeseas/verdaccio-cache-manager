import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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
import {
  applyResolveProgress,
  applyResolvedPackages,
  applyTaskProgress,
  areSelectionsEqual,
  cacheTaskInputsFromDependencies,
  cacheTaskInputsFromResolved,
  createResolveRequestId,
  dependencyRootsFromResolved,
  getRowState,
  isCurrentResolveRequest,
  isSelectableState,
  pruneSelection,
  removeResolvedFromSelection,
  rowKey,
  shouldShowActionBar,
  type ParsedDependency,
  type ResolveProgressPayload,
  type ResolvedImportPackage,
  type RowState,
} from "./importPageLogic";

const SUPPORTED_FILES = [
  "package.json",
  "pnpm-lock.yaml",
  "package-lock.json",
];
const isDependencyFile = (path: string) => {
  const name = path.replace(/\\/g, "/").split("/").pop() ?? path;
  return SUPPORTED_FILES.includes(name);
};

interface CachedStatus {
  name: string;
  version: string;
  cached: boolean;
}

type TaskStatusEnum =
  | "Pending"
  | "Downloading"
  | "Uploading"
  | "Success"
  | "Failed"
  | "Skipped";

interface TaskProgressPayload {
  id: string;
  package_name: string;
  version: string;
  status: TaskStatusEnum;
  error: string | null;
}

export function ImportPage() {
  const { startCacheTasks, resolveDependencies } = useTaskStore();

  const [parsedDeps, setParsedDeps] = useState<ParsedDependency[]>([]);
  const [rowStates, setRowStates] = useState<Map<string, RowState>>(new Map());
  const [checking, setChecking] = useState(false);
  const [loading, setLoading] = useState(false);
  const [fileName, setFileName] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [resolving, setResolving] = useState(false);
  const [caching, setCaching] = useState(false);
  const [exporting, setExporting] = useState(false);
  const dropZoneRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const pendingInitSelectionRef = useRef(false);
  const resolveRequestIdRef = useRef<string | null>(null);

  const rowVirtualizer = useVirtualizer({
    count: parsedDeps.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 37,
    overscan: 12,
  });

  useEffect(() => {
    let stopped = false;
    const handles: UnlistenFn[] = [];

    listen<ResolveProgressPayload>("import-resolve-progress", (event) => {
      if (stopped) return;
      setRowStates((prev) => {
        const next = applyResolveProgress(prev, {
          currentRequestId: resolveRequestIdRef.current,
          payload: event.payload,
        });
        return next;
      });
    }).then((un) => handles.push(un));

    listen<TaskProgressPayload>("task-progress", (event) => {
      if (stopped) return;
      const p = event.payload;
      setRowStates((prev) => {
        const next = applyTaskProgress(prev, p);
        return next;
      });
    }).then((un) => handles.push(un));

    return () => {
      stopped = true;
      handles.forEach((fn) => fn());
    };
  }, []);

  useEffect(() => {
    setSelected((cur) => {
      const next = pruneSelection(cur, parsedDeps, rowStates);
      return areSelectionsEqual(cur, next) ? cur : next;
    });
  }, [parsedDeps, rowStates]);

  const checkCachedStatus = useCallback(async (parsed: ParsedDependency[]) => {
    setChecking(true);
    try {
      const pairs: [string, string][] = parsed.map((d) => [d.name, d.version]);
      const results = await invoke<CachedStatus[]>("check_cached_status", {
        packages: pairs,
      });
      setRowStates((prev) => {
        const next = new Map(prev);
        for (const r of results) {
          const key = rowKey(r.name, r.version);
          const cur = next.get(key) ?? { status: "unknown" };
          next.set(key, { ...cur, status: r.cached ? "cached" : "uncached" });
        }
        return next;
      });
    } catch (_) {
      // ignore
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
      setRowStates(new Map());
      setError(null);
      resolveRequestIdRef.current = null;
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

        const init = new Map<string, RowState>();
        for (const dep of parsed) {
          init.set(rowKey(dep.name, dep.version), { status: "unknown" });
        }
        setRowStates(init);
        pendingInitSelectionRef.current = true;
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
    if (
      !pendingInitSelectionRef.current ||
      parsedDeps.length === 0 ||
      checking
    )
      return;
    const sel = new Set<number>();
    parsedDeps.forEach((d, i) => {
      const s = rowStates.get(rowKey(d.name, d.version))?.status;
      if (s !== "cached") sel.add(i);
    });
    setSelected(sel);
    pendingInitSelectionRef.current = false;
  }, [parsedDeps, rowStates, checking]);

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
    const sel = new Set<number>();
    parsedDeps.forEach((d, i) => {
      const s = rowStates.get(rowKey(d.name, d.version))?.status;
      if (isSelectableState(s ?? "unknown")) sel.add(i);
    });
    setSelected(sel);
  };

  const deselectAll = () => setSelected(new Set());

  const markRowsResolving = (indices: number[]) => {
    setRowStates((prev) => {
      const next = new Map(prev);
      for (const i of indices) {
        const dep = parsedDeps[i];
        if (!dep) continue;
        const key = rowKey(dep.name, dep.version);
        const cur = next.get(key) ?? { status: "unknown" };
        next.set(key, { ...cur, status: "resolving", error: undefined });
      }
      return next;
    });
  };

  const handleCache = async () => {
    if (selected.size === 0) return;
    const indices = Array.from(selected);
    const inputs = indices.map((i) => ({
      package_name: parsedDeps[i].name,
      version: parsedDeps[i].version,
      tarball_url: parsedDeps[i].tarball_url || undefined,
    }));
    if (inputs.length === 0) return;

    markRowsResolving(indices);
    const requestId = createResolveRequestId();
    resolveRequestIdRef.current = requestId;
    setCaching(true);
    try {
      const resolved = await invoke<ResolvedImportPackage[]>(
        "resolve_package_versions",
        { packages: inputs, requestId }
      );
      if (!isCurrentResolveRequest(resolveRequestIdRef.current, requestId)) {
        return;
      }
      setRowStates((prev) => applyResolvedPackages(prev, resolved));
      const tasks = cacheTaskInputsFromResolved(resolved);
      setSelected((cur) => removeResolvedFromSelection(cur, parsedDeps, resolved));
      if (tasks.length > 0) {
        await startCacheTasks(tasks);
      }
    } catch (e) {
      console.error("版本解析失败:", e);
    } finally {
      setCaching(false);
    }
  };

  const handleCacheWithDeps = async () => {
    if (selected.size === 0) return;
    const indices = Array.from(selected);
    const inputs = indices.map((i) => ({
      package_name: parsedDeps[i].name,
      version: parsedDeps[i].version,
      tarball_url: parsedDeps[i].tarball_url || undefined,
    }));
    if (inputs.length === 0) return;

    markRowsResolving(indices);
    const requestId = createResolveRequestId();
    resolveRequestIdRef.current = requestId;
    setResolving(true);
    try {
      const directlyResolved = await invoke<ResolvedImportPackage[]>(
        "resolve_package_versions",
        { packages: inputs, requestId }
      );
      if (!isCurrentResolveRequest(resolveRequestIdRef.current, requestId)) {
        return;
      }
      setRowStates((prev) => applyResolvedPackages(prev, directlyResolved));
      const roots = dependencyRootsFromResolved(directlyResolved);
      setSelected((cur) =>
        removeResolvedFromSelection(cur, parsedDeps, directlyResolved)
      );
      if (roots.length === 0) return;
      const resolved = await resolveDependencies(roots);
      if (!isCurrentResolveRequest(resolveRequestIdRef.current, requestId)) {
        return;
      }
      const dependencyStatuses = await invoke<CachedStatus[]>("check_cached_status", {
        packages: resolved.map((r) => [r.package_name, r.version]),
      });
      if (!isCurrentResolveRequest(resolveRequestIdRef.current, requestId)) {
        return;
      }
      const tasks = cacheTaskInputsFromDependencies(resolved, dependencyStatuses);
      if (tasks.length > 0) {
        await startCacheTasks(tasks);
      }
    } catch (e) {
      console.error("依赖解析失败:", e);
    } finally {
      setResolving(false);
    }
  };

  const getState = (dep: ParsedDependency): RowState =>
    getRowState(rowStates, dep);

  const uncachedCount = parsedDeps.filter(
    (d) => getState(d).status !== "cached"
  ).length;
  const showActionBar = shouldShowActionBar({
    selectedSize: selected.size,
    resolving,
    caching,
    exporting,
  });

  const renderBadge = (state: RowState) => {
    switch (state.status) {
      case "unknown":
        return (
          <Badge variant="secondary" className="animate-pulse">
            检查中
          </Badge>
        );
      case "cached":
        return (
          <Badge
            variant="outline"
            className="border-green-300 text-green-600"
          >
            已缓存
          </Badge>
        );
      case "uncached":
        return <Badge variant="secondary">未缓存</Badge>;
      case "resolving":
        return (
          <Badge variant="secondary" className="animate-pulse">
            解析中
          </Badge>
        );
      case "resolve-failed":
        return (
          <Badge
            variant="outline"
            className="border-red-300 text-red-600"
            title={state.error}
          >
            解析失败
          </Badge>
        );
      case "downloading":
        return (
          <Badge variant="secondary" className="animate-pulse">
            下载中
          </Badge>
        );
      case "uploading":
        return (
          <Badge variant="secondary" className="animate-pulse">
            上传中
          </Badge>
        );
      case "failed":
        return (
          <Badge
            variant="outline"
            className="border-red-300 text-red-600"
            title={state.error}
          >
            失败
          </Badge>
        );
    }
  };

  const renderVersion = (dep: ParsedDependency, state: RowState) => {
    if (state.resolvedVersion && state.resolvedVersion !== dep.version) {
      return (
        <span className="text-sm text-muted-foreground">
          <span>{dep.version}</span>
          <span className="mx-1 opacity-60">→</span>
          <span className="text-foreground/80">{state.resolvedVersion}</span>
        </span>
      );
    }
    return (
      <span className="text-sm text-muted-foreground">{dep.version}</span>
    );
  };

  const { isOver: dropIsOver } = useTauriFileDrop({
    zoneRef: dropZoneRef,
    enabled: parsedDeps.length === 0 && !loading,
    filter: isDependencyFile,
    onDrop: async (paths) => {
      if (paths[0]) await parseFile(paths[0]);
    },
  });

  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-4 text-2xl font-bold">导入</h1>

      {parsedDeps.length === 0 && !loading && (
        <div
          ref={dropZoneRef}
          className={`flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center transition-colors ${
            dropIsOver ? "border-primary bg-primary/5" : ""
          }`}
        >
          <FileInput className="mb-4 h-12 w-12 text-muted-foreground" />
          <p className="mb-2 text-lg font-medium">拖入文件或点击选择</p>
          <p className="mb-4 text-sm text-muted-foreground">
            支持 package.json、pnpm-lock.yaml、package-lock.json
          </p>
          {error && <p className="mb-4 text-sm text-destructive">{error}</p>}
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

      {parsedDeps.length > 0 && !loading && (
        <>
          <div className="mb-3 flex items-center justify-between">
            <div className="flex items-center gap-3">
              <Badge variant="outline">{fileName}</Badge>
              <span className="text-sm text-muted-foreground">
                共 {parsedDeps.length} 个依赖，{uncachedCount} 个未缓存
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
                const dep = parsedDeps[i];
                const state = getState(dep);
                const selectable = isSelectableState(state.status);
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
                    {selectable ? (
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
                    {renderVersion(dep, state)}
                    {renderBadge(state)}
                  </div>
                );
              })}
            </div>
          </div>

          {showActionBar && (
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
                          {parsedDeps[i].name}@{parsedDeps[i].version}
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
                  getSelectedPackages={async () => {
                    const indices = Array.from(selected);
                    const result: { package_name: string; version: string }[] = [];
                    const unresolved: { dep: ParsedDependency; index: number }[] = [];

                    for (const i of indices) {
                      const dep = parsedDeps[i];
                      if (!dep) continue;
                      const state = getState(dep);
                      if (state.resolvedVersion) {
                        result.push({
                          package_name: dep.name,
                          version: state.resolvedVersion,
                        });
                      } else {
                        unresolved.push({ dep, index: i });
                      }
                    }

                    if (unresolved.length === 0) return result;

                    const inputs = unresolved.map(({ dep }) => ({
                      package_name: dep.name,
                      version: dep.version,
                      tarball_url: dep.tarball_url || undefined,
                    }));
                    markRowsResolving(unresolved.map(({ index }) => index));
                    const requestId = createResolveRequestId();
                    resolveRequestIdRef.current = requestId;

                    const resolved = await invoke<ResolvedImportPackage[]>(
                      "resolve_package_versions",
                      { packages: inputs, requestId }
                    );
                    setRowStates((prev) => applyResolvedPackages(prev, resolved));

                    const resolvedMap = new Map<string, string>();
                    for (const r of resolved) {
                      resolvedMap.set(rowKey(r.name, r.raw_range), r.version);
                    }
                    for (const { dep } of unresolved) {
                      const v = resolvedMap.get(rowKey(dep.name, dep.version));
                      if (v) {
                        result.push({ package_name: dep.name, version: v });
                      }
                    }

                    if (result.length === 0) {
                      throw new Error("选中的包均无法解析到具体版本");
                    }
                    return result;
                  }}
                  disabled={resolving || caching}
                  onExportingChange={setExporting}
                />
                <Button
                  variant="outline"
                  onClick={handleCacheWithDeps}
                  disabled={resolving || caching || exporting}
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
                <Button
                  onClick={handleCache}
                  disabled={resolving || caching || exporting}
                >
                  {caching ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      处理中...
                    </>
                  ) : (
                    "缓存到私服"
                  )}
                </Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
