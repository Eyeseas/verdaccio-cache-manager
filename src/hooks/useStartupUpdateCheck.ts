import { useEffect } from "react";
import { checkForUpdateOnStartup } from "@/lib/appUpdate";

export function useStartupUpdateCheck() {
  useEffect(() => {
    // 开发模式版本号不代表发布版本，跳过以免误报。
    if (import.meta.env.DEV) return;
    void checkForUpdateOnStartup();
  }, []);
}
