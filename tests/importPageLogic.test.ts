import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  applyResolveProgress,
  applyResolvedPackages,
  areSelectionsEqual,
  cacheTaskInputsFromResolved,
  cacheTaskInputsFromDependencies,
  dependencyRootsFromResolved,
  isCurrentResolveRequest,
  pruneSelection,
  removeResolvedFromSelection,
  rowKey,
  shouldShowActionBar,
  type ParsedDependency,
  type ResolvedImportPackage,
  type RowState,
} from "../src/pages/importPageLogic.ts";

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
});
