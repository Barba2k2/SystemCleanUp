use std::{
  env,
  ffi::OsStr,
  fs,
  io::ErrorKind,
  path::{Component, Path, PathBuf},
};

use cleaner_core::{CandidateCategory, CandidateRisk};

#[derive(Debug, Clone)]
pub(crate) struct AllowedRoot {
  pub(crate) category: CandidateCategory,
  pub(crate) label: &'static str,
  pub(crate) path: Option<PathBuf>,
  pub(crate) boundary: Option<PathBuf>,
  pub(crate) owner_anchor: Option<PathBuf>,
  pub(crate) required_leaf: Option<&'static str>,
  pub(crate) minimum_relative_components: usize,
  pub(crate) required_relative_component_count: Option<usize>,
  pub(crate) expected_relative_components: Option<&'static [&'static str]>,
  pub(crate) risk: CandidateRisk,
  pub(crate) reason: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRoot {
  pub(crate) spec: AllowedRoot,
  pub(crate) canonical_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootIssue {
  Missing,
  Inaccessible,
  Unsafe,
}

pub(crate) struct PlatformRoots {
  pub(crate) roots: Vec<AllowedRoot>,
  pub(crate) unsupported_categories: Vec<CandidateCategory>,
}

impl AllowedRoot {
  fn new(
    category: CandidateCategory,
    label: &'static str,
    path: Option<PathBuf>,
    boundary: Option<PathBuf>,
    risk: CandidateRisk,
    reason: &'static str,
  ) -> Self {
    Self {
      category,
      label,
      path,
      boundary,
      owner_anchor: None,
      required_leaf: None,
      minimum_relative_components: 0,
      required_relative_component_count: None,
      expected_relative_components: None,
      risk,
      reason,
    }
  }
}

pub(crate) fn platform_roots() -> PlatformRoots {
  #[cfg(target_os = "macos")]
  {
    macos_roots()
  }
  #[cfg(target_os = "windows")]
  {
    windows_roots()
  }
  #[cfg(target_os = "linux")]
  {
    linux_roots()
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
  {
    PlatformRoots {
      roots: Vec::new(),
      unsupported_categories: all_categories(),
    }
  }
}

pub(crate) fn resolve_root(spec: &AllowedRoot) -> Result<ResolvedRoot, RootIssue> {
  let path = spec.path.as_ref().ok_or(RootIssue::Missing)?;
  let boundary = spec.boundary.as_ref().ok_or(RootIssue::Missing)?;

  if !path.is_absolute() || !boundary.is_absolute() {
    return Err(RootIssue::Unsafe);
  }

  let root_metadata = fs::symlink_metadata(path).map_err(map_io_error)?;
  if is_link_or_reparse_point(&root_metadata) || !root_metadata.is_dir() {
    return Err(RootIssue::Unsafe);
  }

  let boundary_metadata = fs::metadata(boundary).map_err(map_io_error)?;
  if !boundary_metadata.is_dir() {
    return Err(RootIssue::Unsafe);
  }

  let canonical_boundary = fs::canonicalize(boundary).map_err(map_io_error)?;
  let canonical_path = fs::canonicalize(path).map_err(map_io_error)?;
  if is_filesystem_root(&canonical_boundary)
    || canonical_path == canonical_boundary
    || !canonical_path.starts_with(&canonical_boundary)
    || is_filesystem_root(&canonical_path)
  {
    return Err(RootIssue::Unsafe);
  }

  if let Some(required_leaf) = spec.required_leaf {
    if canonical_path.file_name().and_then(|leaf| leaf.to_str()) != Some(required_leaf) {
      return Err(RootIssue::Unsafe);
    }

    let relative = canonical_path
      .strip_prefix(&canonical_boundary)
      .map_err(|_| RootIssue::Unsafe)?;
    let components = relative
      .components()
      .filter_map(|component| match component {
        Component::Normal(component) => Some(component),
        _ => None,
      })
      .collect::<Vec<_>>();
    if components.len() < spec.minimum_relative_components
      || spec
        .required_relative_component_count
        .is_some_and(|count| components.len() != count)
      || spec.expected_relative_components.is_some_and(|expected| {
        expected.len() != components.len()
          || expected
            .iter()
            .zip(&components)
            .any(|(expected, actual)| !component_matches(actual, expected))
      })
    {
      return Err(RootIssue::Unsafe);
    }
  }

  if let Some(owner_anchor) = spec.owner_anchor.as_deref() {
    let anchor_metadata = fs::metadata(owner_anchor).map_err(map_io_error)?;
    let canonical_anchor = fs::canonicalize(owner_anchor).map_err(map_io_error)?;
    if is_filesystem_root(&canonical_anchor) || !same_owner(&root_metadata, &anchor_metadata) {
      return Err(RootIssue::Unsafe);
    }
  }

  Ok(ResolvedRoot {
    spec: spec.clone(),
    canonical_path,
  })
}

pub(crate) fn root_issue_message(issue: RootIssue, label: &str) -> String {
  match issue {
    RootIssue::Missing => format!("The approved {label} directory is not available."),
    RootIssue::Inaccessible => {
      format!("The approved {label} directory could not be read; it was skipped.")
    }
    RootIssue::Unsafe => format!("The approved {label} directory failed safety checks."),
  }
}

fn map_io_error(error: std::io::Error) -> RootIssue {
  match error.kind() {
    ErrorKind::NotFound => RootIssue::Missing,
    ErrorKind::PermissionDenied => RootIssue::Inaccessible,
    _ => RootIssue::Inaccessible,
  }
}

fn is_filesystem_root(path: &Path) -> bool {
  !path
    .components()
    .any(|component| matches!(component, Component::Normal(_)))
}

fn component_matches(actual: &OsStr, expected: &str) -> bool {
  #[cfg(windows)]
  {
    actual
      .to_str()
      .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
  }
  #[cfg(not(windows))]
  {
    actual == OsStr::new(expected)
  }
}

#[cfg(unix)]
fn same_owner(left: &fs::Metadata, right: &fs::Metadata) -> bool {
  use std::os::unix::fs::MetadataExt;

  left.uid() == right.uid()
}

#[cfg(not(unix))]
fn same_owner(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
  true
}

pub(crate) fn is_link_or_reparse_point(metadata: &fs::Metadata) -> bool {
  if metadata.file_type().is_symlink() {
    return true;
  }

  #[cfg(windows)]
  {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
  }
  #[cfg(not(windows))]
  {
    false
  }
}

fn home_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  let variable = "USERPROFILE";
  #[cfg(not(target_os = "windows"))]
  let variable = "HOME";

  env::var_os(variable).map(PathBuf::from)
}

#[cfg(target_os = "linux")]
fn env_path(variable: &str) -> Option<PathBuf> {
  env::var_os(variable).map(PathBuf::from)
}

fn child_path(base: Option<PathBuf>, segments: &[&str]) -> Option<PathBuf> {
  base.map(|mut path| {
    for segment in segments {
      path.push(segment);
    }
    path
  })
}

fn spec(
  category: CandidateCategory,
  label: &'static str,
  path: Option<PathBuf>,
  boundary: Option<PathBuf>,
  risk: CandidateRisk,
  reason: &'static str,
) -> AllowedRoot {
  AllowedRoot::new(category, label, path, boundary, risk, reason)
}

#[cfg(target_os = "macos")]
fn macos_roots() -> PlatformRoots {
  use CandidateCategory::{DiagnosticLog, UserCache, UserTemporary};
  use CandidateRisk::{High, Low, Moderate};

  let home = home_dir();
  let mut temp_root = spec(
    UserTemporary,
    "user temporary files",
    Some(env::temp_dir()),
    Some(PathBuf::from("/var/folders")),
    Moderate,
    "Temporary file in the current user's isolated macOS temp directory.",
  );
  temp_root.owner_anchor = home.clone();
  temp_root.required_leaf = Some("T");
  temp_root.minimum_relative_components = 3;
  temp_root.required_relative_component_count = Some(3);

  let mut cache_root = spec(
    UserCache,
    "user cache",
    child_path(home.clone(), &["Library", "Caches"]),
    home.clone(),
    Low,
    "Recreatable cache file in the current user's Library/Caches directory.",
  );
  cache_root.owner_anchor = home.clone();
  cache_root.required_leaf = Some("Caches");
  cache_root.minimum_relative_components = 2;
  cache_root.expected_relative_components = Some(&["Library", "Caches"]);
  let mut log_root = spec(
    DiagnosticLog,
    "diagnostic logs",
    child_path(home.clone(), &["Library", "Logs"]),
    home.clone(),
    High,
    "Diagnostic log in the current user's Library/Logs directory.",
  );
  log_root.owner_anchor = home;
  log_root.required_leaf = Some("Logs");
  log_root.minimum_relative_components = 2;
  log_root.expected_relative_components = Some(&["Library", "Logs"]);

  PlatformRoots {
    roots: vec![cache_root, temp_root, log_root],
    unsupported_categories: Vec::new(),
  }
}

#[cfg(target_os = "windows")]
fn windows_roots() -> PlatformRoots {
  use CandidateCategory::{DiagnosticLog, UserCache, UserTemporary};
  use CandidateRisk::{High, Low, Moderate};

  let home = home_dir();
  let local_app_data = child_path(home.clone(), &["AppData", "Local"]);

  let mut cache_root = spec(
    UserCache,
    "user cache",
    child_path(
      local_app_data.clone(),
      &["Microsoft", "Windows", "INetCache"],
    ),
    home.clone(),
    Low,
    "Internet cache file in the current user's AppData/Local directory.",
  );
  cache_root.owner_anchor = home.clone();
  cache_root.required_leaf = Some("INetCache");
  cache_root.minimum_relative_components = 4;
  cache_root.expected_relative_components =
    Some(&["AppData", "Local", "Microsoft", "Windows", "INetCache"]);
  let mut temporary_root = spec(
    UserTemporary,
    "user temporary files",
    child_path(local_app_data.clone(), &["Temp"]),
    home.clone(),
    Moderate,
    "Temporary file in the current user's AppData/Local/Temp directory.",
  );
  temporary_root.owner_anchor = home.clone();
  temporary_root.required_leaf = Some("Temp");
  temporary_root.minimum_relative_components = 3;
  temporary_root.expected_relative_components = Some(&["AppData", "Local", "Temp"]);
  let mut log_root = spec(
    DiagnosticLog,
    "diagnostic logs",
    child_path(local_app_data, &["CrashDumps"]),
    home.clone(),
    High,
    "Diagnostic crash dump in the current user's AppData/Local/CrashDumps directory.",
  );
  log_root.owner_anchor = home;
  log_root.required_leaf = Some("CrashDumps");
  log_root.minimum_relative_components = 3;
  log_root.expected_relative_components = Some(&["AppData", "Local", "CrashDumps"]);

  PlatformRoots {
    roots: vec![cache_root, temporary_root, log_root],
    unsupported_categories: Vec::new(),
  }
}

#[cfg(target_os = "linux")]
fn linux_roots() -> PlatformRoots {
  use CandidateCategory::{DiagnosticLog, UserCache};
  use CandidateRisk::{High, Low};

  let home = home_dir();
  let cache = env_path("XDG_CACHE_HOME").or_else(|| child_path(home.clone(), &[".cache"]));
  let state = env_path("XDG_STATE_HOME").or_else(|| child_path(home.clone(), &[".local", "state"]));

  let mut cache_root = spec(
    UserCache,
    "user cache",
    cache,
    home.clone(),
    Low,
    "Cache file in the current user's XDG cache directory.",
  );
  cache_root.owner_anchor = home.clone();
  let mut log_root = spec(
    DiagnosticLog,
    "diagnostic logs",
    child_path(state, &["logs"]),
    home.clone(),
    High,
    "Diagnostic log in the current user's XDG state logs directory.",
  );
  log_root.owner_anchor = home;

  PlatformRoots {
    roots: vec![cache_root, log_root],
    unsupported_categories: vec![CandidateCategory::UserTemporary],
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn all_categories() -> Vec<CandidateCategory> {
  vec![
    CandidateCategory::UserCache,
    CandidateCategory::UserTemporary,
    CandidateCategory::DiagnosticLog,
  ]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rejects_root_as_an_approved_directory() {
    let root = AllowedRoot::new(
      CandidateCategory::UserCache,
      "test cache",
      Some(PathBuf::from("/")),
      Some(PathBuf::from("/")),
      CandidateRisk::Low,
      "test",
    );

    assert!(matches!(resolve_root(&root), Err(RootIssue::Unsafe)));
  }

  #[test]
  fn rejects_a_filesystem_root_as_the_user_boundary() {
    let directory = tempfile::tempdir().unwrap();
    let root = AllowedRoot::new(
      CandidateCategory::UserCache,
      "test cache",
      Some(directory.path().to_path_buf()),
      Some(PathBuf::from("/")),
      CandidateRisk::Low,
      "test",
    );

    assert!(matches!(resolve_root(&root), Err(RootIssue::Unsafe)));
  }
}
