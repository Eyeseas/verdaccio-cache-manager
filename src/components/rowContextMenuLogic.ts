export interface MenuPoint {
  x: number;
  y: number;
}

export interface MenuSize {
  width: number;
  height: number;
}

export interface ViewportSize {
  width: number;
  height: number;
}

export interface ContextMenuLayout {
  left: number;
  top: number;
  submenuSide: "left" | "right";
}

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

export const positionContextMenu = (args: {
  anchor: MenuPoint;
  menuSize: MenuSize;
  submenuSize: MenuSize;
  viewport: ViewportSize;
  gap?: number;
}): ContextMenuLayout => {
  const gap = args.gap ?? 4;
  const maxLeft = Math.max(0, args.viewport.width - args.menuSize.width);
  const maxTop = Math.max(0, args.viewport.height - args.menuSize.height);

  let left =
    args.anchor.x + args.menuSize.width > args.viewport.width
      ? args.anchor.x - args.menuSize.width - gap
      : args.anchor.x;
  let top =
    args.anchor.y + args.menuSize.height > args.viewport.height
      ? args.anchor.y - args.menuSize.height - gap
      : args.anchor.y;

  left = clamp(left, 0, maxLeft);
  top = clamp(top, 0, maxTop);

  const fitsRight =
    left + args.menuSize.width + gap + args.submenuSize.width <=
    args.viewport.width;
  const fitsLeft = left - gap - args.submenuSize.width >= 0;
  const submenuSide = fitsRight || !fitsLeft ? "right" : "left";

  return { left, top, submenuSide };
};
