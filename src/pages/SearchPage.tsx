import {
  useState,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  type MouseEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useConfigStore } from "@/stores/configStore";
import { useTaskStore } from "@/stores/taskStore";
import { useCacheStore, type CachedPackage } from "@/stores/cacheStore";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Search,
  Loader2,
  ChevronDown,
  ChevronRight,
  Check,
  PackageCheck,
  X,
  Copy,
} from "lucide-react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ExportDropdown } from "@/components/ExportDropdown";
import { toast } from "sonner";
import {
  RowContextMenu,
  type ContextMenuPosition,
} from "@/components/RowContextMenu";
import {
  UnpublishConfirmDialog,
  type ManageTarget,
} from "@/components/UnpublishConfirmDialog";
import { DeprecateConfirmDialog } from "@/components/DeprecateConfirmDialog";

type SearchResult = CachedPackage;

interface ContextMenuState {
  position: ContextMenuPosition;
  pkg: string;
  version?: string;
  cached: boolean;
}

interface ExpandedPackage {
  name: string;
  versions: string[];
  loading: boolean;
}

type RegistrySource = "npmjs" | "verdaccio";

function isStableVersion(v: string): boolean {
  return /^\d+\.\d+\.\d+$/.test(v);
}

function formatRelativeTime(ts: number): string {
  const diffSec = Math.floor((Date.now() - ts) / 1000);
  if (diffSec < 60) return `${diffSec} 秒前`;
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)} 分钟前`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} 小时前`;
  return new Date(ts).toLocaleString();
}

interface CopyButtonProps {
  text: string;
  title: string;
  className?: string;
}

