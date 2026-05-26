# package.json Downgrade Design

Date: 2026-05-26

## Goal

Add a dedicated package.json downgrade workflow for offline/internal-network projects.
The user can import a project's `package.json`, compare its dependency versions against
the local Verdaccio cache index, and generate a downgraded `package.json` that prefers
already cached versions.

Primary scenario: the internal Verdaccio cache contains `react@18.3.0`, but a newly
imported project declares `react@18.4.0`. The generated file should use `18.3.0` so
`npm i` can succeed against the internal registry.

## Scope

In scope:

- Add a new sidebar entry and route for the downgrade workflow.
- Accept only `package.json` as input for this feature.
- Analyze and rewrite these dependency sections:
  - `dependencies`
  - `devDependencies`
  - `optionalDependencies`
- Use the local SQLite cache index as the source of available versions.
- Default to safe same-major downgrades.
- Provide an optional "allow cross-major downgrade" switch.
- Generate a new `package.json` by default.
- Support overwriting the original file only after explicit confirmation.
- Create a backup before overwriting the original file.

Out of scope for the first implementation:

- Rewriting `package-lock.json`, `pnpm-lock.yaml`, `yarn.lock`, or other lockfiles.
- Modifying `peerDependencies`, `overrides`, `resolutions`, or `pnpm.overrides`.
- Manually selecting target versions per dependency.
- Fetching public npm metadata during downgrade analysis.
- Preserving original JSON formatting exactly.

## Chosen Approach

Use a Rust/Tauri command for the core analysis and rewrite, with React focused on
file selection, preview, filtering, confirmation, and save/overwrite actions.

Reasons:

- The authoritative cache index is already in Rust-side SQLite.
- Rust already depends on `node-semver`, which should be reused for version matching.
- File writes and backup behavior are safer in Tauri commands than browser-side code.
- The new workflow has different state and intent from the existing Import page, so it
  should not be folded into `ImportPage`.

## UI Design

Add `PackageJsonDowngradePage` at `/downgrade`.

Sidebar:

- Add a new entry labelled `降级`.
- Keep existing entries and sync button behavior.

Empty state:

- Large drop zone and "选择 package.json" button.
- Accept drag/drop and dialog selection.
- Reject files whose basename is not `package.json`.
- Show cache index status. If the cache index is empty or unavailable, show a warning and
  a "同步缓存索引" action that reuses existing sync store behavior.

Analysis view:

- Header area:
  - file name
  - selected strategy label
  - `允许跨 major 降级` switch
  - `重新选择` action
- Summary cards:
  - total dependencies
  - changed dependencies
  - unchanged cached dependencies
  - missing cache entries
  - cross-major changes when applicable
- Main list:
  - package name
  - dependency section
  - original spec
  - resolved/current requested version when available
  - target version
  - status badge
- Filters:
  - all
  - changed
  - missing/skipped
- Right-side preview:
  - generated `package.json` content
  - "另存为新 package.json" primary path
  - "确认后覆盖原文件" destructive path

Default output action:

- Save as a new file.
- Suggested filename: `package.downgraded.json`.

Overwrite action:

- Requires an explicit confirmation dialog.
- The dialog must state that the original `package.json` will be overwritten and a backup
  will be created first.

## Tauri Commands

### `analyze_package_json_downgrade`

Input:

```ts
{
  filePath: string;
  allowMajorDowngrade: boolean;
  requestId?: string;
}
```

Output:

```ts
interface DowngradeAnalysis {
  request_id?: string;
  file_path: string;
  file_name: string;
  allow_major_downgrade: boolean;
  original_content: string;
  updated_content: string;
  items: DowngradeItem[];
  summary: DowngradeSummary;
  cache_index_empty: boolean;
}

interface DowngradeItem {
  name: string;
  section: "dependencies" | "devDependencies" | "optionalDependencies";
  original_spec: string;
  original_resolved_version: string | null;
  target_version: string | null;
  cached_versions: string[];
  status:
    | "unchanged-cached"
    | "rewritten-cached"
    | "downgraded"
    | "major-downgraded"
    | "missing-cache"
    | "unsupported-spec"
    | "invalid-range";
  reason: string;
}

interface DowngradeSummary {
  total: number;
  changed: number;
  rewritten_cached: number;
  unchanged_cached: number;
  missing_cache: number;
  unsupported: number;
  invalid: number;
  major_downgraded: number;
}
```

Behavior:

- Read the package file from disk.
- Validate basename equals `package.json`.
- Parse JSON with `serde_json`.
- Read cached versions from the local cache index.
- Analyze each dependency string in the three supported sections.
- Return both original and generated content.
- Do not write files.

