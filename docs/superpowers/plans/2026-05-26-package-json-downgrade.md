# package.json Downgrade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a dedicated workflow that imports a `package.json`, rewrites selected dependency versions to already cached Verdaccio versions, and saves or safely overwrites the result.

**Architecture:** Put the analysis, version matching, JSON rewrite, save, and overwrite/backup behavior in Rust. Keep React responsible for file selection, strategy toggling, preview, filtering, confirmation, and invoking Tauri commands. Add a focused frontend logic module for view filtering and summary labels rather than extending the existing Import page state machine.

**Tech Stack:** Tauri v2, Rust, `node-semver`, `serde_json`, React 19, TypeScript, zustand, shadcn/base-ui components, Vitest, Rust unit tests.

---

## File Structure

- Create `src-tauri/src/package_downgrade/mod.rs`: core downgrade analysis, JSON rewrite, version matching, save, overwrite, and Rust tests.
- Modify `src-tauri/src/lib.rs`: register the new module and expose Tauri commands.
- Create `src/pages/packageJsonDowngradeLogic.ts`: frontend pure helpers for filters, status labels, counts, and stale response checks.
- Create `src/pages/packageJsonDowngradeLogic.test.ts`: Vitest tests for frontend helpers.
- Create `src/pages/PackageJsonDowngradePage.tsx`: new page UI.
- Create `src/pages/PackageJsonDowngradePage.test.tsx`: interaction tests for save, strategy toggle, filters, and overwrite confirmation.
- Modify `src/App.tsx`: add `/downgrade` route.
- Modify `src/components/layout/Sidebar.tsx`: add `降级` navigation item.

## Task 1: Rust Core Analysis

**Files:**
- Create: `src-tauri/src/package_downgrade/mod.rs`
- Test: Rust tests in the same file

- [ ] **Step 1: Write failing Rust tests for version matching and JSON rewrite**

Add tests covering:

```rust
#[test]
fn same_major_fallback_downgrades_to_highest_cached_same_major() {
    let mut cached = std::collections::HashMap::new();
    cached.insert("react".to_string(), vec!["18.2.0".to_string(), "18.3.0".to_string()]);
    let input = r#"{"dependencies":{"react":"18.4.0"}}"#;

    let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

    assert_eq!(analysis.summary.changed, 1);
    assert_eq!(analysis.items[0].status, DowngradeStatus::Downgraded);
    assert_eq!(analysis.items[0].target_version.as_deref(), Some("18.3.0"));
    assert!(analysis.updated_content.contains("\"react\": \"18.3.0\""));
}

#[test]
fn cross_major_fallback_uses_highest_cached_when_enabled() {
    let mut cached = std::collections::HashMap::new();
    cached.insert("react".to_string(), vec!["17.9.0".to_string()]);
    let input = r#"{"dependencies":{"react":"^18.4.0"}}"#;

    let analysis = analyze_content("package.json", input, &cached, true, None).unwrap();

    assert_eq!(analysis.items[0].status, DowngradeStatus::MajorDowngraded);
    assert_eq!(analysis.items[0].target_version.as_deref(), Some("17.9.0"));
}

#[test]
fn range_satisfying_cached_version_is_rewritten_cached() {
    let mut cached = std::collections::HashMap::new();
    cached.insert("vite".to_string(), vec!["7.0.4".to_string()]);
    let input = r#"{"devDependencies":{"vite":"^7.0.0"}}"#;

    let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

    assert_eq!(analysis.items[0].status, DowngradeStatus::RewrittenCached);
    assert!(analysis.updated_content.contains("\"vite\": \"7.0.4\""));
}

#[test]
fn unsupported_specs_and_unsupported_sections_remain_unchanged() {
    let mut cached = std::collections::HashMap::new();
    cached.insert("local-lib".to_string(), vec!["1.0.0".to_string()]);
    cached.insert("peer-lib".to_string(), vec!["1.0.0".to_string()]);
    let input = r#"{
      "dependencies":{"local-lib":"file:../local-lib"},
      "peerDependencies":{"peer-lib":"2.0.0"},
      "overrides":{"peer-lib":"2.0.0"}
    }"#;

    let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

    assert_eq!(analysis.items[0].status, DowngradeStatus::UnsupportedSpec);
    assert!(analysis.updated_content.contains("\"local-lib\": \"file:../local-lib\""));
    assert!(analysis.updated_content.contains("\"peer-lib\": \"2.0.0\""));
}

#[test]
fn missing_cache_keeps_original_spec() {
    let cached = std::collections::HashMap::new();
    let input = r#"{"optionalDependencies":{"fsevents":"^2.3.3"}}"#;

    let analysis = analyze_content("package.json", input, &cached, false, None).unwrap();

    assert_eq!(analysis.items[0].status, DowngradeStatus::MissingCache);
    assert!(analysis.updated_content.contains("\"fsevents\": \"^2.3.3\""));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml package_downgrade
```

