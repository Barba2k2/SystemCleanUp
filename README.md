# System CleanUp

System CleanUp is a Rust desktop utility for reviewing allowlisted user files and installed applications on macOS, Windows, and Linux. The desktop shell uses Tauri 2, React, and TypeScript. The optional CLI shares the same Rust engines.

## Implemented capabilities

| Capability | macOS | Windows | Linux |
| --- | --- | --- | --- |
| File inventory | User caches, constrained user temp, and user logs | User INetCache, Temp, and CrashDumps | XDG cache and state logs; shared `/tmp` is unsupported |
| File cleanup | Trash or confirmed permanent removal | Recycle Bin or confirmed permanent removal | Desktop Trash or confirmed permanent removal |
| Application inventory | `.app` bundles in `/Applications` and `~/Applications` | HKCU/HKLM uninstall registrations in 32-bit and 64-bit views | Flatpak, Snap, and visible `.desktop` entries |
| Application action | Finder moves the selected bundle to Trash | Opens the matching Settings page or Apps & Features | Removes an exact Flatpak/Snap ID; other entries are unavailable |

Application inventory is descriptive. The app does not measure use or mark software as unused. Application support files and personal data are not removed with a macOS app bundle.

Windows Settings actions return `delegated_to_system`. Opening Settings is a handoff, not proof that the user completed removal. Win32 `UninstallString` values are never executed. Flatpak and Snap actions use fixed executable paths and separately validated package IDs.

## Safe workflow

File cleanup follows scan, explicit candidate selection, preview, confirmation, and execution. Rust stores scan and preview catalogs in process memory and revalidates selected files immediately before action. Trash is the normal reversible mode. Permanent removal requires a separate explicit confirmation.

The CLI completes each flow in one invocation because its catalogs are process-local:

~~~sh
cargo run -p cleaner-cli -- files
cargo run -p cleaner-cli -- apps
~~~

The `files` command lists paths and accepts comma-separated selections or `all`, then prepares a preview and asks for a mode-specific confirmation. The `apps` command lists discovered applications, asks for one selection, and requires explicit confirmation before removal or handoff. IDs from an earlier CLI invocation are not accepted.

The desktop bridge forwards typed Rust progress events on the `operation-progress` Tauri event. Its capability grants only event listening; it does not grant filesystem or shell access. Tauri bundling is enabled for target-specific packages.

## Workspace

- `crates/cleaner-core` defines domain data, typed events, and platform ports.
- `crates/cleaner-files` implements allowlisted file discovery, preview, revalidation, and cleanup.
- `crates/cleaner-apps` implements per-platform application inventory and safe removal or handoff.
- `crates/cleaner-cli` provides the optional same-process interactive workflows.
- `apps/desktop` contains the React/TypeScript interface.
- `apps/desktop/src-tauri` contains the shared service state and Tauri command bridge.
- `docs/architecture.md` describes the event-driven architecture and service boundaries.
- `docs/safety.md` records the operational limits and confirmation requirements.

## Platform limits

- macOS inventory reads bundle identifiers and versions from `Info.plist`; a bundle without a readable identifier is skipped. Finder moves only the selected `.app` bundle to Trash; support data remains.
- Windows inventory covers registered uninstall entries. Validated package family names open their exact Settings page; other registered applications open Apps & Features. The user must finish the uninstall there.
- Linux package inventory covers Flatpak and Snap. Other applications may appear through `.desktop` files, but there is no generic safe uninstall action for those entries.
- Linux temporary files are intentionally unsupported because the usual `/tmp` directory is shared and cannot be safely scoped to one user.
- File cleanup is limited to the configured per-user roots described in `docs/safety.md`; it does not scan application data or system-wide locations.

## Development

Tauri requires the platform-specific system libraries and packaging tools for the target OS. The application builds a separate bundle or installer per target; it does not produce one universal binary.

From `apps/desktop`, the intended desktop commands are:

~~~sh
npm install
npm run tauri dev
npm run tauri build
~~~

The project uses the MIT license.
