import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { UnpublishConfirmDialog } from "./UnpublishConfirmDialog";

function setup(loading = false) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(
    <UnpublishConfirmDialog
      open
      targets={[{ name: "left-pad", version: "1.0.0" }]}
      loading={loading}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
  return { onCancel, onConfirm };
}

describe("UnpublishConfirmDialog", () => {
  it("keeps confirm disabled until the confirmation word is typed", () => {
    const { onConfirm } = setup();
    const confirm = screen.getByRole("button", { name: "确认删除" });

    expect(confirm).toBeDisabled();
    fireEvent.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();

    fireEvent.change(screen.getByPlaceholderText("unpublish"), {
      target: { value: "unpublish" },
    });

    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("invokes onCancel from the cancel button", () => {
    const { onCancel } = setup();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("shows the processing state while loading", () => {
    setup(true);
    expect(
      screen.getByRole("button", { name: /处理中/ })
    ).toBeDisabled();
  });
});
