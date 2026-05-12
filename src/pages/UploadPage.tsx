import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTaskStore } from "@/stores/taskStore";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { FolderOpen, Upload, Loader2, Package } from "lucide-react";

interface LocalPackage {
  name: string;
  version: string;
  path: string;
}

interface PackageWithStatus extends LocalPackage {
  cached: boolean;
}

export function UploadPage() {
  return (
    <div className="flex h-full flex-col p-6">
      <h1 className="mb-4 text-2xl font-bold">本地上传</h1>
      <Tabs defaultValue="scan" className="flex flex-1 flex-col">
        <TabsList className="w-fit">
          <TabsTrigger value="scan">扫描 node_modules</TabsTrigger>
          <TabsTrigger value="tgz">上传 .tgz</TabsTrigger>
        </TabsList>
        <TabsContent value="scan" className="flex-1">
          <ScanTab />
        </TabsContent>
        <TabsContent value="tgz" className="flex-1">
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

  const handleSelectDir = async () => {
    const dir = await open({ directory: true });
    if (!dir) return;

    setLoading(true);
    setPackages([]);
    setSelected(new Set());
    setDirPath(dir);

    try {
      const scanned = await invoke<LocalPackage[]>("scan_node_modules", {
        dirPath: dir,
      });

      const withStatus: PackageWithStatus[] = scanned.map((pkg) => ({
        ...pkg,
        cached: false,
      }));

      setPackages(withStatus);
      const allIndices = new Set<number>(withStatus.map((_, i) => i));
      setSelected(allIndices);
    } catch (e) {
      console.error("扫描失败:", e);
    } finally {
      setLoading(false);
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

  const handleUpload = async () => {
    const pkgs = Array.from(selected).map((i) => ({
      package_name: packages[i].name,
      version: packages[i].version,
    }));
    if (pkgs.length === 0) return;
    await startCacheTasks(pkgs);
    setSelected(new Set());
  };

  const uncachedCount = packages.filter((p) => !p.cached).length;

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Loader2 className="mr-2 h-5 w-5 animate-spin" />
        <span>扫描 node_modules 中...</span>
      </div>
    );
  }

  if (packages.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
        <Package className="mb-4 h-12 w-12 text-muted-foreground" />
        <p className="mb-2 text-lg font-medium">选择项目目录</p>
        <p className="mb-4 text-sm text-muted-foreground">
          扫描 node_modules 中的包并上传到 Verdaccio
        </p>
        <Button variant="outline" onClick={handleSelectDir}>
          <FolderOpen className="mr-2 h-4 w-4" />
          选择目录
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col pt-3">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Badge variant="outline">{dirPath}</Badge>
          <span className="text-sm text-muted-foreground">
            共 {packages.length} 个包，{uncachedCount} 个未缓存
          </span>
        </div>
        <Button variant="outline" size="sm" onClick={handleSelectDir}>
          重新扫描
        </Button>
      </div>

      <ScrollArea className="flex-1 rounded-md border">
        <div className="divide-y">
          {packages.map((pkg, i) => (
            <div
              key={`${pkg.name}@${pkg.version}`}
              className="flex items-center gap-3 px-4 py-2"
            >
              {!pkg.cached ? (
                <Checkbox
                  checked={selected.has(i)}
                  onCheckedChange={() => toggleSelect(i)}
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
          ))}
        </div>
      </ScrollArea>

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

function TgzTab() {
  const { startCacheTasks } = useTaskStore();
  const [files, setFiles] = useState<LocalPackage[]>([]);
  const [loading, setLoading] = useState(false);

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
      const parsed: LocalPackage[] = [];
      for (const p of paths) {
        const pkg = await invoke<LocalPackage>("parse_tgz", { filePath: p });
        parsed.push(pkg);
      }
      setFiles(parsed);
    } catch (e) {
      console.error("解析 tgz 失败:", e);
    } finally {
      setLoading(false);
    }
  };

  const handleUpload = async () => {
    const packages = files.map((f) => ({
      package_name: f.name,
      version: f.version,
      tarball_url: `file://${f.path}`,
    }));
    await startCacheTasks(packages);
    setFiles([]);
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
        className="flex flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12"
        onDragOver={(e) => e.preventDefault()}
        onDrop={async (e) => {
          e.preventDefault();
          const paths: string[] = [];
          for (const file of Array.from(e.dataTransfer.files)) {
            const path = (file as unknown as { path: string }).path;
            if (path && path.endsWith(".tgz")) paths.push(path);
          }
          if (paths.length > 0) await parseTgzFiles(paths);
        }}
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

  return (
    <div className="flex flex-1 flex-col pt-3">
      <div className="mb-3 flex items-center justify-between">
        <span className="text-sm text-muted-foreground">
          已解析 {files.length} 个 tarball
        </span>
        <Button variant="outline" size="sm" onClick={handleSelectFiles}>
          添加更多
        </Button>
      </div>

      <ScrollArea className="flex-1 rounded-md border">
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
            </div>
          ))}
        </div>
      </ScrollArea>

      <div className="mt-4 flex items-center justify-between rounded-lg border bg-muted/30 p-3">
        <span className="text-sm">
          共 <strong>{files.length}</strong> 个包
        </span>
        <Button onClick={handleUpload}>上传到私服</Button>
      </div>
    </div>
  );
}
