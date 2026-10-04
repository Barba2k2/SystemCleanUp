use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use cleaner_core::{
  ApplicationSource, InstalledApplication, PortError, UninstallRequest, UninstallResponse,
  UninstallStatus,
};

use super::{package_name_is_safe, DiscoverySnapshot, PendingApplication, PlatformAction};

const FLATPAK_EXECUTABLE: &str = "/usr/bin/flatpak";
const SNAP_EXECUTABLE: &str = "/usr/bin/snap";

struct PackageRow {
  id: String,
  name: String,
  version: Option<String>,
}

struct DesktopEntry {
  name: String,
  flatpak_id: Option<String>,
}

pub(super) fn discover() -> DiscoverySnapshot {
  let mut snapshot = DiscoverySnapshot::default();
  snapshot
    .warnings
    .push("Application usage is not measured; no application is classified as unused.".to_owned());
  snapshot.warnings.push(
    "Only Flatpak and Snap have command-line uninstall adapters; other desktop entries are listed as unavailable.".to_owned(),
  );
  snapshot.warnings.push(
    "The application manager does not run package managers through sudo; permissions or system authorization may be required.".to_owned(),
  );

  let flatpak_ids = match run_list(
    FLATPAK_EXECUTABLE,
    &["list", "--app", "--columns=application,name,version"],
  ) {
    Ok(output) => parse_flatpak_list(&output),
    Err(message) => {
      snapshot.warnings.push(message);
      Vec::new()
    }
  };
  let mut known_flatpak_ids = HashSet::new();
  for package in flatpak_ids {
    known_flatpak_ids.insert(package.id.clone());
    snapshot.applications.push(PendingApplication {
      name: package.name,
      version: package.version,
      source: ApplicationSource::LinuxPackageManager,
      action: PlatformAction::Flatpak {
        application_id: package.id,
      },
    });
  }

  match run_list(SNAP_EXECUTABLE, &["list"]) {
    Ok(output) => {
      for package in parse_snap_list(&output) {
        snapshot.applications.push(PendingApplication {
          name: package.name,
          version: package.version,
          source: ApplicationSource::LinuxPackageManager,
          action: PlatformAction::Snap {
            package_name: package.id,
          },
        });
      }
    }
    Err(message) => snapshot.warnings.push(message),
  }

  let mut seen_desktop_ids = HashSet::new();
  for applications_directory in desktop_application_directories() {
    let entries = match fs::read_dir(&applications_directory) {
      Ok(entries) => entries,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
      Err(_) => {
        snapshot.warnings.push(format!(
          "Could not read desktop application entries under {}.",
          applications_directory.display()
        ));
        continue;
      }
    };

    for entry in entries.flatten() {
      let path = entry.path();
      if !is_desktop_file(&path) {
        continue;
      }
      let Some(desktop_id) = path.file_name().and_then(|name| name.to_str()) else {
        continue;
      };
      if !seen_desktop_ids.insert(desktop_id.to_ascii_lowercase()) {
        continue;
      }
      let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(_) => continue,
      };
      let Some(entry) = parse_desktop_entry(&contents) else {
        continue;
      };
      if entry
        .flatpak_id
        .as_deref()
        .is_some_and(|flatpak_id| known_flatpak_ids.contains(flatpak_id))
      {
        continue;
      }

      snapshot.applications.push(PendingApplication {
        name: entry.name,
        version: None,
        source: ApplicationSource::LinuxDesktopEntry,
        action: PlatformAction::DesktopEntry,
      });
    }
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

  let (executable, arguments) = match action {
    PlatformAction::Flatpak { application_id } => (
      FLATPAK_EXECUTABLE,
      flatpak_uninstall_args(application_id).ok_or(PortError::AdapterUnavailable {
        operation: "uninstalling the selected Flatpak application",
      })?,
    ),
    PlatformAction::Snap { package_name } => (
      SNAP_EXECUTABLE,
      snap_uninstall_args(package_name).ok_or(PortError::AdapterUnavailable {
        operation: "uninstalling the selected Snap application",
      })?,
    ),
    PlatformAction::DesktopEntry => {
      return Err(PortError::AdapterUnavailable {
        operation: "uninstalling this application through its package manager",
      });
    }
  };

  let is_present = match action {
    PlatformAction::Flatpak { application_id } => run_list(
      FLATPAK_EXECUTABLE,
      &["list", "--app", "--columns=application,name,version"],
    )
    .map(|output| {
      parse_flatpak_list(&output)
        .iter()
        .any(|package| package.id.as_str() == application_id.as_str())
    }),
    PlatformAction::Snap { package_name } => run_list(SNAP_EXECUTABLE, &["list"]).map(|output| {
      parse_snap_list(&output)
        .iter()
        .any(|package| package.id.as_str() == package_name.as_str())
    }),
    PlatformAction::DesktopEntry => unreachable!("desktop entries are not uninstallable"),
  }
  .map_err(|_| PortError::CandidateChanged)?;
  if !is_present {
    return Err(PortError::CandidateChanged);
  }

  let output = Command::new(executable)
    .args(arguments)
    .output()
    .map_err(|error| PortError::OperationFailed {
      message: format!("Could not start the native package manager: {error}"),
    })?;
  if !output.status.success() {
    return Err(PortError::OperationFailed {
      message: format!(
        "The native package manager did not complete the removal (status {}).",
        output.status
      ),
    });
  }

  Ok(UninstallResponse {
    application_id: application.id.clone(),
    status: UninstallStatus::Completed,
  })
}

