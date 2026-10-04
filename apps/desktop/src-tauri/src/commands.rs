use cleaner_apps::ApplicationManager;
use cleaner_core::{
  ApplicationDiscoveryPort, ApplicationDiscoveryRequest, ApplicationDiscoveryResponse,
  CleanupPreview, CleanupRequest, CleanupResponse, DomainEvent, EventPublisher, PreviewRequest,
  ScanRequest, ScanResponse, UninstallRequest, UninstallResponse,
};
use cleaner_files::FileCleaner;
use tauri::{AppHandle, Emitter, State};

pub struct CleanerState {
  file_cleaner: FileCleaner,
  application_manager: ApplicationManager,
}

impl Default for CleanerState {
  fn default() -> Self {
    Self {
      file_cleaner: FileCleaner::new(),
      application_manager: ApplicationManager::new(),
    }
  }
}

struct TauriEvents(AppHandle);

impl EventPublisher for TauriEvents {
  fn publish(&self, event: DomainEvent) {
    if let DomainEvent::Progress(progress) = event {
      let _ = self.0.emit("operation-progress", progress);
    }
  }
}

#[tauri::command]
pub fn scan_candidates(
  app: AppHandle,
  state: State<'_, CleanerState>,
  request: ScanRequest,
) -> Result<ScanResponse, String> {
  let events = TauriEvents(app);
  state
    .file_cleaner
    .scan(request, &events)
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn prepare_cleanup_preview(
  app: AppHandle,
  state: State<'_, CleanerState>,
  request: PreviewRequest,
) -> Result<CleanupPreview, String> {
  let events = TauriEvents(app);
  state
    .file_cleaner
    .prepare_preview(request, &events)
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn execute_cleanup(
  app: AppHandle,
  state: State<'_, CleanerState>,
  request: CleanupRequest,
) -> Result<CleanupResponse, String> {
  let events = TauriEvents(app);
  state
    .file_cleaner
    .execute_cleanup(request, &events)
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn discover_applications(
  app: AppHandle,
  state: State<'_, CleanerState>,
  request: ApplicationDiscoveryRequest,
) -> Result<ApplicationDiscoveryResponse, String> {
  let events = TauriEvents(app);
  state
    .application_manager
    .discover(&request, &events)
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn uninstall_application(
  app: AppHandle,
  state: State<'_, CleanerState>,
  request: UninstallRequest,
) -> Result<UninstallResponse, String> {
  let events = TauriEvents(app);
  state
    .application_manager
    .uninstall_application(request, &events)
    .map_err(|error| error.to_string())
}
