import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  applyResolveProgress,
  applyResolvedPackagesForCurrentRequest,
  applyResolvedPackages,
  areSelectionsEqual,
  cacheTaskInputsFromResolved,
  cacheTaskInputsFromDependencies,
  dependencyRootsFromResolved,
  exportPackagesFromResolvedSelection,
  getContextMenuActionState,
  getResolvedVersionOrThrow,
  isCurrentResolveRequest,
  pruneSelection,
  removeResolvedFromSelection,
  rowKey,
  shouldShowActionBar,
  type ParsedDependency,
  type ResolvedImportPackage,
  type RowState,
} from "../src/pages/importPageLogic.ts";
import { positionContextMenu } from "../src/components/rowContextMenuLogic.ts";

describe("import page package resolution flow", () => {
  it("keeps cached resolved root packages for dependency resolution", () => {
    const resolved: ResolvedImportPackage[] = [
      {
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        tarball_url: null,
        cached: true,
      },
    ];

    assert.deepEqual(dependencyRootsFromResolved(resolved), [
      { package_name: "react", version: "19.1.0" },
    ]);
  });

  it("starts cache tasks only for resolved packages that are not cached", () => {
    const resolved: ResolvedImportPackage[] = [
      {
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        tarball_url: null,
        cached: true,
      },
      {
        name: "vite",
        raw_range: "~7.0",
        version: "7.0.4",
        tarball_url: null,
        cached: false,
      },
    ];

    assert.deepEqual(cacheTaskInputsFromResolved(resolved), [
      { package_name: "vite", version: "7.0.4", tarball_url: undefined },
    ]);
  });

  it("starts dependency cache tasks only for dependency versions that are not cached", () => {
    assert.deepEqual(
      cacheTaskInputsFromDependencies(
        [
          { package_name: "react", version: "19.1.0" },
          { package_name: "loose-envify", version: "1.4.0" },
        ],
        [
          { name: "react", version: "19.1.0", cached: true },
          { name: "loose-envify", version: "1.4.0", cached: false },
        ]
      ),
      [
        {
          package_name: "loose-envify",
          version: "1.4.0",
          tarball_url: undefined,
        },
      ]
    );
  });

  it("removes selected rows that become cached or busy after resolution", () => {
    const deps: ParsedDependency[] = [
      { name: "react", version: "^19", tarball_url: null },
      { name: "vite", version: "~7.0", tarball_url: null },
      { name: "zod", version: "^4", tarball_url: null },
    ];
    const states = new Map<string, RowState>([
      [rowKey("react", "^19"), { status: "cached", resolvedVersion: "19.1.0" }],
      [rowKey("vite", "~7.0"), { status: "uncached", resolvedVersion: "7.0.4" }],
      [rowKey("zod", "^4"), { status: "resolving" }],
    ]);

    assert.deepEqual(pruneSelection(new Set([0, 1, 2]), deps, states), new Set([1]));
  });

  it("compares selections by value", () => {
    assert.equal(areSelectionsEqual(new Set([2, 1]), new Set([1, 2])), true);
    assert.equal(areSelectionsEqual(new Set([1]), new Set([1, 2])), false);
  });

  it("keeps the action bar visible while cache actions are running", () => {
    assert.equal(
      shouldShowActionBar({
        selectedSize: 0,
        resolving: true,
        caching: false,
        exporting: false,
      }),
      true
    );
    assert.equal(
      shouldShowActionBar({
        selectedSize: 0,
        resolving: false,
        caching: true,
        exporting: false,
      }),
      true
    );
    assert.equal(
      shouldShowActionBar({
        selectedSize: 0,
        resolving: false,
        caching: false,
        exporting: false,
      }),
      false
    );
  });

  it("keeps the action bar visible while export actions are running", () => {
    assert.equal(
      shouldShowActionBar({
        selectedSize: 0,
        resolving: false,
        caching: false,
        exporting: true,
      }),
      true
    );
  });

  it("disables context menu cache actions for rows that are already busy or cached", () => {
    assert.deepEqual(getContextMenuActionState("uncached", false), {
      cacheDisabled: false,
      exportDisabled: false,
    });
    assert.deepEqual(getContextMenuActionState("downloading", false), {
      cacheDisabled: true,
      exportDisabled: false,
    });
    assert.deepEqual(getContextMenuActionState("uploading", false), {
      cacheDisabled: true,
      exportDisabled: false,
    });
    assert.deepEqual(getContextMenuActionState("resolving", false), {
      cacheDisabled: true,
      exportDisabled: false,
    });
    assert.deepEqual(getContextMenuActionState("cached", false), {
      cacheDisabled: true,
      exportDisabled: false,
    });
  });

  it("disables all context menu actions while global import actions are busy", () => {
    assert.deepEqual(getContextMenuActionState("uncached", true), {
      cacheDisabled: true,
      exportDisabled: true,
    });
  });

  it("clears successfully resolved selected rows even when no cache task starts", () => {
    const deps: ParsedDependency[] = [
      { name: "react", version: "^19", tarball_url: null },
      { name: "missing-package", version: "^1", tarball_url: null },
    ];
    const resolved: ResolvedImportPackage[] = [
      {
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        tarball_url: null,
        cached: true,
      },
    ];

    assert.deepEqual(
      removeResolvedFromSelection(new Set([0, 1]), deps, resolved),
      new Set([1])
    );
  });

  it("keeps unresolved selected rows after caching with dependencies starts", () => {
    const deps: ParsedDependency[] = [
      { name: "react", version: "^19", tarball_url: null },
      { name: "missing-package", version: "^1", tarball_url: null },
      { name: "vite", version: "~7.0", tarball_url: null },
    ];
    const resolved: ResolvedImportPackage[] = [
      {
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        tarball_url: null,
        cached: true,
      },
      {
        name: "vite",
        raw_range: "~7.0",
        version: "7.0.4",
        tarball_url: null,
        cached: false,
      },
    ];

    assert.deepEqual(
      removeResolvedFromSelection(new Set([0, 1, 2]), deps, resolved),
      new Set([1])
    );
  });

  it("ignores resolve progress events from older import sessions", () => {
    const states = new Map<string, RowState>([
      [rowKey("react", "^19"), { status: "unknown" }],
    ]);

    const next = applyResolveProgress(states, {
      currentRequestId: "current",
      payload: {
        request_id: "old",
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        cached: true,
        error: null,
      },
    });

    assert.equal(next, states);
    assert.equal(next.get(rowKey("react", "^19"))?.status, "unknown");
  });

  it("identifies stale resolve command responses", () => {
    assert.equal(isCurrentResolveRequest("current", "old"), false);
    assert.equal(isCurrentResolveRequest("current", "current"), true);
    assert.equal(isCurrentResolveRequest(null, "old"), false);
  });

  it("applies resolved packages when progress events arrive before listener registration", () => {
    const states = new Map<string, RowState>([
      [rowKey("react", "^19"), { status: "resolving" }],
    ]);

    const next = applyResolvedPackages(states, [
      {
        name: "react",
        raw_range: "^19",
        version: "19.1.0",
        tarball_url: null,
        cached: true,
      },
    ]);

    assert.deepEqual(next.get(rowKey("react", "^19")), {
      status: "cached",
      error: undefined,
      resolvedVersion: "19.1.0",
    });
  });

  it("does not apply resolved command responses from stale requests", () => {
    const states = new Map<string, RowState>([
      [rowKey("react", "^19"), { status: "resolving" }],
    ]);

    const next = applyResolvedPackagesForCurrentRequest(states, {
      currentRequestId: "current",
      responseRequestId: "old",
      resolved: [
        {
          name: "react",
          raw_range: "^19",
          version: "19.1.0",
          tarball_url: null,
          cached: true,
        },
      ],
    });

    assert.equal(next, states);
    assert.deepEqual(next.get(rowKey("react", "^19")), {
      status: "resolving",
    });
  });

  it("rejects export when any selected package cannot be resolved", () => {
    assert.throws(
      () =>
        exportPackagesFromResolvedSelection({
          alreadyResolved: [{ package_name: "react", version: "19.1.0" }],
          pending: [
            { name: "vite", version: "^7", tarball_url: null },
            { name: "missing-package", version: "^1", tarball_url: null },
          ],
          resolved: [
            {
              name: "vite",
              raw_range: "^7",
              version: "7.0.4",
              tarball_url: null,
              cached: false,
            },
          ],
        }),
      /missing-package@\^1/
    );
  });

  it("rejects single package export when resolution returns no concrete version", () => {
    assert.throws(
      () => getResolvedVersionOrThrow("missing-package", "^1", []),
      /missing-package@\^1/
    );
  });

  it("builds export packages only after all pending packages resolve", () => {
    assert.deepEqual(
      exportPackagesFromResolvedSelection({
        alreadyResolved: [{ package_name: "react", version: "19.1.0" }],
        pending: [{ name: "vite", version: "^7", tarball_url: null }],
        resolved: [
          {
            name: "vite",
            raw_range: "^7",
            version: "7.0.4",
            tarball_url: null,
            cached: false,
          },
        ],
      }),
      [
        { package_name: "react", version: "19.1.0" },
        { package_name: "vite", version: "7.0.4" },
      ]
    );
  });

  it("keeps context menus and submenus inside the viewport", () => {
    assert.deepEqual(
      positionContextMenu({
        anchor: { x: 760, y: 560 },
        menuSize: { width: 180, height: 120 },
        submenuSize: { width: 210, height: 80 },
        viewport: { width: 800, height: 600 },
        gap: 4,
      }),
      {
        left: 576,
        top: 436,
        submenuSide: "left",
      }
    );
  });
});
