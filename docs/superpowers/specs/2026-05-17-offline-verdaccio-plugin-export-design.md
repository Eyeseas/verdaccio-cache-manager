# Offline Verdaccio Plugin Export Design

## Goal

Add a Settings page action that exports the bundled `verdaccio-cached-list` Verdaccio middleware plugin as an npm-installable `.tgz` package, so users can install the plugin on an offline Verdaccio host without separately downloading it.

## Scope

The feature exports a local plugin package from the desktop app. It does not fetch the plugin from npm, GitHub, or any other network source. The exported file must be installable with npm from a local path:

```bash
npm install -g /path/to/verdaccio-cached-list-0.1.0.tgz
```

The first implementation will export the existing `verdaccio-cached-list` package only.

## User Experience

Settings gets a small Verdaccio plugin section near the Registry settings because the plugin supports cache index discovery via `/-/cached-packages`.

The section contains:

- A short description that the plugin helps list proxy-cached packages from Verdaccio.
- A `导出插件包` button.
- A save-file dialog defaulting to `verdaccio-cached-list-0.1.0.tgz`.
- A success toast that includes the saved path and local install command.
- An error toast that shows the underlying failure message.

Button copy uses "导出" instead of "下载" because the operation is offline and copies a bundled file out of the app.

## Architecture

The app bundles a prebuilt npm tarball for `verdaccio-cached-list` as a Tauri resource. The Settings page calls a Tauri command with the user-selected output path. The command resolves the resource from the app bundle and copies it to the selected path.

The implementation has three parts:

- Package artifact: generate or maintain `src-tauri/resources/verdaccio-cached-list-0.1.0.tgz` from `verdaccio-cached-list/`.
- Tauri config: include that tarball in `src-tauri/tauri.conf.json` under bundle resources.
- Tauri command: expose `export_verdaccio_plugin(output_path)` and return the final saved path.

## Data Flow

1. User clicks `导出插件包`.
2. Frontend opens a save dialog with a `.tgz` filename.
3. If the user cancels, nothing happens.
4. Frontend invokes `export_verdaccio_plugin` with the selected path.
5. Rust command resolves the bundled tarball from Tauri resources.
6. Rust copies the tarball to the selected path.
7. Frontend displays a success toast with the installation command.

## Error Handling

The frontend disables the button while export is running. Cancellation is silent.

The Rust command returns clear errors for:

- Missing bundled plugin resource.
- Failing to create the output parent directory.
- Failing to copy the tarball.

The frontend surfaces those errors in a toast.

## Testing

Frontend tests cover:

- Clicking the export button opens a save dialog with the expected `.tgz` filename.
- A selected path invokes the Tauri command with that path.
- Canceling the save dialog does not invoke the command.
- A successful export displays the installation command.

Rust tests cover pure helper behavior such as the bundled tarball filename and output filename constants. The Tauri resource resolution itself is verified through build/type checks because it depends on app runtime context.

Verification commands:

```bash
pnpm test
pnpm tsc --noEmit
pnpm test:rust
pnpm build
```

## Non-Goals

This feature does not install the plugin into Verdaccio automatically, edit Verdaccio `config.yaml`, restart Verdaccio, publish the plugin to npm, or download plugin updates from the internet.
