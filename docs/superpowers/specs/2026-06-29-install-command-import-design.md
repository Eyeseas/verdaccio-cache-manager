# Install Command Import Design

## Summary

Add an installation-command input mode to the existing Import page. Users can paste commands such as `bun install -g @oh-my-pi/pi-coding-agent` or `npm install react@19 vite`, parse the root package names, review them in the existing import list, then explicitly resolve the full dependency tree.

The feature reuses the current import flow instead of creating a new page: package rows, cache status checks, selection behavior, cache tasks, dependency resolution, tarball export, and context menu actions stay in one place.

## Goals

- Support npm, pnpm, yarn, and bun install/add commands.
- Support both global and local install commands.
- Support multiple root packages in one command.
- Support common npm registry package specs:
  - `react`
  - `react@19`
  - `react@^19`
  - `@scope/pkg`
  - `@scope/pkg@latest`
- Show the current Node version at the top of the parsed result.
- Let users explicitly trigger full dependency resolution after root packages are shown.
- Mark root packages in the list after full dependency resolution.

## Non-Goals

- Do not execute pasted scripts or shell commands.
- Do not support npm aliases, git URLs, local file/link specs, local paths, or tarball URLs.
- Do not parse every install command in a multi-line script. The first recognized install command is parsed; additional recognized commands produce a warning.
- Do not show npm, pnpm, yarn, or bun versions in the environment header for this iteration.
- Do not create a separate page or duplicate the existing import task system.

## Existing Context

The Import page already supports dependency-file import via `package.json`, `pnpm-lock.yaml`, and `package-lock.json`.

Existing commands and utilities to reuse:

- `parse_file`: parses dependency files into `ParsedDependency[]`.
- `resolve_package_versions`: resolves package ranges such as `latest` or `^19` into concrete versions and emits row progress.
- `resolve_dependencies`: recursively resolves dependency trees from concrete root versions.
- `check_cached_status`: marks packages as cached or uncached.
- `start_cache_tasks`: starts cache/upload tasks.
- `importPageLogic.ts`: contains pure row-state and selection helpers.

## User Flow

1. User opens the Import page.
2. Empty state offers two input modes: dependency file and install command.
3. User selects install command mode.
4. User pastes an install command and clicks "解析命令".
5. The app parses root packages locally and enters the existing import result list.
6. The header shows:
   - source badge: `安装命令`
   - Node version: `Node v22.x.x`, or `Node 未检测到` if unavailable
   - dependency count and uncached count
7. Root packages appear in the list with a `根包` badge.
8. User may cache/export just the root packages using existing actions.
9. User may click "解析完整依赖".
10. The app resolves root package versions, resolves the full dependency tree, merges the full list, preserves root markers, checks cache status, and selects uncached packages by default.

## Architecture

### Frontend

Extend `src/pages/ImportPage.tsx` with a second empty-state input mode. Keep the result list and action bar shared between file imports and command imports.

Add pure parsing and merge helpers to `src/pages/importPageLogic.ts`:

- `parseInstallCommand(input: string): InstallCommandParseResult`
- `mergeResolvedDependencyList(args): ParsedDependency[]`
- root marker helpers based on stable row keys

The parser must be a local string parser. It must not execute shell content or use the system shell.

### Backend

Add a small Tauri command:

- `get_node_version() -> Result<String, String>`

It runs `node -v`, trims stdout, and returns the version string. Failure is expected on systems without Node and is handled as a non-blocking UI state.

No backend package parser is needed for install commands in this iteration.

## Data Model

Existing `ParsedDependency` remains the row source:

```ts
interface ParsedDependency {
  name: string;
  version: string;
  tarball_url: string | null;
}
```

Install command parsing returns:

```ts
type PackageManager = "npm" | "pnpm" | "yarn" | "bun";

interface InstallCommandParseResult {
  manager: PackageManager;
  command: "install" | "i" | "add";
  global: boolean;
  packages: ParsedDependency[];
  warnings: string[];
}
```

Root package identity is tracked outside `ParsedDependency` to avoid changing the Rust-facing data contract. Use `rowKey(name, version)` for the initially parsed root specs and a separate `Set<string>` for root markers. After a root package is resolved from `latest` or a range to a concrete version, carry the root marker forward to `rowKey(name, resolvedVersion)`.

## Parsing Rules

Supported command forms:

- `npm install <pkg...>`
- `npm i <pkg...>`
- `pnpm install <pkg...>`
- `pnpm add <pkg...>`
- `yarn add <pkg...>`
- `bun install <pkg...>`
- `bun add <pkg...>`

Global flags:

- `-g`
- `--global`

Global flags are recorded but do not change dependency resolution.

