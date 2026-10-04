use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cleaner_core::{
  ApplicationSource, InstalledApplication, PortError, UninstallRequest, UninstallResponse,
  UninstallStatus,
};

use super::{plist_xml_string, DiscoverySnapshot, PendingApplication, PlatformAction};

const PLUTIL_EXECUTABLE: &str = "/usr/bin/plutil";
const OSASCRIPT_EXECUTABLE: &str = "/usr/bin/osascript";
const FINDER_TRASH_SCRIPT: &str = "on run argv\n  set selectedItem to POSIX file (item 1 of argv) as alias\n  tell application \"Finder\" to delete selectedItem\nend run";

#[derive(Default)]
struct BundleMetadata {
  bundle_id: Option<String>,
  version: Option<String>,
}

pub(super) fn discover() -> DiscoverySnapshot {
  let mut snapshot = DiscoverySnapshot::default();
  snapshot.warnings.extend([
    "Application usage is not measured; no application is classified as unused.".to_owned(),
    "Confirmed removal asks Finder to move only the selected .app bundle to Trash; application support data and user files are left in place.".to_owned(),
  ]);

  let mut roots = vec![PathBuf::from("/Applications")];
  if let Some(home) = std::env::var_os("HOME") {
    roots.push(PathBuf::from(home).join("Applications"));
  }

  let mut seen = HashSet::new();
  let mut metadata_failures = 0usize;
  let mut missing_bundle_ids = 0usize;
  for root in roots {
    let canonical_root = match fs::canonicalize(&root) {
      Ok(path) => path,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
      Err(_) => {
        snapshot.warnings.push(format!(
          "Could not access the application directory {}.",
          root.display()
        ));
        continue;
      }
    };
    if is_system_app_path(&canonical_root) {
      continue;
    }
    let mut pending_directories = vec![canonical_root.clone()];
    while let Some(directory) = pending_directories.pop() {
      let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(_) => {
          snapshot.warnings.push(format!(
            "Could not read an application directory under {}.",
            root.display()
          ));
          continue;
        }
      };

      for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
          Ok(file_type) => file_type,
          Err(_) => continue,
        };
        if file_type.is_symlink() || !file_type.is_dir() {
          continue;
        }

        if is_app_bundle(&path) {
          let canonical_bundle = match fs::canonicalize(&path) {
            Ok(path) if path.starts_with(&canonical_root) && !is_system_app_path(&path) => path,
            _ => continue,
          };
          if !seen.insert(canonical_bundle.clone()) {
            continue;
          }
          let Some(name) = app_name(&canonical_bundle) else {
            continue;
          };
          let metadata = read_bundle_metadata(&canonical_bundle);
          if metadata.bundle_id.is_none() && metadata.version.is_none() {
            metadata_failures += 1;
          }
          if metadata.bundle_id.is_none() {
            missing_bundle_ids += 1;
            continue;
          }
          snapshot.applications.push(PendingApplication {
            name,
            version: metadata.version,
            source: ApplicationSource::MacApplicationsDirectory,
            action: PlatformAction::MacBundle {
              path: canonical_bundle,
              bundle_id: metadata.bundle_id,
            },
          });
          continue;
        }
        pending_directories.push(path);
      }
    }
  }

  if metadata_failures > 0 {
    snapshot.warnings.push(format!(
      "Metadata could not be read for {metadata_failures} application bundle(s); entries without a verifiable bundle identifier were skipped."
    ));
  }
  if missing_bundle_ids > 0 {
    snapshot.warnings.push(format!(
      "{missing_bundle_ids} application bundle(s) have no readable CFBundleIdentifier and were skipped because they cannot be identified safely for removal."
    ));
  }
  snapshot
}

pub(super) fn uninstall(
  application: &InstalledApplication,
  action: &PlatformAction,
  request: &UninstallRequest,
) -> Result<UninstallResponse, PortError> {
  if !request.confirmed {
    return Err(PortError::ConfirmationRequired);
  }
  if application.id != request.application_id {
    return Err(PortError::OperationFailed {
      message: "The uninstall request does not match the selected application.".to_owned(),
    });
  }

  let PlatformAction::MacBundle { path, bundle_id } = action;
  let bundle_id = require_bundle_id(bundle_id.as_deref())?;
  validate_bundle(path, Some(bundle_id))?;
  let arguments = trash_script_arguments(path)?;
  let output = Command::new(OSASCRIPT_EXECUTABLE)
    .args(arguments)
    .output()
    .map_err(|error| PortError::OperationFailed {
      message: format!("Could not ask Finder to move the selected application to Trash: {error}"),
    })?;
  if !output.status.success() {
    return Err(PortError::OperationFailed {
      message: "Finder did not move the selected application to Trash.".to_owned(),
    });
  }

  Ok(UninstallResponse {
    application_id: application.id.clone(),
    status: UninstallStatus::Completed,
  })
}

