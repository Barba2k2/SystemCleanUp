#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
  tauri::Builder::default()
    .manage(commands::CleanerState::default())
    .invoke_handler(tauri::generate_handler![
      commands::scan_candidates,
      commands::prepare_cleanup_preview,
      commands::execute_cleanup,
      commands::discover_applications,
      commands::uninstall_application
    ])
    .run(tauri::generate_context!())
    .expect("failed to run System CleanUp");
}