fn run_list(executable: &str, arguments: &[&str]) -> Result<String, String> {
  let output = Command::new(executable)
    .args(arguments)
    .output()
    .map_err(|error| {
      if error.kind() == std::io::ErrorKind::NotFound {
        format!(
          "{} is not installed; that package source was skipped.",
          executable.rsplit('/').next().unwrap_or(executable)
        )
      } else {
        format!(
          "Could not query {}; that package source was skipped.",
          executable.rsplit('/').next().unwrap_or(executable)
        )
      }
    })?;
  if !output.status.success() {
    return Err(format!(
      "{} could not list installed applications (status {}).",
      executable.rsplit('/').next().unwrap_or(executable),
      output.status
    ));
  }
  String::from_utf8(output.stdout).map_err(|_| {
    format!(
      "{} returned non-text application data.",
      executable.rsplit('/').next().unwrap_or(executable)
    )
  })
}

fn parse_flatpak_list(output: &str) -> Vec<PackageRow> {
  output
    .lines()
    .filter_map(|line| {
      let columns: Vec<&str> = line.split('\t').map(str::trim).collect();
      if columns.len() < 2
        || columns[0].eq_ignore_ascii_case("application")
        || !package_name_is_safe(columns[0])
      {
        return None;
      }
      let name = if columns[1].is_empty() {
        columns[0]
      } else {
        columns[1]
      };
      Some(PackageRow {
        id: columns[0].to_owned(),
        name: name.to_owned(),
        version: columns.get(2).and_then(|version| nonempty_version(version)),
      })
    })
    .collect()
}

fn parse_snap_list(output: &str) -> Vec<PackageRow> {
  output
    .lines()
    .filter_map(|line| {
      let columns: Vec<&str> = line.split_whitespace().collect();
      if columns.len() < 2
        || columns[0].eq_ignore_ascii_case("name")
        || !is_safe_snap_name(columns[0])
      {
        return None;
      }
      Some(PackageRow {
        id: columns[0].to_owned(),
        name: columns[0].to_owned(),
        version: nonempty_version(columns.get(1).copied().unwrap_or_default()),
      })
    })
    .collect()
}

