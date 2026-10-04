# Architecture

## Overview

The Rust workspace is the shared application boundary for the Tauri desktop shell and optional CLI. `cleaner-core` owns domain models, request and response contracts, typed events, and ports. `cleaner-files` and `cleaner-apps` implement the platform-facing workflows. The frontend never performs file deletion or runs package-manager commands.

The design is event-driven, not event-sourced. Use cases publish typed domain events as work progresses and completes. Events are not a persisted log or source of truth. The Rust catalog and operation results remain authoritative.

## Request and event flow

1. The React interface sends a typed request to a Tauri command with `invoke`.
2. A shared managed Tauri state calls the file cleaner or application manager.
3. The Rust use case checks opaque IDs against its in-process catalog and executes the requested operation.
4. A Tauri `EventPublisher` forwards `ProgressEvent` payloads as `operation-progress`; the command returns the typed response.

The desktop state shares each manager across command calls in that app process. The CLI creates one manager per invocation and completes its full interactive flow in that process. Neither interface persists candidate or application IDs between launches.

Tauri command entry points are `scan_candidates`, `prepare_cleanup_preview`, `execute_cleanup`, `discover_applications`, and `uninstall_application`. File cleanup accepts the selected `RemovalMode` (`trash` or `permanent`). The frontend event payload types in `apps/desktop/src/api/cleaner.ts` mirror the Rust event contract.

## File cleanup

The file flow is scan, explicit candidate-ID selection, preview, confirmation, and cleanup. `cleaner-files` owns allowlisted roots and stores scan and preview catalogs in memory. The frontend sends IDs rather than paths. Rust validates IDs against its catalog and rechecks root identity, canonical path containment, file type, size, and modification time immediately before removal.

`RemovalMode::Trash` asks the OS to move each approved regular file to its Trash or Recycle Bin. `RemovalMode::Permanent` uses permanent file removal and requires explicit confirmation. Each selected entry is reported as removed or failed; a failed item does not broaden into a parent path or wildcard.

The file roots are fixed by target. macOS uses user caches, user-owned temp subdirectories, and user logs. Windows uses user INetCache, Temp, and CrashDumps. Linux uses XDG cache and state log roots; shared `/tmp` is not scanned. The exact boundaries and known path-race limitation are documented in `docs/safety.md` and `crates/cleaner-files/README.md`.

## Application inventory and action

Application inventory does not infer whether software is unused. It exposes an opaque ID mapped to a local platform record and requires the current catalog ID plus explicit confirmation for an action.

- macOS reads `.app` bundles under `/Applications` and `~/Applications`, excluding `/System/Applications`. It reads bundle identity and version from `Info.plist`. A confirmed action asks Finder to move only the selected bundle to Trash; app support data and personal files remain. Bundles without a readable `CFBundleIdentifier` are skipped.
- Windows reads registered uninstall entries from HKCU and HKLM in both registry views. It filters entries without `DisplayName` and entries marked as system components. It does not invoke `UninstallString` or guess a WinGet ID. A validated package family name opens its exact `ms-settings` app page; otherwise the adapter opens Apps & Features. Both are handoffs and return `delegated_to_system`.
- Linux lists installed Flatpak and Snap packages using fixed executables. Before removal, it re-queries the package source and requires the exact validated ID to remain present, then passes that ID as a separate argument. Other visible `.desktop` applications have no generic uninstall adapter and return an explicit unavailable error.

An application `Completed` status is returned only when the adapter itself completes the supported action. Opening Windows Settings returns `DelegatedToSystem`; the app does not claim that the user finished the removal.

## Platform packaging and permissions

The source tree is shared, while Tauri bundles are target-specific. `tauri.conf.json` enables bundling for supported targets. The desktop capability grants core event listening only; there are no filesystem, shell, or process permissions because those operations remain inside Rust adapters.