Expected: compile failure because `package_downgrade` and `analyze_content` do not exist.

- [ ] **Step 3: Implement minimal Rust core**

Implement:

- `DowngradeStatus` with serde rename values matching the design.
- `DowngradeItem`, `DowngradeSummary`, `DowngradeAnalysis`.
- `analyze_content(file_name, content, cached_map, allow_major_downgrade, request_id)`.
- Internal helpers:
  - `is_supported_package_file`
  - `unsupported_spec_reason`
  - `highest_version`
  - `highest_satisfying`
  - `infer_major`
  - `analyze_dependency`
  - `recompute_summary`

Use `node_semver::Version` and `node_semver::Range`.

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml package_downgrade
```

Expected: package downgrade tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/package_downgrade/mod.rs
git commit -m "feat: add package json downgrade analyzer"
```

## Task 2: Rust Tauri Commands

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/package_downgrade/mod.rs`

- [ ] **Step 1: Write failing Rust tests for save and overwrite**

Add tests:

```rust
#[test]
fn save_downgraded_content_writes_selected_file() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("package.downgraded.json");

    let written = save_content_to_path(&output, "{\"dependencies\":{}}\n").unwrap();

    assert_eq!(written, output);
    assert_eq!(std::fs::read_to_string(output).unwrap(), "{\"dependencies\":{}}\n");
}

#[test]
fn overwrite_package_json_creates_backup_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let package_json = dir.path().join("package.json");
    std::fs::write(&package_json, "{\"dependencies\":{\"react\":\"18.4.0\"}}\n").unwrap();

    let result = overwrite_with_backup(&package_json, "{\"dependencies\":{\"react\":\"18.3.0\"}}\n").unwrap();

    assert_eq!(std::fs::read_to_string(&package_json).unwrap(), "{\"dependencies\":{\"react\":\"18.3.0\"}}\n");
    assert!(result.backup_path.ends_with(".bak"));
    assert!(std::path::Path::new(&result.backup_path).exists());
    assert_eq!(
        std::fs::read_to_string(result.backup_path).unwrap(),
        "{\"dependencies\":{\"react\":\"18.4.0\"}}\n"
    );
}

#[test]
fn overwrite_rejects_non_package_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("other.json");
    std::fs::write(&path, "{}").unwrap();

    let err = overwrite_with_backup(&path, "{}").unwrap_err();

    assert!(err.contains("package.json"));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml package_downgrade
```

Expected: compile failure because save/overwrite helpers do not exist.

- [ ] **Step 3: Implement save/overwrite helpers and commands**

In `package_downgrade/mod.rs` implement:

- `SavePathResult { output_path }`
- `OverwriteResult { file_path, backup_path }`
- `save_content_to_path`
- `overwrite_with_backup`
- `cached_map_from_packages`

In `lib.rs`:

- Add `mod package_downgrade;`.
- Add command `analyze_package_json_downgrade`.
- Add command `save_downgraded_package_json`.
- Add command `overwrite_package_json`.
- Register all three in `tauri::generate_handler!`.

- [ ] **Step 4: Run Rust tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml package_downgrade
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/package_downgrade/mod.rs
git commit -m "feat: expose package json downgrade commands"
```

## Task 3: Frontend Pure Logic

**Files:**
- Create: `src/pages/packageJsonDowngradeLogic.ts`
- Create: `src/pages/packageJsonDowngradeLogic.test.ts`

- [ ] **Step 1: Write failing frontend logic tests**

Create tests for:

```ts
import { describe, expect, it } from "vitest";
import {
  filterDowngradeItems,
  isStaleAnalysis,
  statusLabel,
  type DowngradeAnalysis,
} from "./packageJsonDowngradeLogic";

const analysis: DowngradeAnalysis = {
  request_id: "req-2",
  file_path: "/tmp/package.json",
  file_name: "package.json",
  allow_major_downgrade: false,
  original_content: "{}",
  updated_content: "{}",
  cache_index_empty: false,
  summary: {
    total: 3,
    changed: 1,
    rewritten_cached: 1,
    unchanged_cached: 1,
    missing_cache: 1,
    unsupported: 0,
    invalid: 0,
    major_downgraded: 0,
  },
  items: [
    { name: "react", section: "dependencies", original_spec: "18.4.0", original_resolved_version: "18.4.0", target_version: "18.3.0", cached_versions: ["18.3.0"], status: "downgraded", reason: "" },
    { name: "vite", section: "devDependencies", original_spec: "^7.0.0", original_resolved_version: null, target_version: "7.0.4", cached_versions: ["7.0.4"], status: "rewritten-cached", reason: "" },
    { name: "missing", section: "optionalDependencies", original_spec: "^1.0.0", original_resolved_version: null, target_version: null, cached_versions: [], status: "missing-cache", reason: "" },
  ],
};

describe("packageJsonDowngradeLogic", () => {
  it("filters changed and missing items", () => {
    expect(filterDowngradeItems(analysis.items, "changed").map((i) => i.name)).toEqual(["react", "vite"]);
    expect(filterDowngradeItems(analysis.items, "missing").map((i) => i.name)).toEqual(["missing"]);
  });

  it("labels statuses for display", () => {
    expect(statusLabel("downgraded")).toBe("降级");
    expect(statusLabel("major-downgraded")).toBe("跨 major");
    expect(statusLabel("missing-cache")).toBe("无缓存");
  });

  it("detects stale analysis responses", () => {
    expect(isStaleAnalysis("req-3", analysis)).toBe(true);
    expect(isStaleAnalysis("req-2", analysis)).toBe(false);
  });
});
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
pnpm vitest run src/pages/packageJsonDowngradeLogic.test.ts
```

