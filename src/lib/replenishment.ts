import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import type { ReplenishmentList } from "@/pages/replenishmentLogic";

export async function saveReplenishmentList(list: ReplenishmentList) {
  if (!list.entries.length) { toast.warning("没有可导出的精确缺失版本"); return; }
  const outputPath = await save({ defaultPath: "replenishment.json", filters: [{ name: "补包清单", extensions: ["json"] }] });
  if (!outputPath) return;
  await invoke("export_replenishment_list", { outputPath, list });
  toast.success(`已导出 ${list.entries.length} 条补包清单`);
}