### `save_downgraded_package_json`

Input:

```ts
{
  outputPath: string;
  content: string;
}
```

Behavior:

- Write `content` to `outputPath`.
- Return the output path on success.

### `overwrite_package_json`

Input:

```ts
{
  filePath: string;
  content: string;
}
```

Output:

```ts
{
  file_path: string;
  backup_path: string;
}
```

Behavior:

- Validate basename equals `package.json`.
- Create a timestamped backup in the same directory before writing.
- If backup creation fails, abort and do not overwrite.
- Write generated content to the original file only after backup succeeds.

## Version Matching

Supported dependency specs:

- Exact versions such as `18.4.0`.
- Semver ranges such as `^18.4.0`, `~18.4.0`, `>=18 <19`.

Unsupported specs:

- `file:`
- `workspace:`
- `git:`
- `git+`
- `git@`
- `http://`
- `https://`
- `link:`
- `npm:` alias
- empty string
- `*`
- `latest`

Selection algorithm:

1. Get the cached versions for the package.
2. If the original spec is unsupported, keep it and mark `unsupported-spec`.
3. If no cached versions exist, keep the original spec and mark `missing-cache`.
4. If the spec is a valid semver range, first select the highest cached version satisfying
   that range.
5. If step 4 finds a version, output that exact version.
   - If the original spec was already the same exact version, mark `unchanged-cached`.
   - If the original spec was a range or a different same-major exact version, mark
     `rewritten-cached`. This is a content change, but not necessarily a downgrade.
   - If the selected version is lower than the original exact version in the same major,
     mark `downgraded`.
   - If the selected version differs by major and the switch is enabled, mark
     `major-downgraded`.
6. If no cached version satisfies the range, use the downgrade fallback:
   - Default: infer a major from the exact version or semver lower bound, then select the
     highest cached version in that major.
   - With cross-major enabled: select the highest cached version for the package.
7. If same-major fallback cannot infer a major or finds no target, keep the original spec
   and mark `missing-cache`.
8. All changed values are written as exact versions without `^` or `~`.

Examples:

- Original `18.4.0`, cached `18.3.0` -> `18.3.0`, `downgraded`.
- Original `^18.4.0`, cached `18.3.0` -> `18.3.0`, `downgraded`.
- Original `^18.0.0`, cached `18.3.0` -> `18.3.0`, `rewritten-cached`.
- Original `18.4.0`, cached `18.5.0` -> `18.5.0`, `rewritten-cached`.
- Original `^18.4.0`, cached `17.9.0`, cross-major off -> unchanged, `missing-cache`.
- Original `^18.4.0`, cached `17.9.0`, cross-major on -> `17.9.0`, `major-downgraded`.

## JSON Rewriting

- Modify only string values in supported dependency sections.
- Preserve unsupported and missing-cache entries unchanged.
- Preserve all package.json fields outside supported sections.
- Output pretty JSON with two-space indentation.
- Exact original formatting preservation is not required in this version.
- The implementation should avoid reordering fields if feasible, but stable pretty output is
  more important than introducing a new parser solely for formatting preservation.

## Error Handling

- Non-`package.json` input: show a clear validation error.
- Invalid JSON: show parse error and do not generate preview.
- Empty cache index: analyze anyway, but show all relevant items as missing and prompt sync.
- Save failure: show the error and keep current analysis visible.
- Backup failure before overwrite: show the error and do not overwrite.
- Overwrite failure after backup succeeds: show both the error and backup path.
- Strategy toggle or file reselect during analysis: ignore stale responses by request id.

## Tests

Rust tests:

- Same-major fallback selects highest cached version.
- Cross-major fallback selects highest cached version.
- Range-satisfying cached version wins before fallback.
- Range spec rewritten to an exact cached version is reported as `rewritten-cached`.
- Missing cache keeps original spec.
- Unsupported specs are skipped.
- Only `dependencies`, `devDependencies`, and `optionalDependencies` are changed.
- `peerDependencies`, `overrides`, `resolutions`, and `pnpm.overrides` remain unchanged.
- Save command writes content to the selected path.
- Overwrite command creates backup before writing.
- Backup failure prevents overwrite.

Frontend tests:

- Summary counts render from an analysis result.
- Filter buttons show changed and missing/skipped subsets.
- Cross-major switch triggers a new analysis.
- Overwrite action requires confirmation before calling the command.
- Save action uses suggested filename `package.downgraded.json`.

Verification commands:

```bash
pnpm test
pnpm tsc --noEmit
pnpm test:rust
```