Expected: fail because the logic module does not exist.

- [ ] **Step 3: Implement frontend logic module**

Implement exported types and helpers:

- `DowngradeStatus`
- `DowngradeItem`
- `DowngradeSummary`
- `DowngradeAnalysis`
- `DowngradeFilter`
- `createDowngradeRequestId`
- `isPackageJsonPath`
- `filterDowngradeItems`
- `statusLabel`
- `statusVariant`
- `isChangedStatus`
- `isMissingOrSkippedStatus`
- `isStaleAnalysis`

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
pnpm vitest run src/pages/packageJsonDowngradeLogic.test.ts
```

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/pages/packageJsonDowngradeLogic.ts src/pages/packageJsonDowngradeLogic.test.ts
git commit -m "feat: add package json downgrade frontend logic"
```

## Task 4: Frontend Page, Route, and Sidebar

**Files:**
- Create: `src/pages/PackageJsonDowngradePage.tsx`
- Create: `src/pages/PackageJsonDowngradePage.test.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/layout/Sidebar.tsx`

- [ ] **Step 1: Write failing page tests**

Create tests that mock Tauri `invoke`, dialog `open`/`save`, sync store, and sonner toast. Cover:

- selecting a package file calls `analyze_package_json_downgrade`
- toggling cross-major calls analysis again with `allowMajorDowngrade: true`
- changed filter hides missing-only rows
- save button opens save dialog with `package.downgraded.json` and invokes `save_downgraded_package_json`
- overwrite button opens confirmation and only calls `overwrite_package_json` after confirm

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
pnpm vitest run src/pages/PackageJsonDowngradePage.test.tsx
```

Expected: fail because the page does not exist.

- [ ] **Step 3: Implement page UI**

Implement:

- file selection with `open({ multiple: false, filters: [{ name: "package.json", extensions: ["json"] }] })`
- drag/drop using `useTauriFileDrop`
- `analyzePackage(filePath, allowMajorDowngrade)` wrapper with request id guard
- cache index warning using `analysis.cache_index_empty`
- switch as a checkbox-style control for cross-major strategy
- summary cards
- item table/list
- filter buttons
- right-side preview
- save action through `save`
- overwrite confirmation modal local to this page
- toast success/error paths

- [ ] **Step 4: Add route and sidebar item**

Modify `App.tsx`:

```tsx
<Route path="/downgrade" element={<PackageJsonDowngradePage />} />
```

Modify `Sidebar.tsx` nav items:

```tsx
{ to: "/downgrade", icon: PackageCheck, label: "降级" }
```

Use a lucide icon already available from `lucide-react`, such as `PackageCheck` or `FileDown`.

- [ ] **Step 5: Run page tests**

Run:

```bash
pnpm vitest run src/pages/PackageJsonDowngradePage.test.tsx
```

Expected: pass.

- [ ] **Step 6: Commit**

```bash
git add src/pages/PackageJsonDowngradePage.tsx src/pages/PackageJsonDowngradePage.test.tsx src/App.tsx src/components/layout/Sidebar.tsx
git commit -m "feat: add package json downgrade page"
```

## Task 5: Full Verification

**Files:**
- Potential fixes from verification only

- [ ] **Step 1: Run frontend tests**

```bash
pnpm test
```

Expected: pass.

- [ ] **Step 2: Run type check**

```bash
pnpm tsc --noEmit
```

Expected: pass.

- [ ] **Step 3: Run Rust tests**

```bash
pnpm test:rust
```

Expected: pass.

- [ ] **Step 4: Commit fixes if any**

If verification required changes:

```bash
git add <changed-files>
git commit -m "fix: stabilize package json downgrade workflow"
```

- [ ] **Step 5: Report final status**

Summarize:

- files changed
- tests run
- any known limitations from the design