fn parse_desktop_entry(contents: &str) -> Option<DesktopEntry> {
  let mut in_desktop_entry = false;
  let mut entry_type = None;
  let mut name = None;
  let mut hidden = false;
  let mut no_display = false;
  let mut flatpak_id = None;

  for line in contents.lines().map(str::trim) {
    if line.starts_with('[') && line.ends_with(']') {
      in_desktop_entry = line == "[Desktop Entry]";
      continue;
    }
    if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
      continue;
    }
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    match key.trim() {
      "Type" => entry_type = Some(value.trim()),
      "Name" => name = Some(value.trim()),
      "Hidden" => hidden = value.trim().eq_ignore_ascii_case("true"),
      "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
      "X-Flatpak" => flatpak_id = Some(value.trim()),
      _ => {}
    }
  }

  let name = name.filter(|value| !value.is_empty())?;
  if entry_type != Some("Application") || hidden || no_display {
    return None;
  }
  Some(DesktopEntry {
    name: name.to_owned(),
    flatpak_id: flatpak_id
      .filter(|value| !value.is_empty())
      .map(str::to_owned),
  })
}

fn desktop_application_directories() -> Vec<PathBuf> {
  let mut directories = Vec::new();
  if let Some(home) = std::env::var_os("HOME") {
    directories.push(PathBuf::from(home).join(".local/share/applications"));
  }
  directories.push(PathBuf::from("/usr/local/share/applications"));
  directories.push(PathBuf::from("/usr/share/applications"));
  directories
}

fn is_desktop_file(path: &Path) -> bool {
  path
    .extension()
    .is_some_and(|extension| extension == "desktop")
}

fn nonempty_version(value: &str) -> Option<String> {
  let value = value.trim();
  (!value.is_empty() && value != "-").then(|| value.to_owned())
}

fn is_safe_snap_name(value: &str) -> bool {
  !value.is_empty()
    && value.as_bytes()[0].is_ascii_lowercase()
    && value
      .bytes()
      .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn flatpak_uninstall_args(application_id: &str) -> Option<Vec<String>> {
  package_name_is_safe(application_id).then(|| {
    vec![
      "uninstall".to_owned(),
      "--assumeyes".to_owned(),
      "--noninteractive".to_owned(),
      application_id.to_owned(),
    ]
  })
}

fn snap_uninstall_args(package_name: &str) -> Option<Vec<String>> {
  is_safe_snap_name(package_name).then(|| vec!["remove".to_owned(), package_name.to_owned()])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_flatpak_inventory_as_tab_separated_rows() {
    let rows = parse_flatpak_list(
      "org.example.Editor\tExample Editor\t4.2\norg.example.Platform\tPlatform\t-\n",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "org.example.Editor");
    assert_eq!(rows[0].name, "Example Editor");
    assert_eq!(rows[0].version.as_deref(), Some("4.2"));
    assert_eq!(rows[1].version, None);
  }

  #[test]
  fn parses_snap_inventory_and_ignores_header_and_unsafe_names() {
    let rows = parse_snap_list(
      "Name Version Rev Tracking Publisher Notes\neditor 2.3 17 latest/stable vendor -\n--help 1 1 latest/stable vendor -\n",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "editor");
    assert_eq!(rows[0].version.as_deref(), Some("2.3"));
  }

  #[test]
  fn desktop_parser_excludes_hidden_and_non_application_entries() {
    let visible = parse_desktop_entry(
      "[Desktop Entry]\nType=Application\nName=Editor\nX-Flatpak=org.example.Editor\n",
    )
    .expect("visible application should be parsed");
    assert_eq!(visible.name, "Editor");
    assert_eq!(visible.flatpak_id.as_deref(), Some("org.example.Editor"));
    assert!(parse_desktop_entry(
      "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\n"
    )
    .is_none());
    assert!(parse_desktop_entry("[Desktop Entry]\nType=Link\nName=Link\n").is_none());
  }

  #[test]
  fn uninstall_arguments_keep_package_ids_as_single_arguments() {
    assert_eq!(
      flatpak_uninstall_args("org.example.Editor").expect("valid Flatpak id"),
      vec![
        "uninstall".to_owned(),
        "--assumeyes".to_owned(),
        "--noninteractive".to_owned(),
        "org.example.Editor".to_owned(),
      ]
    );
    assert_eq!(
      snap_uninstall_args("my-editor").expect("valid Snap name"),
      vec!["remove".to_owned(), "my-editor".to_owned()]
    );
    assert!(flatpak_uninstall_args("--help").is_none());
    assert!(snap_uninstall_args("editor;touch").is_none());
  }
}
