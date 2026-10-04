# cleaner-files

This crate implements allowlisted user-file discovery, in-process scan and preview catalogs, metadata revalidation, and file removal. Its public flow is `FileCleaner::new()`, `scan(request, events)`, `prepare_preview(request, events)`, and `execute_cleanup(request, events)`. Preview preparation publishes a typed `CleanupPreviewPrepared` domain event. The crate accepts category selections and opaque IDs; it never accepts a caller-supplied filesystem path.

## Supported roots

The roots are fixed by target and are not configurable through the API:

| Target | User cache | User temporary | Diagnostic logs |
| --- | --- | --- | --- |
| macOS | $HOME/Library/Caches | Current process temp directory only when it is a user-owned T directory under /var/folders | $HOME/Library/Logs |
| Windows | %USERPROFILE%/AppData/Local/Microsoft/Windows/INetCache | %USERPROFILE%/AppData/Local/Temp | %USERPROFILE%/AppData/Local/CrashDumps |
| Linux | $XDG_CACHE_HOME under $HOME, or $HOME/.cache | Unsupported: the usual /tmp is shared and cannot be safely scoped to one user by this crate | $XDG_STATE_HOME/logs under $HOME, or $HOME/.local/state/logs |

Windows roots are derived from `USERPROFILE`; `LOCALAPPDATA` is not trusted as a root override. Fixed macOS and Windows roots must preserve their expected relative directory layout after canonicalization, so a symlinked ancestor cannot redirect discovery elsewhere in the profile. Every accepted root must be a real directory rather than a symlink/reparse point and must be user-owned where the platform exposes ownership metadata. A filesystem root or a root equal to its boundary is rejected. Missing, unreadable, unsafe, or unsupported roots are reported as scan warnings. Linux does not scan a shared temp directory. No system-wide cache, system log, application-data root, or entire AppData directory is included.

## Removal modes

RemovalMode::Trash asks the operating system to move a selected regular file to its Recycle Bin/Trash and is the recommended default. RemovalMode::Permanent calls remove_file for a selected regular file only. Both require a previously prepared preview and confirmed: true.

Trash mode uses the trash crate (MIT), which supports Windows, macOS, and FreeDesktop-compliant Linux environments. This gives the application native recovery semantics without implementing three trash formats itself. Linux support depends on a usable desktop trash environment and can fail on headless or unusual setups; failures are returned per candidate. The upstream crate documents a mutex-protected use of libc mount-point APIs on Linux/FreeBSD as a residual unsafe-code tradeoff.

The catalog is process-local by design. A CLI must complete scan, selection, preview, confirmation, and execution in one process. The crate does not persist candidate paths or previews to disk.

## Safety boundary

Discovery does not follow symlinks and emits regular files only. A scan stores the candidate's root, canonical path, size, and modification time. Preview checks that snapshot without modifying files. Execution requires an explicit mode and confirmation, then re-resolves the approved root and rechecks the candidate's canonical containment, file type, size, and modification time immediately before removal. Changed or unsafe entries are skipped and returned as failures.

Filesystem checks and the trash API operate on paths, so another process running as the same user could race a path replacement between final validation and the OS operation. This crate narrows that window but cannot make path-based trash operations atomic. A future hardened adapter can use platform-specific handle-relative operations where the OS supports them.