fn read_bundle_metadata(bundle: &Path) -> BundleMetadata {
  let info_plist = bundle.join("Contents/Info.plist");
  let output = match Command::new(PLUTIL_EXECUTABLE)
    .args(["-convert", "xml1", "-o", "-"])
    .arg(info_plist)
    .output()
  {
    Ok(output) if output.status.success() => output,
    _ => return BundleMetadata::default(),
  };
  let Ok(xml) = String::from_utf8(output.stdout) else {
    return BundleMetadata::default();
  };
  BundleMetadata {
    bundle_id: plist_xml_string(&xml, "CFBundleIdentifier"),
    version: plist_xml_string(&xml, "CFBundleShortVersionString")
      .or_else(|| plist_xml_string(&xml, "CFBundleVersion")),
  }
}

fn validate_bundle(path: &Path, expected_bundle_id: Option<&str>) -> Result<(), PortError> {
  let metadata = fs::symlink_metadata(path).map_err(|_| PortError::CandidateChanged)?;
  if !metadata.is_dir() || metadata.file_type().is_symlink() || !is_app_bundle(path) {
    return Err(PortError::UnsafeTarget);
  }
  let canonical = fs::canonicalize(path).map_err(|_| PortError::CandidateChanged)?;
  if canonical != path || is_system_app_path(&canonical) || !is_within_application_roots(&canonical)
  {
    return Err(PortError::UnsafeTarget);
  }
  if let Some(expected_bundle_id) = expected_bundle_id {
    let current_bundle_id = read_bundle_metadata(&canonical).bundle_id;
    if current_bundle_id.as_deref() != Some(expected_bundle_id) {
      return Err(PortError::CandidateChanged);
    }
  }
  Ok(())
}

fn require_bundle_id(bundle_id: Option<&str>) -> Result<&str, PortError> {
  bundle_id
    .filter(|bundle_id| !bundle_id.trim().is_empty())
    .ok_or(PortError::AdapterUnavailable {
      operation: "removing a bundle without a verifiable CFBundleIdentifier",
    })
}

fn is_within_application_roots(path: &Path) -> bool {
  if let Ok(root) = fs::canonicalize("/Applications") {
    if !is_system_app_path(&root) && path.starts_with(root) {
      return true;
    }
  }
  std::env::var_os("HOME")
    .and_then(|home| fs::canonicalize(PathBuf::from(home).join("Applications")).ok())
    .map(|root| !is_system_app_path(&root) && path.starts_with(root))
    .unwrap_or(false)
}

fn is_system_app_path(path: &Path) -> bool {
  path.starts_with("/System/Applications")
}

fn is_app_bundle(path: &Path) -> bool {
  path.extension().is_some_and(|extension| extension == "app")
}

fn app_name(path: &Path) -> Option<String> {
  path
    .file_name()?
    .to_str()?
    .strip_suffix(".app")
    .filter(|name| !name.is_empty())
    .map(str::to_owned)
}

fn trash_script_arguments(path: &Path) -> Result<Vec<std::ffi::OsString>, PortError> {
  if !path.is_absolute() {
    return Err(PortError::UnsafeTarget);
  }
  Ok(vec![
    "-e".into(),
    FINDER_TRASH_SCRIPT.into(),
    path.as_os_str().to_owned(),
  ])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn trash_script_keeps_the_selected_path_as_one_argument() {
    let path = PathBuf::from("/Applications/An App; echo unsafe.app");
    let arguments = trash_script_arguments(&path).expect("absolute paths are accepted");

    assert_eq!(arguments.len(), 3);
    assert_eq!(arguments[0], "-e");
    assert_eq!(arguments[2], path.as_os_str());
  }

  #[test]
  fn system_app_paths_are_excluded_by_component_prefix() {
    assert!(is_system_app_path(Path::new(
      "/System/Applications/Calendar.app"
    )));
    assert!(!is_system_app_path(Path::new(
      "/System/Applications-Backup/Calendar.app"
    )));
  }

  #[test]
  fn bundle_without_identifier_has_no_safe_removal_action() {
    assert!(matches!(
      require_bundle_id(None),
      Err(PortError::AdapterUnavailable { .. })
    ));
    assert_eq!(
      require_bundle_id(Some("org.example.Editor")).expect("a valid bundle identifier is accepted"),
      "org.example.Editor"
    );
  }
}