function CopyButton({ text, title, className }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      toast.error("复制失败");
    }
  };

  return (
    <button
      type="button"
      title={title}
      onClick={handleCopy}
      className={
        className ??
        "shrink-0 rounded p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-muted hover:text-foreground group-hover:opacity-100"
      }
    >
      {copied ? (
        <Check className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </button>
  );
}

export function SearchPage() {
  const { config } = useConfigStore();
  const { startCacheTasks, resolveDependencies } = useTaskStore();
  const listRef = useRef<HTMLDivElement>(null);

  const {
    cachedAll,
    cachedError,
    loading: cacheLoading,
    lastLoadedAt,
    loadCachedPackages: loadCachedFromStore,
  } = useCacheStore();
  const [query, setQuery] = useState("");
  const [source, setSource] = useState<RegistrySource>("npmjs");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, ExpandedPackage>>({});
  const [selected, setSelected] = useState<Map<string, Set<string>>>(
    new Map()
  );
  const [stableOnly, setStableOnly] = useState(true);
  const [resolving, setResolving] = useState(false);
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(
    null
  );
  const [unpublishTargets, setUnpublishTargets] = useState<
    ManageTarget[] | null
  >(null);
  const [deprecateTargets, setDeprecateTargets] = useState<
    ManageTarget[] | null
  >(null);
  const [manageLoading, setManageLoading] = useState(false);

  const isVerdaccio = source === "verdaccio";

  const registryUrl =
    source === "npmjs" ? "https://registry.npmjs.org" : config.registry_url;

  useEffect(() => {
    if (source === "npmjs") setResults([]);
    if (source === "verdaccio") loadCachedFromStore();
    setExpanded({});
    // 切换数据源必须清空选择，否则 npmjs 选中的版本会被
    // 带入 Verdaccio 的批量 Unpublish/Deprecate，对错误目标执行破坏性操作
    setSelected(new Map());
  }, [source, loadCachedFromStore]);

  const filteredCached = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return cachedAll;

    const scored: { pkg: SearchResult; score: number }[] = [];
    for (const pkg of cachedAll) {
      const name = pkg.name.toLowerCase();
      const idx = name.indexOf(q);
      if (idx < 0) continue;
      const bare = name.startsWith("@") ? name.split("/")[1] ?? name : name;
      let score: number;
      if (name === q) score = 0;
      else if (bare === q) score = 1;
      else if (idx === 0) score = 2;
      else if (bare.startsWith(q)) score = 3;
      else score = 100 + idx;
      scored.push({ pkg, score });
    }

    scored.sort((a, b) => {
      if (a.score !== b.score) return a.score - b.score;
      if (a.pkg.name.length !== b.pkg.name.length) {
        return a.pkg.name.length - b.pkg.name.length;
      }
      return a.pkg.name.localeCompare(b.pkg.name);
    });

    return scored.map((s) => s.pkg);
  }, [cachedAll, query]);

  const displayResults = source === "verdaccio" ? filteredCached : results;

  const rowVirtualizer = useVirtualizer({
    count: displayResults.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 76,
    getItemKey: (index) => displayResults[index]?.name ?? index,
    overscan: 10,
  });

  useEffect(() => {
    listRef.current?.scrollTo({ top: 0 });
  }, [source, query, results, cachedAll]);

  useLayoutEffect(() => {
    const measureVisibleRows = () => {
      listRef.current
        ?.querySelectorAll<HTMLElement>("[data-virtual-row]")
        .forEach((node) => rowVirtualizer.measureElement(node));
    };

    measureVisibleRows();
    const frame = window.requestAnimationFrame(measureVisibleRows);
    return () => window.cancelAnimationFrame(frame);
  }, [expanded, stableOnly, displayResults.length, rowVirtualizer]);

  const handleSearch = useCallback(async () => {
    if (source === "verdaccio") return;
    if (!query.trim()) return;
    setSearching(true);
    setResults([]);
    try {
      const res = await invoke<SearchResult[]>("search_packages", {
        registryUrl,
        query: query.trim(),
      });
      setResults(res);
    } catch (e) {
      console.error("搜索失败:", e);
    } finally {
      setSearching(false);
    }
  }, [query, registryUrl, source]);

  const cachedVersionsByName = useMemo(() => {
    const map = new Map<string, Set<string>>();
    cachedAll.forEach((p) =>
      map.set(p.name, new Set(p.cached_versions))
    );
    return map;
  }, [cachedAll]);

  const localPackageByName = useMemo(() => {
    const map = new Map<string, SearchResult>();
    cachedAll.forEach((p) => map.set(p.name, p));
    return map;
  }, [cachedAll]);

  const toggleExpand = async (name: string) => {
    if (expanded[name]) {
      const next = { ...expanded };
      delete next[name];
      setExpanded(next);
      return;
    }

    // Only use local data on verdaccio tab
    if (source === "verdaccio") {
      const local = localPackageByName.get(name);
      if (local && local.versions.length > 0) {
        setExpanded((prev) => ({
          ...prev,
          [name]: { name, versions: local.versions, loading: false },
        }));
        return;
      }
    }

    setExpanded((prev) => ({
      ...prev,
      [name]: { name, versions: [], loading: true },
    }));

    try {
      const versions = await invoke<string[]>("get_package_versions", {
        registryUrl: "https://registry.npmjs.org",
        packageName: name,
      });

      setExpanded((prev) => ({
        ...prev,
        [name]: { name, versions, loading: false },
      }));
    } catch {
      const fallback = localPackageByName.get(name)?.cached_versions ?? [];
      setExpanded((prev) => ({
        ...prev,
        [name]: { name, versions: fallback, loading: false },
      }));
    }
  };

  const toggleVersion = (pkgName: string, version: string) => {
    setSelected((prev) => {
      const next = new Map(prev);
      const versions = new Set(next.get(pkgName) || []);
      if (versions.has(version)) {
        versions.delete(version);
      } else {
        versions.add(version);
      }
      if (versions.size === 0) {
        next.delete(pkgName);
      } else {
        next.set(pkgName, versions);
      }
      return next;
    });
  };

  const totalSelected = Array.from(selected.values()).reduce(
    (sum, s) => sum + s.size,
    0
  );

  const handleCache = async () => {
    const packages: { package_name: string; version: string }[] = [];
    selected.forEach((versions, pkgName) => {
      versions.forEach((v) => {
        packages.push({ package_name: pkgName, version: v });
      });
    });
    if (packages.length === 0) return;
    await startCacheTasks(packages);
    setSelected(new Map());
  };

  const handleCacheWithDeps = async () => {
    const packages: { package_name: string; version: string }[] = [];
    selected.forEach((versions, pkgName) => {
      versions.forEach((v) => {
        packages.push({ package_name: pkgName, version: v });
      });
    });
    if (packages.length === 0) return;
    setResolving(true);
    try {
      const resolved = await resolveDependencies(packages);
      await startCacheTasks(
        resolved.map((r) => ({ package_name: r.package_name, version: r.version }))
      );
      setSelected(new Map());
    } catch (e) {
      console.error("依赖解析失败:", e);
    } finally {
      setResolving(false);
    }
  };

  const collectSelectedTargets = (): ManageTarget[] => {
    const targets: ManageTarget[] = [];
    selected.forEach((versions, pkgName) => {
      const cached = cachedVersionsByName.get(pkgName);
      versions.forEach((v) => {
        // 仅对当前 Verdaccio 索引中确实缓存的版本执行破坏性操作，
        // 防止跨数据源残留 selection 命中错误目标
        if (cached?.has(v)) targets.push({ name: pkgName, version: v });
      });
    });
    return targets;
  };

  const runUnpublish = async (targets: ManageTarget[]) => {
    setManageLoading(true);
    const failures: string[] = [];
    let ok = 0;
    for (const t of targets) {
      try {
        await invoke("unpublish_package", {
          name: t.name,
          version: t.version ?? null,
        });
        ok += 1;
      } catch (e) {
        failures.push(
          `${t.version ? `${t.name}@${t.version}` : t.name}: ${e}`
        );
      }
    }
    setManageLoading(false);
    setUnpublishTargets(null);
    if (ok > 0) {
      toast.success(`已 unpublish ${ok} 项`);
      setSelected(new Map());
      const affected = new Set(targets.map((t) => t.name));
      setExpanded((prev) => {
        const next = { ...prev };
        affected.forEach((n) => delete next[n]);
        return next;
      });
      await loadCachedFromStore();
    }
    if (failures.length > 0) {
      toast.error(`${failures.length} 项失败`, {
        description: failures.slice(0, 5).join("\n"),
      });
    }
  };

  const runDeprecate = async (targets: ManageTarget[], message: string) => {
    setManageLoading(true);
    const failures: string[] = [];
    let ok = 0;
    for (const t of targets) {
      if (!t.version) continue;
      try {
        await invoke("deprecate_package", {
          name: t.name,
          version: t.version,
          message,
        });
        ok += 1;
      } catch (e) {
        failures.push(`${t.name}@${t.version}: ${e}`);
      }
    }
    setManageLoading(false);
    setDeprecateTargets(null);
    if (ok > 0) {
      toast.success(`已 deprecate ${ok} 个版本`);
      setSelected(new Map());
      await loadCachedFromStore();
    }
    if (failures.length > 0) {
      toast.error(`${failures.length} 项失败`, {
        description: failures.slice(0, 5).join("\n"),
      });
    }
  };

  const getFilteredVersions = (versions: string[]) => {
    const filtered = stableOnly
      ? versions.filter(isStableVersion)
      : versions;
    return [...filtered].sort((a, b) => compareVersions(b, a));
  };

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 space-y-3">
        <h1 className="text-2xl font-bold">搜索与缓存</h1>

        <div className="flex gap-2">
          <Tabs
            value={source}
            onValueChange={(val) => setSource(val as RegistrySource)}
          >
            <TabsList>
              <TabsTrigger value="npmjs">npmjs</TabsTrigger>
              <TabsTrigger value="verdaccio">Verdaccio</TabsTrigger>
            </TabsList>
          </Tabs>

          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="pl-9"
              placeholder={
                source === "verdaccio"
                  ? "在已缓存的包中过滤..."
                  : "搜索 npm 包..."
              }
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            />
          </div>

          {source === "npmjs" && (
            <Button onClick={handleSearch} disabled={searching}>
              {searching ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                "搜索"
              )}
            </Button>
          )}
        </div>

        <div className="flex items-center justify-between">
          <label className="flex items-center gap-2 text-sm">
            <Checkbox
              checked={stableOnly}
              onCheckedChange={(v) => setStableOnly(v === true)}
            />
            <span>仅显示正式版本</span>
          </label>

          {lastLoadedAt && (
            <span className="text-xs text-muted-foreground">
              已缓存来源：本地索引
              · {cachedAll.length} 个包 · 更新于{" "}
              {formatRelativeTime(lastLoadedAt)}
            </span>
          )}
        </div>
      </div>

      <div ref={listRef} className="min-h-0 flex-1 overflow-auto">
        {displayResults.length > 0 ? (
          <div
            className="relative"
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
            }}
          >
            {rowVirtualizer.getVirtualItems().map((row) => {
              const pkg = displayResults[row.index];
              if (!pkg) return null;

              const expandedPackage = expanded[pkg.name];
              const filteredVersions = expandedPackage
                ? getFilteredVersions(expandedPackage.versions)
                : [];

              return (
                <div
                  key={row.key}
                  data-index={row.index}
                  data-virtual-row
                  ref={rowVirtualizer.measureElement}
                  className="absolute left-0 top-0 w-full pb-1"
                  style={{
                    transform: `translateY(${row.start}px)`,
                  }}
                >
                  <div className="rounded-lg border">
                    <div
                      className="group flex cursor-pointer items-center gap-3 p-3 hover:bg-muted/50"
                      onClick={() => toggleExpand(pkg.name)}
                      onContextMenu={
                        isVerdaccio
                          ? (e) => {
                              e.preventDefault();
                              setContextMenu({
                                position: { x: e.clientX, y: e.clientY },
                                pkg: pkg.name,
                                cached: false,
                              });
                            }
                          : undefined
                      }
                    >
                      {expandedPackage ? (
                        <ChevronDown className="h-4 w-4 shrink-0" />
                      ) : (
                        <ChevronRight className="h-4 w-4 shrink-0" />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{pkg.name}</span>
                          <CopyButton text={pkg.name} title="复制包名" />
                          {pkg.latest_version && (
                            <Badge variant="secondary">
                              {pkg.latest_version}
                            </Badge>
                          )}
                        </div>
                        {pkg.description && (
                          <p className="truncate text-sm text-muted-foreground">
                            {pkg.description}
                          </p>
                        )}
                      </div>
                    </div>

                    {expandedPackage && (
                      <div className="border-t px-3 py-2">
                        {expandedPackage.loading ? (
                          <div className="flex items-center gap-2 py-2 text-sm text-muted-foreground">
                            <Loader2 className="h-3 w-3 animate-spin" />
                            加载版本列表...
                          </div>
                        ) : (
                          <div className="max-h-48 overflow-y-auto">
                            <div className="flex flex-wrap gap-2 py-1">
                              {filteredVersions.map((v) => {
                                const isCached =
                                  cachedVersionsByName
                                    .get(pkg.name)
                                    ?.has(v) ?? false;
                                const isSelected =
                                  selected.get(pkg.name)?.has(v) || false;

                                const openVersionMenu = (
                                  e: MouseEvent
                                ) => {
                                  e.preventDefault();
                                  setContextMenu({
                                    position: {
                                      x: e.clientX,
                                      y: e.clientY,
                                    },
                                    pkg: pkg.name,
                                    version: v,
                                    cached: isCached,
                                  });
                                };

                                const copyChipBtnClass =
                                  "-mr-1 ml-0.5 shrink-0 rounded p-0.5 text-current opacity-0 hover:bg-emerald-500/20 hover:opacity-100 group-hover/v:opacity-70";

                                if (isCached && !isVerdaccio) {
                                  return (
                                    <span
                                      key={v}
                                      title="已缓存到 Verdaccio"
                                      className="group/v flex items-center gap-1.5 rounded-md border border-emerald-500/40 bg-emerald-500/10 px-2 py-1 text-sm text-emerald-700 dark:text-emerald-400"
                                    >
                                      <Check className="h-3.5 w-3.5" />
                                      <span>{v}</span>
                                      <CopyButton
                                        text={`${pkg.name}@${v}`}
                                        title="复制 包名@版本号"
                                        className={copyChipBtnClass}
                                      />
                                    </span>
                                  );
                                }

                                if (isCached) {
                                  return (
                                    <label
                                      key={v}
                                      title="已缓存到 Verdaccio · 右键可管理"
                                      onContextMenu={openVersionMenu}
                                      className={`group/v flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 text-sm transition-colors ${
                                        isSelected
                                          ? "border-emerald-500 bg-emerald-500/20"
                                          : "border-emerald-500/40 bg-emerald-500/10 hover:bg-emerald-500/20"
                                      } text-emerald-700 dark:text-emerald-400`}
                                    >
                                      <Checkbox
                                        checked={isSelected}
                                        onCheckedChange={() =>
                                          toggleVersion(pkg.name, v)
                                        }
                                        className="h-3.5 w-3.5"
                                      />
                                      <Check className="h-3.5 w-3.5" />
                                      <span>{v}</span>
                                      <CopyButton
                                        text={`${pkg.name}@${v}`}
                                        title="复制 包名@版本号"
                                        className={copyChipBtnClass}
                                      />
                                    </label>
                                  );
                                }

                                return (
                                  <label
                                    key={v}
                                    onContextMenu={
                                      isVerdaccio
                                        ? openVersionMenu
                                        : undefined
                                    }
                                    className={`flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 text-sm transition-colors ${
                                      isSelected
                                        ? "border-primary bg-primary/5"
                                        : "hover:bg-muted"
                                    }`}
                                  >
                                    <Checkbox
                                      checked={isSelected}
                                      onCheckedChange={() =>
                                        toggleVersion(pkg.name, v)
                                      }
                                      className="h-3.5 w-3.5"
                                    />
                                    <span>{v}</span>
                                  </label>
                                );
                              })}
                              {filteredVersions.length === 0 && (
                                <span className="text-sm text-muted-foreground">
                                  {stableOnly ? "无正式版本" : "无版本信息"}
                                </span>
                              )}
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          !searching &&
          !cacheLoading && (
            <div className="py-8 text-center text-muted-foreground">
              {source === "verdaccio" ? (
                lastLoadedAt === null ? (
                  <p className="text-sm">索引为空，请在设置中配置源地址并同步</p>
                ) : cachedError ? (
                  <p className="text-sm">{cachedError}</p>
                ) : cachedAll.length === 0 ? (
                  <p>Verdaccio 暂无已缓存的包</p>
                ) : (
                  <p>未找到匹配的包</p>
                )
              ) : query ? (
                <p>未找到匹配的包</p>
              ) : null}
            </div>
          )
        )}
      </div>

      {totalSelected > 0 && (
        <div className="-mx-6 -mb-6 mt-4 flex items-center justify-between border-t bg-background px-6 py-3">
          <Popover>
            <PopoverTrigger className="flex items-center gap-2 rounded-md px-2 py-1 text-sm transition-colors hover:bg-muted">
              <PackageCheck className="h-4 w-4" />
              已选择 <strong>{totalSelected}</strong> 个版本
              <ChevronDown className="h-3 w-3 text-muted-foreground" />
            </PopoverTrigger>
            <PopoverContent
              align="start"
              side="top"
              className="w-80 p-0"
            >
              <div className="flex items-center justify-between border-b px-3 py-2">
                <span className="text-sm font-medium">
                  已选择 {totalSelected} 个版本
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 text-xs text-muted-foreground"
                  onClick={() => setSelected(new Map())}
                >
                  清空
                </Button>
              </div>
              <div className="max-h-64 overflow-y-auto p-2">
                {Array.from(selected.entries()).map(([pkgName, versions]) => (
                  <div key={pkgName} className="mb-2 last:mb-0">
                    <div className="px-1 text-xs font-medium text-muted-foreground">
                      {pkgName}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-1">
                      {Array.from(versions).map((v) => (
                        <span
                          key={v}
                          className="group flex items-center gap-1 rounded-md border bg-muted/50 px-2 py-0.5 text-xs"
                        >
                          {v}
                          <X
                            className="h-3 w-3 cursor-pointer text-muted-foreground opacity-0 transition-opacity hover:text-foreground group-hover:opacity-100"
                            onClick={() => toggleVersion(pkgName, v)}
                          />
                        </span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </PopoverContent>
          </Popover>
          <div className="flex gap-2">
            {isVerdaccio && (
              <>
                <Button
                  variant="destructive"
                  onClick={() => {
                    const t = collectSelectedTargets();
                    if (t.length === 0) {
                      toast.error("选中的版本不在当前 Verdaccio 缓存中");
                      return;
                    }
                    setUnpublishTargets(t);
                  }}
                >
                  Unpublish 选中
                </Button>
                <Button
                  variant="outline"
                  className="border-amber-500/40 text-amber-600 hover:bg-amber-500/10 hover:text-amber-600 dark:text-amber-500"
                  onClick={() => {
                    const t = collectSelectedTargets();
                    if (t.length === 0) {
                      toast.error("选中的版本不在当前 Verdaccio 缓存中");
                      return;
                    }
                    setDeprecateTargets(t);
                  }}
                >
                  Deprecate 选中
                </Button>
              </>
            )}
            <ExportDropdown
              getSelectedPackages={() => {
                const packages: { package_name: string; version: string }[] = [];
                selected.forEach((versions, pkgName) => {
                  versions.forEach((v) => {
                    packages.push({ package_name: pkgName, version: v });
                  });
                });
                return packages;
              }}
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

      {contextMenu && (
        <RowContextMenu
          position={contextMenu.position}
          onClose={() => setContextMenu(null)}
          showManageActions={isVerdaccio}
          onCache={
            contextMenu.version && !contextMenu.cached
              ? () =>
                  startCacheTasks([
                    {
                      package_name: contextMenu.pkg,
                      version: contextMenu.version!,
                    },
                  ])
              : undefined
          }
          onCacheWithDeps={
            contextMenu.version && !contextMenu.cached
              ? async () => {
                  const resolved = await resolveDependencies([
                    {
                      package_name: contextMenu.pkg,
                      version: contextMenu.version!,
                    },
                  ]);
                  await startCacheTasks(
                    resolved.map((r) => ({
                      package_name: r.package_name,
                      version: r.version,
                    }))
                  );
                }
              : undefined
          }
          onUnpublish={
            contextMenu.version && contextMenu.cached
              ? () =>
                  setUnpublishTargets([
                    {
                      name: contextMenu.pkg,
                      version: contextMenu.version,
                    },
                  ])
              : undefined
          }
          onDeprecate={
            contextMenu.version && contextMenu.cached
              ? () =>
                  setDeprecateTargets([
                    {
                      name: contextMenu.pkg,
                      version: contextMenu.version,
                    },
                  ])
              : undefined
          }
          onUnpublishAll={
            !contextMenu.version
              ? () => setUnpublishTargets([{ name: contextMenu.pkg }])
              : undefined
          }
        />
      )}

      <UnpublishConfirmDialog
        open={unpublishTargets !== null}
        targets={unpublishTargets ?? []}
        loading={manageLoading}
        onCancel={() => setUnpublishTargets(null)}
        onConfirm={() => runUnpublish(unpublishTargets ?? [])}
      />

      <DeprecateConfirmDialog
        open={deprecateTargets !== null}
        targets={deprecateTargets ?? []}
        loading={manageLoading}
        onCancel={() => setDeprecateTargets(null)}
        onConfirm={(message) =>
          runDeprecate(deprecateTargets ?? [], message)
        }
      />
    </div>
  );
}

function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const na = pa[i] || 0;
    const nb = pb[i] || 0;
    if (na !== nb) return na - nb;
  }
  return 0;
}
