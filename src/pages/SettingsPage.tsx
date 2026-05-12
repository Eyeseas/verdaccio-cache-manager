import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useConfigStore } from "@/stores/configStore";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { FolderOpen } from "lucide-react";

export function SettingsPage() {
  const { config, loadConfig, saveConfig, testConnection } = useConfigStore();
  const [form, setForm] = useState(config);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  useEffect(() => {
    setForm(config);
  }, [config]);

  const handleSave = async () => {
    await saveConfig(form);
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      await testConnection(form.registry_url);
      setTestResult("连接成功");
    } catch (e) {
      setTestResult(`连接失败: ${e}`);
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="p-6 max-w-2xl">
      <h1 className="text-2xl font-bold mb-6">设置</h1>

      <Card>
        <CardHeader>
          <CardTitle>Verdaccio 配置</CardTitle>
          <CardDescription>配置私有 registry 地址和任务参数</CardDescription>
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
                disabled={testing}
              >
                {testing ? "测试中..." : "测试连接"}
              </Button>
            </div>
            {testResult && (
              <p
                className={`text-sm ${testResult.startsWith("连接成功") ? "text-green-600" : "text-destructive"}`}
              >
                {testResult}
              </p>
            )}
          </div>

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
            </div>
          </div>

          <div className="space-y-2 border-t pt-4">
            <Label htmlFor="storage_path">Verdaccio storage 路径（可选）</Label>
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
                placeholder="/path/to/verdaccio/storage 或挂载点"
              />
              <Button
                variant="outline"
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
              用于在「搜索 → Verdaccio」中列出已缓存包（含 proxy
              缓存）。可填本地路径或通过 SMB/NFS/sshfs 挂载的远程目录。若 Verdaccio
              安装了 verdaccio-plugin-cached-list 插件则可不填。
            </p>
          </div>

          <Button onClick={handleSave}>保存配置</Button>
        </CardContent>
      </Card>
    </div>
  );
}
