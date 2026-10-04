import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type CandidateCategory =
  | "user_cache"
  | "user_temporary"
  | "diagnostic_log";

export type ScanRequest = {
  categories: CandidateCategory[];
};

export type PreviewRequest = {
  scan_id: string;
  selected_candidate_ids: string[];
};

export type CleanupRequest = {
  preview_id: string;
  confirmed: boolean;
  removal_mode: RemovalMode;
};

export type RemovalMode = "trash" | "permanent";

export type ApplicationDiscoveryRequest = Record<string, never>;

export type UninstallRequest = {
  application_id: string;
  confirmed: boolean;
};

export type OperationKind =
  | "candidate_scan"
  | "cleanup_preview"
  | "cleanup"
  | "application_discovery"
  | "uninstall";

export type OperationProgress = {
  operation_id: string;
  operation: OperationKind;
  completed_units: number;
  total_units: number | null;
  message: string;
};

export type ScanResponse = {
  scan_id: string;
  candidates: RemovalCandidate[];
  warnings: string[];
};

export type RemovalCandidate = {
  id: string;
  category: CandidateCategory;
  path: string;
  size_bytes: number;
  reason: string;
  risk: "low" | "moderate" | "high";
};

export type PreviewResponse = {
  preview_id: string;
  entries: Array<
    | { state: "eligible"; candidate: RemovalCandidate }
    | { state: "blocked"; candidate: RemovalCandidate; reason: string }
  >;
};

export type CleanupResponse = {
  removed_candidate_ids: string[];
  failed_candidate_ids: string[];
};

export type InstalledApplication = {
  id: string;
  name: string;
  version: string | null;
  source:
    | "mac_applications_directory"
    | "windows_registry"
    | "windows_package_manager"
    | "linux_package_manager"
    | "linux_desktop_entry"
    | "other";
};

export type ApplicationDiscoveryResponse = {
  applications: InstalledApplication[];
  warnings: string[];
};

export type UninstallResponse = {
  application_id: string;
  status: "completed" | "delegated_to_system";
};

export function scanCandidates(request: ScanRequest): Promise<ScanResponse> {
  return invoke<ScanResponse>("scan_candidates", { request });
}

export function prepareCleanupPreview(
  request: PreviewRequest,
): Promise<PreviewResponse> {
  return invoke<PreviewResponse>("prepare_cleanup_preview", { request });
}

export function executeCleanup(request: CleanupRequest): Promise<CleanupResponse> {
  return invoke<CleanupResponse>("execute_cleanup", { request });
}

export function discoverApplications(
  request: ApplicationDiscoveryRequest,
): Promise<ApplicationDiscoveryResponse> {
  return invoke<ApplicationDiscoveryResponse>("discover_applications", {
    request,
  });
}

export function uninstallApplication(
  request: UninstallRequest,
): Promise<UninstallResponse> {
  return invoke<UninstallResponse>("uninstall_application", { request });
}

export function subscribeToProgress(
  handler: (progress: OperationProgress) => void,
): Promise<UnlistenFn> {
  return listen<OperationProgress>("operation-progress", (event) => {
    handler(event.payload);
  });
}
