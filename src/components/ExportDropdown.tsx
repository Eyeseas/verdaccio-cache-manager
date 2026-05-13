import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useTaskStore } from "@/stores/taskStore";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Download, ChevronUp, Loader2 } from "lucide-react";
import { toast } from "sonner";

interface ExportDropdownProps {
  getSelectedPackages: () => { package_name: string; version: string }[];
  disabled?: boolean;
}

export function ExportDropdown({
  getSelectedPackages,
  disabled,
}: ExportDropdownProps) {
  const { resolveDependencies } = useTaskStore();
  const [exporting, setExporting] = useState(false);

  const selectOutputDir = async (): Promise<string | null> => {
    const dir = await open({ directory: true, multiple: false });
    return dir ?? null;
  };

  const doExport = async (packages: { package_name: string; version: string }[]) => {
    const dir = await selectOutputDir();
    if (!dir) return;

    setExporting(true);
    try {
      const count = await invoke<number>("download_tarballs", {
        packages,
        outputDir: dir,
      });
      toast.success(`导出完成`, {
        description: `已下载 ${count} 个 tarball 到目标目录`,
      });
    } catch (e) {
      toast.error("导出失败", { description: String(e) });
    } finally {
      setExporting(false);
    }
  };

  const handleExportSelected = async () => {
    const packages = getSelectedPackages();
    if (packages.length === 0) return;
    await doExport(packages);
  };

  const handleExportWithDeps = async () => {
    const packages = getSelectedPackages();
    if (packages.length === 0) return;

    setExporting(true);
    try {
      const resolved = await resolveDependencies(packages);
      const resolvedPkgs = resolved.map((r) => ({
        package_name: r.package_name,
        version: r.version,
      }));
      setExporting(false);
      await doExport(resolvedPkgs);
    } catch (e) {
      toast.error("依赖解析失败", { description: String(e) });
      setExporting(false);
    }
  };

  if (exporting) {
    return (
      <span className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-background px-2.5 text-sm font-medium opacity-50">
        <Loader2 className="h-4 w-4 animate-spin" />
        导出中...
      </span>
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-background px-2.5 text-sm font-medium whitespace-nowrap transition-all outline-none select-none hover:bg-muted hover:text-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 dark:border-input dark:bg-input/30 dark:hover:bg-input/50"
        disabled={disabled}
      >
        <Download className="h-4 w-4" />
        导出 Tarball
        <ChevronUp className="h-4 w-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={handleExportSelected}>
          仅导出选中的包
        </DropdownMenuItem>
        <DropdownMenuItem onClick={handleExportWithDeps}>
          导出选中的包及其依赖
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