Ignored option examples:

- `--save-dev`
- `-D`
- `--registry=...`
- `--ignore-scripts`
- `--frozen-lockfile`

Options that consume a following value should skip that value, such as `--registry https://registry.npmjs.org`.

Supported package specs:

- Unscoped name with no version: `react` becomes `react@latest`.
- Unscoped name with version/range/tag: `react@19`, `react@^19`, `react@latest`.
- Scoped name with no version: `@scope/pkg` becomes `@scope/pkg@latest`.
- Scoped name with version/range/tag: `@scope/pkg@latest`, `@scope/pkg@^1`.

Unsupported package specs fail the parse with a specific message:

- `npm:alias`
- `git+https://...`
- `github:user/repo`
- `file:...`
- `link:...`
- local paths such as `../pkg` or `./pkg`
- tarball URLs
- raw `.tgz` paths

If a command mixes supported and unsupported package specs, parsing fails as a whole. This prevents silently producing an incomplete install set.

## Dependency Resolution Flow

### Parse Command

When the user clicks "解析命令":

1. Clear previous import state.
2. Parse the command locally.
3. If no packages are found, show an error.
4. Store packages in `parsedDeps`.
5. Store root package keys.
6. Set source label to `安装命令`.
7. Fetch Node version in parallel or immediately after parse.
8. Initialize row state as `unknown`.
9. Run `checkCachedStatus(parsedDeps)`.
10. Default-select uncached rows using the existing selection initialization behavior.

### Resolve Full Dependencies

When the user clicks "解析完整依赖":

1. Resolve root package specs through `resolve_package_versions`.
2. If any root package cannot be resolved, mark its row as `resolve-failed` and keep the current list.
3. Convert resolved root packages to dependency roots.
4. Call `resolveDependencies(roots)`.
5. Convert the returned dependency tree into `ParsedDependency[]`.
6. Merge with resolved root packages.
7. Deduplicate by `name@version`.
8. Preserve root markers on resolved root rows.
9. Replace the list with the merged full list.
10. Run `check_cached_status` for all rows.
11. Select uncached rows by default.

The existing "缓存包及依赖" action remains available. It can still resolve dependencies and start cache tasks directly for selected rows, preserving current behavior.

## UI Behavior

Empty state:

- Use a compact input-mode switch: dependency file / install command.
- Dependency file mode keeps the current drag-and-drop and file picker behavior.
- Install command mode shows a textarea and a parse button.
- The supported-command hint should be concise and not replace validation.

Result header:

- Source badge: file name for file imports, `安装命令` for command imports.
- Node badge for command imports only:
  - `Node v22.x.x`
  - `Node 未检测到`
- Counts remain visible: total dependency count and uncached count.
- Add a "解析完整依赖" button for command imports after root packages are parsed.

Rows:

- Root packages show a `根包` badge.
- Version rendering continues to show resolved versions when `resolve_package_versions` updates row state.
- Existing row status badges remain unchanged.

## Error Handling

- No recognized install command: `未找到支持的 npm/pnpm/yarn/bun 安装命令`
- Recognized command but no packages: `安装命令中未包含包名`
- Unsupported package specs: include the unsupported spec list in the message.
- Multiple recognized install commands: parse the first and show a warning.
- Node detection failure: show `Node 未检测到`; do not block parsing or caching.
- Full dependency resolution failure: show a toast or page error, keep the root package list intact.
- Stale async responses: continue using request IDs to ignore outdated version resolution results.

## Testing

Add unit tests for `parseInstallCommand` in `tests/importPageLogic.test.ts`:

- npm local install with one package.
- npm global install with `-g`.
- bun global install with scoped package.
- pnpm/yarn add commands.
- multiple root packages.
- scoped package with and without version.
- range/tag versions.
- ignored flags before, between, and after packages.
- option with following value.
- unsupported alias/git/file/link/path/tarball specs.
- multi-line input with first recognized command and warning for additional commands.

Add unit tests for dependency-list merge helpers:

- root package markers are preserved after `latest` resolves to a concrete version.
- transitive dependencies are added.
- duplicate `name@version` rows are deduplicated.
- root package wins when a transitive dependency duplicates a root package.

Verification commands:

- `pnpm test`
- `pnpm test:rust` if the `get_node_version` command is added in Rust during implementation.

## Open Decisions

All product decisions for this design are resolved:

- Default behavior is two-stage: parse roots first, resolve full dependencies on explicit action.
- Both global and local install commands are supported.
- Multiple root packages are supported.
- Environment display is Node version only.
- Root and transitive packages share one list, with root packages marked.
- Common registry package specs are supported; complex non-registry specs are out of scope.
