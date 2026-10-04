# Safety Requirements

## File cleanup

- Accept only the `CandidateCategory` allowlist in `cleaner-core`; do not accept user-supplied roots, arbitrary paths, or shell commands as cleanup targets.
- Every discovered candidate includes its category, path, byte size, reason, risk, and an opaque ID held in a process-local catalog.
- Require explicit candidate selection and a preview before cleanup. A scan or preview never deletes files.
- Resolve IDs against the Rust-side scan catalog. Do not trust candidate details echoed by the frontend.
- Revalidate the category and target in Rust immediately before removal. Check canonical path containment within a category-specific approved root, reject roots and unexpected file types, and account for symlinks and metadata changes.
- `RemovalMode::Trash` is the default reversible choice. `RemovalMode::Permanent` must be selected and separately confirmed; it cannot be undone through the application's Trash flow.
- If identity, path, category, or metadata changed after preview, block removal and require a new scan and preview.
- Report each result. Never silently broaden a failed candidate into a parent directory or wildcard deletion.

The fixed file roots are limited to the current user's cache, temporary, and diagnostic-log locations. Linux does not scan shared `/tmp`. System-wide paths, application data, and arbitrary folders are excluded. A path replacement by another process under the same user can race a path-based OS trash operation after validation; see `crates/cleaner-files/README.md` for the remaining limitation.

## Installed applications

- Keep inventory separate from removal and never classify an application as unused from age, launch history, or other heuristics.
- Require the user to select one current application ID and confirm the action.
- Keep the ID-to-record catalog in process memory. The CLI must complete discovery, selection, confirmation, and action in one invocation.
- Use only the platform action documented for the current source. Never run arbitrary `UninstallString`, `Exec`, or caller-provided command text.
- Revalidate the selected target immediately before action. macOS checks the canonical bundle path and the current `CFBundleIdentifier`; bundles without a readable identifier are skipped. Windows checks the registry entry, display name, and package family name again. Linux queries Flatpak/Snap again and requires the selected exact package ID.
- Do not remove application support folders, personal documents, or other user data automatically.
- Report when no safe adapter exists. Opening a native settings page is a handoff, not proof that an uninstall completed.

### Operating-system scope

- macOS inventories `.app` bundles in `/Applications` and `~/Applications`, excluding `/System/Applications`. Confirmed removal asks Finder to move the selected bundle to Trash. It does not claim that support files were removed.
- Windows inventories uninstall registrations in HKCU and HKLM across 32-bit and 64-bit registry views. It filters missing display names and system components. A validated package family name opens that app's Settings page; otherwise the action opens Apps & Features. Both return `delegated_to_system`. `UninstallString` is never executed and WinGet is not used without an unambiguous package-ID mapping.
- Linux inventories Flatpak and Snap packages and may list other launchers from `.desktop` files. Flatpak/Snap removal passes one validated ID as a separate argument to a fixed executable. `.desktop` entries without a known package adapter are unavailable for removal.

## Desktop permissions

Tauri bundling is enabled for target-specific packages. The capability grants core event listening only. It does not grant frontend filesystem, shell, or process access. Package-manager and Finder/Settings actions remain behind the Rust command boundary and explicit confirmation.
