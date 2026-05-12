import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTaskStore } from "@/stores/taskStore";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { FileInput, Loader2, FolderOpen } from "lucide-react";

interface ParsedDependency {
  name: string;
  version: string;
  tarball_url: string | null;
}

interface DependencyWithStatus extends ParsedDependency {
  cached: boolean;
}

export function ImportPage() {
  const { startCacheTasks } = useTaskStore();

  const [deps, setDeps] = useState<DependencyWithStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [fileName, setFileName] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

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
      setDeps([]);
      setSelected(new Set());
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

        // Don't check Verdaccio for cached status — Verdaccio proxies upstream
        // and would report all npmjs versions as "available". Instead, show all
        // deps as uncached and let the task engine handle 409 conflicts (→ Skipped).
        const withStatus: DependencyWithStatus[] = parsed.map((dep) => ({
          ...dep,
          cached: false,
        }));

        setDeps(withStatus);

        const allIndices = new Set<number>(
          withStatus.map((_, i) => i)
        );
        setSelected(allIndices);
      } catch (e) {
        setError(`解析失败: ${e}`);
      } finally {
        setLoading(false);
      }
    },
    []
  );

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

  const uncachedCount = deps.filter((d) => !d.cached).length;

  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-4 text-2xl font-bold">导入</h1>

      {deps.length === 0 && !loading && (
        <div
          className="flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12 text-center"
          onDragOver={(e) => e.preventDefault()}
          onDrop={async (e) => {
            e.preventDefault();
            const file = e.dataTransfer.files[0];
            if (file) {
              const path = (file as unknown as { path: string }).path;
              if (path) await parseFile(path);
            }
          }}
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

          <ScrollArea className="flex-1 rounded-md border">
            <div className="divide-y">
              {deps.map((dep, i) => (
                <div
                  key={`${dep.name}@${dep.version}-${i}`}
                  className="flex items-center gap-3 px-4 py-2"
                >
                  {!dep.cached ? (
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
                  {dep.cached ? (
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
              ))}
            </div>
          </ScrollArea>

          {selected.size > 0 && (
            <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
              <span className="text-sm">
                已选择 <strong>{selected.size}</strong> 个包
              </span>
              <Button onClick={handleCache}>缓存到私服</Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
