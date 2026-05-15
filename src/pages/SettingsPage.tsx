import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useConfigStore } from "@/stores/configStore";
import { useSyncStore } from "@/stores/syncStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  FolderOpen,
  RefreshCw,
  Trash2,
  Loader2,
  Check,
  Unplug,
  Database,
  Settings2,
  Heart,
} from "lucide-react";
import { toast } from "sonner";

export function SettingsPage() {
  const { config, loadConfig, saveConfig, testConnection } = useConfigStore();
  const { status: syncStatus, lastSyncAt, startSync, clearIndex } =
    useSyncStore();
  const [form, setForm] = useState(config);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    ok: boolean;
    msg: string;
  } | null>(null);
  const [saving, setSaving] = useState(false);
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    loadConfig();
    getVersion().then(setVersion);
  }, [loadConfig]);

  useEffect(() => {
    setForm(config);
  }, [config]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await saveConfig(form);
      toast.success("配置已保存");
    } catch (e) {
      toast.error("保存失败", { description: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      await testConnection(form.registry_url);
      setTestResult({ ok: true, msg: "连接成功" });
    } catch (e) {
      setTestResult({ ok: false, msg: `连接失败: ${e}` });
    } finally {
      setTesting(false);
    }
  };

  const handleCheckUpdate = async () => {
    setChecking(true);
    try {
      const update = await check();
      if (update) {
        toast.info(`发现新版本 v${update.version}`, {
          description: "正在下载更新...",
          duration: Infinity,
          id: "update-progress",
        });
        await update.downloadAndInstall();
        toast.success("更新已下载，即将重启应用", { id: "update-progress" });
        await relaunch();
      } else {
        toast.info("当前已是最新版本");
      }
    } catch (e) {
      toast.error("检查更新失败", { description: String(e) });
    } finally {
      setChecking(false);
    }
  };

  const isDirty =
    form.registry_url !== config.registry_url ||
    form.concurrency !== config.concurrency ||
    form.retry_count !== config.retry_count ||
    form.timeout_secs !== config.timeout_secs ||
    form.verdaccio_storage_path !== config.verdaccio_storage_path;

  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mx-auto max-w-2xl space-y-6">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">设置</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            管理 Verdaccio 私有源连接和缓存任务参数
          </p>
        </div>

        {/* Registry Connection */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Unplug className="h-4 w-4" />
              Registry 连接
            </CardTitle>
            <CardDescription>
              配置 Verdaccio 私有源地址，变更后将自动重新同步索引
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="registry_url">Registry 地址</Label>
              <div className="flex gap-2">
                <Input
                  id="registry_url"
                  value={form.registry_url}
                  onChange={(e) =>
                    setForm({ ...form, registry_url: e.target.value })
                  }
                  placeholder="http://localhost:4873"
                />
                <Button
                  variant="outline"
                  onClick={handleTest}
                  disabled={testing || !form.registry_url}
                >
                  {testing ? (
                    <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  ) : null}
                  {testing ? "测试中" : "测试连接"}
                </Button>
              </div>
              {testResult && (
                <p
                  className={`flex items-center gap-1.5 text-sm ${testResult.ok ? "text-green-600" : "text-destructive"}`}
                >
                  {testResult.ok ? (
                    <Check className="h-3.5 w-3.5" />
                  ) : null}
                  {testResult.msg}
                </p>
              )}
            </div>

            <div className="space-y-2">
              <Label htmlFor="storage_path">
                Storage 路径
                <span className="ml-1.5 text-xs font-normal text-muted-foreground">
                  可选
                </span>
              </Label>
              <div className="flex gap-2">
                <Input
                  id="storage_path"
                  value={form.verdaccio_storage_path ?? ""}
                  onChange={(e) =>
                    setForm({
                      ...form,
                      verdaccio_storage_path: e.target.value || null,
                    })
                  }
                  placeholder="/path/to/verdaccio/storage"
                />
                <Button
                  variant="outline"
                  size="icon"
                  onClick={async () => {
                    const selected = await open({
                      directory: true,
                      multiple: false,
                    });
                    if (typeof selected === "string") {
                      setForm({ ...form, verdaccio_storage_path: selected });
                    }
                  }}
                >
                  <FolderOpen className="h-4 w-4" />
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                用于列出已缓存包（含 proxy 缓存）。支持本地路径或
                SMB/NFS/sshfs 挂载目录。安装了 verdaccio-cached-list
                插件则可不填。
              </p>
            </div>
          </CardContent>
        </Card>

        {/* Task Parameters */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Settings2 className="h-4 w-4" />
              任务参数
            </CardTitle>
            <CardDescription>
              控制缓存下载任务的并发、重试和超时行为
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-3 gap-4">
              <div className="space-y-2">
                <Label htmlFor="concurrency">并发数</Label>
                <Input
                  id="concurrency"
                  type="number"
                  min={1}
                  max={20}
                  value={form.concurrency}
                  onChange={(e) =>
                    setForm({ ...form, concurrency: Number(e.target.value) })
                  }
                />
                <p className="text-xs text-muted-foreground">1–20</p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="retry_count">重试次数</Label>
                <Input
                  id="retry_count"
                  type="number"
                  min={0}
                  max={10}
                  value={form.retry_count}
                  onChange={(e) =>
                    setForm({ ...form, retry_count: Number(e.target.value) })
                  }
                />
                <p className="text-xs text-muted-foreground">0–10</p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="timeout_secs">超时 (秒)</Label>
                <Input
                  id="timeout_secs"
                  type="number"
                  min={10}
                  max={300}
                  value={form.timeout_secs}
                  onChange={(e) =>
                    setForm({ ...form, timeout_secs: Number(e.target.value) })
                  }
                />
                <p className="text-xs text-muted-foreground">10–300</p>
              </div>
            </div>
          </CardContent>
          <CardFooter className="justify-between">
            <p className="text-xs text-muted-foreground">
              {isDirty ? "有未保存的更改" : "所有更改已保存"}
            </p>
            <Button onClick={handleSave} disabled={saving || !isDirty}>
              {saving ? (
                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
              ) : null}
              保存配置
            </Button>
          </CardFooter>
        </Card>

        {/* Cache Index */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Database className="h-4 w-4" />
              缓存索引
            </CardTitle>
            <CardDescription>
              本地索引用于快速查询已缓存的包。源地址变更时自动全量同步，手动同步为增量更新。
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-center justify-between">
              <div className="space-y-1">
                {lastSyncAt ? (
                  <p className="text-sm">
                    上次同步:{" "}
                    <span className="text-muted-foreground">
                      {new Date(lastSyncAt).toLocaleString()}
                    </span>
                  </p>
                ) : (
                  <p className="text-sm text-muted-foreground">尚未同步</p>
                )}
              </div>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={startSync}
                  disabled={syncStatus === "syncing"}
                >
                  <RefreshCw
                    className={`mr-1.5 h-3.5 w-3.5 ${syncStatus === "syncing" ? "animate-spin" : ""}`}
                  />
                  {syncStatus === "syncing" ? "同步中..." : "同步"}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={clearIndex}
                  disabled={syncStatus === "syncing"}
                  className="text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="mr-1.5 h-3.5 w-3.5" />
                  清除
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <div className="flex items-center justify-center gap-3 text-xs text-muted-foreground">
          <span>
            {version && `v${version}`} <Heart className="inline h-3 w-3 fill-red-500 text-red-500" /> Eyeseas
          </span>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 text-xs text-muted-foreground"
            onClick={handleCheckUpdate}
            disabled={checking}
          >
            {checking ? (
              <Loader2 className="mr-1 h-3 w-3 animate-spin" />
            ) : (
              <RefreshCw className="mr-1 h-3 w-3" />
            )}
            {checking ? "检查中..." : "检查更新"}
          </Button>
        </div>
      </div>
    </div>
  );
}
