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
import { Download, ChevronUp, Loader2, Box, Boxes } from "lucide-react";
import { toast } from "sonner";
import {
  type DownloadSummary,
  toastDownloadSummary,
} from "@/lib/downloadSummary";

interface ExportDropdownProps {
  getSelectedPackages: () =>
    | { package_name: string; version: string }[]
    | Promise<{ package_name: string; version: string }[]>;
  disabled?: boolean;
  onExportingChange?: (exporting: boolean) => void;
}

export function ExportDropdown({
  getSelectedPackages,
  disabled,
  onExportingChange,
}: ExportDropdownProps) {
  const { resolveDependencies } = useTaskStore();
  const [exporting, setExporting] = useState(false);

  const setExportingState = (value: boolean) => {
    setExporting(value);
    onExportingChange?.(value);
  };

  const selectOutputDir = async (): Promise<string | null> => {
    const dir = await open({ directory: true, multiple: false });
    return dir ?? null;
  };

  const doExport = async (packages: { package_name: string; version: string }[]) => {
    const dir = await selectOutputDir();
    if (!dir) return;

    try {
      const summary = await invoke<DownloadSummary>("download_tarballs", {
        packages,
        outputDir: dir,
      });
      toastDownloadSummary(summary);
    } catch (e) {
      toast.error("导出失败", { description: String(e) });
    }
  };

  const collectPackages = async () => {
    try {
      return await getSelectedPackages();
    } catch (e) {
      toast.error("版本解析失败", { description: String(e) });
      return null;
    }
  };

  const handleExportSelected = async () => {
    setExportingState(true);
    try {
      const packages = await collectPackages();
      if (!packages || packages.length === 0) return;
      await doExport(packages);
    } finally {
      setExportingState(false);
    }
  };

  const handleExportWithDeps = async () => {
    setExportingState(true);
    try {
      const packages = await collectPackages();
      if (!packages || packages.length === 0) return;

      const resolved = await resolveDependencies(packages);
      const resolvedPkgs = resolved.map((r) => ({
        package_name: r.package_name,
        version: r.version,
      }));
      await doExport(resolvedPkgs);
    } catch (e) {
      toast.error("依赖解析失败", { description: String(e) });
    } finally {
      setExportingState(false);
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
          <Box className="h-4 w-4" />
          仅导出选中的包
        </DropdownMenuItem>
        <DropdownMenuItem onClick={handleExportWithDeps}>
          <Boxes className="h-4 w-4" />
          导出选中的包及其依赖
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
