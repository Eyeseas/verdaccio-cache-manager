import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useConfigStore } from "@/stores/configStore";
import { useTaskStore } from "@/stores/taskStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Search, Loader2, ChevronDown, ChevronRight } from "lucide-react";

interface SearchResult {
  name: string;
  description: string | null;
  latest_version: string | null;
  versions: string[];
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

export function SearchPage() {
  const { config } = useConfigStore();
  const { startCacheTasks } = useTaskStore();

  const [query, setQuery] = useState("");
  const [source, setSource] = useState<RegistrySource>("npmjs");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, ExpandedPackage>>({});
  const [selected, setSelected] = useState<Map<string, Set<string>>>(
    new Map()
  );
  const [stableOnly, setStableOnly] = useState(true);

  const registryUrl =
    source === "npmjs" ? "https://registry.npmjs.org" : config.registry_url;

  const handleSearch = useCallback(async () => {
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
  }, [query, registryUrl]);

  const toggleExpand = async (name: string) => {
    if (expanded[name]) {
      const next = { ...expanded };
      delete next[name];
      setExpanded(next);
      return;
    }

    setExpanded((prev) => ({
      ...prev,
      [name]: { name, versions: [], loading: true },
    }));

    try {
      const versions = await invoke<string[]>("get_package_versions", {
        registryUrl,
        packageName: name,
      });

      setExpanded((prev) => ({
        ...prev,
        [name]: { name, versions, loading: false },
      }));
    } catch {
      setExpanded((prev) => ({
        ...prev,
        [name]: { name, versions: [], loading: false },
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

  const getFilteredVersions = (versions: string[]) => {
    const filtered = stableOnly
      ? versions.filter(isStableVersion)
      : versions;
    return filtered.sort((a, b) => compareVersions(b, a));
  };

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 space-y-3">
        <h1 className="text-2xl font-bold">搜索与缓存</h1>

        <div className="flex gap-2">
          <div className="flex rounded-md border">
            <button
              className={`px-3 py-1.5 text-sm transition-colors ${
                source === "npmjs"
                  ? "bg-primary text-primary-foreground"
                  : "hover:bg-muted"
              }`}
              onClick={() => setSource("npmjs")}
            >
              npmjs
            </button>
            <button
              className={`px-3 py-1.5 text-sm transition-colors ${
                source === "verdaccio"
                  ? "bg-primary text-primary-foreground"
                  : "hover:bg-muted"
              }`}
              onClick={() => setSource("verdaccio")}
            >
              Verdaccio
            </button>
          </div>

          <div className="relative flex-1">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="pl-9"
              placeholder="搜索 npm 包..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
            />
          </div>

          <Button onClick={handleSearch} disabled={searching}>
            {searching ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              "搜索"
            )}
          </Button>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={stableOnly}
            onCheckedChange={(v) => setStableOnly(v === true)}
          />
          <span>仅显示正式版本</span>
        </label>
      </div>

      <ScrollArea className="flex-1">
        <div className="space-y-1">
          {results.map((pkg) => (
            <div key={pkg.name} className="rounded-lg border">
              <div
                className="flex cursor-pointer items-center gap-3 p-3 hover:bg-muted/50"
                onClick={() => toggleExpand(pkg.name)}
              >
                {expanded[pkg.name] ? (
                  <ChevronDown className="h-4 w-4 shrink-0" />
                ) : (
                  <ChevronRight className="h-4 w-4 shrink-0" />
                )}
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-medium">{pkg.name}</span>
                    {pkg.latest_version && (
                      <Badge variant="secondary">{pkg.latest_version}</Badge>
                    )}
                  </div>
                  {pkg.description && (
                    <p className="truncate text-sm text-muted-foreground">
                      {pkg.description}
                    </p>
                  )}
                </div>
              </div>

              {expanded[pkg.name] && (
                <div className="border-t px-3 py-2">
                  {expanded[pkg.name].loading ? (
                    <div className="flex items-center gap-2 py-2 text-sm text-muted-foreground">
                      <Loader2 className="h-3 w-3 animate-spin" />
                      加载版本列表...
                    </div>
                  ) : (
                    <div className="max-h-48 overflow-y-auto">
                      <div className="flex flex-wrap gap-2 py-1">
                        {getFilteredVersions(expanded[pkg.name].versions).map(
                          (v) => {
                            const isSelected =
                              selected.get(pkg.name)?.has(v) || false;

                            return (
                              <label
                                key={v}
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
                          }
                        )}
                        {getFilteredVersions(expanded[pkg.name].versions)
                          .length === 0 && (
                          <span className="text-sm text-muted-foreground">
                            {stableOnly
                              ? "无正式版本"
                              : "无版本信息"}
                          </span>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}

          {!searching && results.length === 0 && query && (
            <p className="py-8 text-center text-muted-foreground">
              未找到匹配的包
            </p>
          )}
        </div>
      </ScrollArea>

      {totalSelected > 0 && (
        <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
          <span className="text-sm">
            已选择 <strong>{totalSelected}</strong> 个版本
          </span>
          <Button onClick={handleCache}>缓存到私服</Button>
        </div>
      )}
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
