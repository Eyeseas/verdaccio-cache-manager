import assert from "node:assert/strict";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, it, vi } from "vitest";
import { StorageHealthPage } from "./StorageHealthPage";
import { Sidebar } from "@/components/layout/Sidebar";
import { useHealthStore, type HealthSnapshot } from "@/stores/healthStore";

const { pageConfig } = vi.hoisted(() => ({ pageConfig: {
  registry_url: "http://configured", verdaccio_storage_path: null,
} }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: vi.fn(), open: vi.fn() }));
vi.mock("@/stores/configStore", async (importOriginal) => ({
  ...await importOriginal<object>(),
  useConfigStore: () => ({
    config: pageConfig,
    loadConfig: vi.fn(),
  }),
}));

const snapshot: HealthSnapshot = {
  mode: "remote", value: "http://automatic", source: "automatic", completedAt: 1_790_000_000_000,
  report: { scanned: 12, unhealthy: [], unreadable: [], plugin_outdated: false }, error: null,
};
beforeEach(() => {
  pageConfig.registry_url = "http://configured";
  useHealthStore.setState({ latest: null, busy: false });
});

describe("shared health result UI", () => {
  it("follows asynchronously loaded registry defaults and preserves a custom address", () => {
    const { rerender } = render(<StorageHealthPage />);
    fireEvent.click(screen.getByRole("button", { name: "远程插件" }));
    assert.ok(screen.getByDisplayValue("http://configured"));
    pageConfig.registry_url = "http://saved-registry";
    rerender(<StorageHealthPage />);
    fireEvent.change(screen.getByDisplayValue("http://saved-registry"), { target: { value: "http://custom" } });
    pageConfig.registry_url = "http://another-default";
    rerender(<StorageHealthPage />);
    assert.ok(screen.getByDisplayValue("http://custom"));
  });

  it("opens the latest automatic result with its target and time", () => {
    useHealthStore.setState({ latest: snapshot });
    render(<StorageHealthPage />);
    assert.ok(screen.getByText(/最近自动体检/).textContent?.includes(new Date(snapshot.completedAt).toLocaleString("zh-CN")));
    assert.ok(screen.getByDisplayValue("http://automatic"));
    assert.ok(screen.getByText(/已检查 12 个包/));
    assert.ok(screen.getByText("一切正常"));
  });

  it("disables manual checks while a background operation is active", () => {
    useHealthStore.setState({ latest: snapshot, busy: true });
    render(<StorageHealthPage />);
    assert.equal(screen.getByRole("button", { name: "体检中" }).hasAttribute("disabled"), true);
  });

  it("does not report a healthy registry when metadata cannot be read", () => {
    useHealthStore.setState({ latest: { ...snapshot, report: {
      scanned: 1, unhealthy: [], unreadable: ["broken/package.json"], plugin_outdated: false,
    } } });
    render(<StorageHealthPage />);
    assert.equal(screen.queryByText("一切正常") === null, true);
    assert.ok(screen.getByText(/1 个元数据无法解析/));
  });

  it("hides a zero badge and exposes a failed check in the menu tooltip", () => {
    useHealthStore.setState({ latest: snapshot });
    const { unmount } = render(<MemoryRouter><Sidebar /></MemoryRouter>);
    assert.equal(screen.queryByLabelText("仓库体检"), null);
    unmount();
    useHealthStore.setState({ latest: { ...snapshot, report: null, error: "连接被拒绝" } });
    render(<MemoryRouter><Sidebar /></MemoryRouter>);
    assert.equal(screen.getByText("体检").closest("a")?.title, "体检失败：连接被拒绝");
    assert.equal(screen.getByLabelText("体检失败：连接被拒绝").textContent, "!");
  });
});
