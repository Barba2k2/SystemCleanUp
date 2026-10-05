use std::{
  collections::{HashMap, HashSet, VecDeque},
  fs::{self, Metadata},
  io::ErrorKind,
  path::{Path, PathBuf},
  sync::{Mutex, MutexGuard},
  time::SystemTime,
};

use cleaner_core::{
  CandidateCategory, CandidateId, CandidatePreview, CleanupPreview, CleanupRequest,
  CleanupResponse, DomainEvent, EventPublisher, OperationKind, PortError, PreviewId,
  PreviewRequest, ProgressEvent, RemovalCandidate, RemovalMode, ScanId, ScanRequest, ScanResponse,
};
use uuid::Uuid;

use crate::roots::{
  is_link_or_reparse_point, platform_roots, resolve_root, root_issue_message, AllowedRoot,
  ResolvedRoot, RootIssue,
};

const MAX_ENTRIES_PER_SCAN: usize = 100_000;
const MAX_CANDIDATES_PER_SCAN: usize = 20_000;
const MAX_WARNINGS_PER_SCAN: usize = 100;
const MAX_STORED_SCANS: usize = 8;
const MAX_STORED_PREVIEWS: usize = 8;

#[derive(Debug)]
pub struct FileCleaner {
  roots: Vec<AllowedRoot>,
  unsupported_categories: Vec<CandidateCategory>,
  catalog: Mutex<Catalog>,
}

#[derive(Debug, Default)]
struct Catalog {
  scans: HashMap<ScanId, StoredScan>,
  scan_order: VecDeque<ScanId>,
  previews: HashMap<PreviewId, StoredPreview>,
  preview_order: VecDeque<PreviewId>,
}

#[derive(Debug, Clone)]
struct StoredScan {
  candidates: HashMap<CandidateId, ScannedCandidate>,
}

#[derive(Debug, Clone)]
struct ScannedCandidate {
  candidate: RemovalCandidate,
  root: ResolvedRoot,
  fingerprint: FileFingerprint,
}

#[derive(Debug, Clone)]
struct StoredPreview {
  scan_id: ScanId,
  entries: Vec<PreviewEntry>,
}

#[derive(Debug, Clone)]
struct PreviewEntry {
  candidate: RemovalCandidate,
  root: ResolvedRoot,
  fingerprint: FileFingerprint,
  eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
  size_bytes: u64,
  modified: SystemTime,
}

struct WarningCollector {
  warnings: Vec<String>,
  omitted: usize,
}

struct RootWalk {
  candidates: Vec<ScannedCandidate>,
  visited_entries: usize,
  limit_reached: bool,
}

impl Default for WarningCollector {
  fn default() -> Self {
    Self {
      warnings: Vec::new(),
      omitted: 0,
    }
  }
}

impl WarningCollector {
  fn push(&mut self, warning: impl Into<String>) {
    if self.warnings.len() < MAX_WARNINGS_PER_SCAN {
      self.warnings.push(warning.into());
    } else {
      self.omitted += 1;
    }
  }

  fn finish(mut self) -> Vec<String> {
    if self.omitted > 0 {
      self.warnings.push(format!(
        "{} additional scan warning(s) were omitted.",
        self.omitted
      ));
    }
    self.warnings
  }
}

impl FileCleaner {
  pub fn new() -> Self {
    let platform = platform_roots();
    Self {
      roots: platform.roots,
      unsupported_categories: platform.unsupported_categories,
      catalog: Mutex::new(Catalog::default()),
    }
  }

  pub fn scan(
    &self,
    request: ScanRequest,
    events: &dyn EventPublisher,
  ) -> Result<ScanResponse, PortError> {
    let scan_id = ScanId(Uuid::new_v4().to_string());
    let mut warnings = WarningCollector::default();
    let mut seen_categories = HashSet::new();
    let mut scanned_candidates = Vec::new();
    let mut visited_entries = 0;
    let mut limit_reached = false;

    for category in request.categories {
      if !seen_categories.insert(category) {
        continue;
      }

      let category_roots: Vec<_> = self
        .roots
        .iter()
        .filter(|root| root.category == category)
        .collect();

      if category_roots.is_empty() {
        warnings.push(if self.unsupported_categories.contains(&category) {
          format!(
            "Category {} is not supported on this operating system.",
            category_name(category)
          )
        } else {
          format!(
            "No approved root is configured for {}.",
            category_name(category)
          )
        });
        continue;
      }

      for root_spec in category_roots {
        let root = match resolve_root(root_spec) {
          Ok(root) => root,
          Err(issue) => {
            warnings.push(root_issue_message(issue, root_spec.label));
            continue;
          }
        };

        let remaining_entries = MAX_ENTRIES_PER_SCAN.saturating_sub(visited_entries);
        let remaining_candidates = MAX_CANDIDATES_PER_SCAN.saturating_sub(scanned_candidates.len());
        if remaining_entries == 0 || remaining_candidates == 0 {
          limit_reached = true;
          break;
        }

        let walk = walk_root(
          &root,
          remaining_entries,
          remaining_candidates,
          &mut warnings,
        );
        visited_entries += walk.visited_entries;
        scanned_candidates.extend(walk.candidates);
        if walk.limit_reached {
          limit_reached = true;
          break;
        }

        events.publish(DomainEvent::Progress(ProgressEvent {
          operation_id: scan_id.0.clone(),
          operation: OperationKind::CandidateScan,
          completed_units: visited_entries as u64,
          total_units: None,
          message: format!("Scanned the approved {} root.", root.spec.label),
        }));
      }

      if limit_reached {
        break;
      }
    }

    if limit_reached {
      warnings.push(format!(
        "The scan stopped at its safety limit of {MAX_ENTRIES_PER_SCAN} entries or {MAX_CANDIDATES_PER_SCAN} candidates."
      ));
    }

    scanned_candidates.sort_by(|left, right| left.candidate.path.cmp(&right.candidate.path));
    let candidates = scanned_candidates
      .iter()
      .map(|scanned| scanned.candidate.clone())
      .collect::<Vec<_>>();
    let stored_candidates = scanned_candidates
      .into_iter()
      .map(|scanned| (scanned.candidate.id.clone(), scanned))
      .collect();
    let response = ScanResponse {
      scan_id: scan_id.clone(),
      candidates,
      warnings: warnings.finish(),
    };

    self.store_scan(scan_id.clone(), stored_candidates)?;
    events.publish(DomainEvent::CandidatesDiscovered {
      scan_id,
      candidate_ids: response
        .candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect(),
    });

    Ok(response)
  }

  pub fn prepare_preview(
    &self,
    request: PreviewRequest,
    events: &dyn EventPublisher,
  ) -> Result<CleanupPreview, PortError> {
    let selected_candidates = {
      let catalog = self.lock_catalog()?;
      let scan = catalog
        .scans
        .get(&request.scan_id)
        .ok_or(PortError::CandidateChanged)?;
      let mut seen_ids = HashSet::new();
      let mut selected = Vec::with_capacity(request.selected_candidate_ids.len());

      for candidate_id in request.selected_candidate_ids {
        if !seen_ids.insert(candidate_id.clone()) {
          return Err(selection_error(
            "The selection contains a duplicate candidate ID.",
          ));
        }

        selected.push(
          scan
            .candidates
            .get(&candidate_id)
            .cloned()
            .ok_or(PortError::CandidateChanged)?,
        );
      }

      selected
    };

    let preview_id = PreviewId(Uuid::new_v4().to_string());
    let mut preview_entries = Vec::with_capacity(selected_candidates.len());
    let mut stored_entries = Vec::with_capacity(selected_candidates.len());

    for scanned in selected_candidates {
      let validation = validate_candidate(&scanned);
      let (eligible, public_entry) = match validation {
        Ok(()) => (
          true,
          CandidatePreview::Eligible {
            candidate: scanned.candidate.clone(),
          },
        ),
        Err(error) => (
          false,
          CandidatePreview::Blocked {
            candidate: scanned.candidate.clone(),
            reason: error.to_string(),
          },
        ),
      };

      preview_entries.push(public_entry);
      stored_entries.push(PreviewEntry {
        candidate: scanned.candidate,
        root: scanned.root,
        fingerprint: scanned.fingerprint,
        eligible,
      });
    }

    let response = CleanupPreview {
      preview_id: preview_id.clone(),
      entries: preview_entries,
    };
    self.store_preview(
      preview_id,
      StoredPreview {
        scan_id: request.scan_id,
        entries: stored_entries,
      },
    )?;
    events.publish(DomainEvent::CleanupPreviewPrepared {
      preview_id: response.preview_id.clone(),
      selected_count: response.entries.len(),
    });

    Ok(response)
  }

  pub fn execute_cleanup(
    &self,
    request: CleanupRequest,
    events: &dyn EventPublisher,
  ) -> Result<CleanupResponse, PortError> {
    if !request.confirmed {
      return Err(PortError::ConfirmationRequired);
    }

    let preview = self.take_preview(&request.preview_id)?;
    let total = preview.entries.len();
    let mut removed_candidate_ids = Vec::new();
    let mut failed_candidate_ids = Vec::new();

    events.publish(DomainEvent::Progress(ProgressEvent {
      operation_id: request.preview_id.0.clone(),
      operation: OperationKind::Cleanup,
      completed_units: 0,
      total_units: Some(total as u64),
      message: "Cleanup started for the explicitly selected preview.".to_string(),
    }));

    for entry in preview.entries {
      let result = if entry.eligible {
        let snapshot = ScannedCandidate {
          candidate: entry.candidate.clone(),
          root: entry.root,
          fingerprint: entry.fingerprint,
        };
        validate_candidate(&snapshot)
          .and_then(|()| remove_file(&request.removal_mode, &snapshot.candidate.path))
      } else {
        Err(PortError::CandidateChanged)
      };

      match result {
        Ok(()) => removed_candidate_ids.push(entry.candidate.id),
        Err(_) => failed_candidate_ids.push(entry.candidate.id),
      }
    }

    events.publish(DomainEvent::Progress(ProgressEvent {
      operation_id: request.preview_id.0.clone(),
      operation: OperationKind::Cleanup,
      completed_units: total as u64,
      total_units: Some(total as u64),
      message: format!("Processed {total} explicitly selected candidate(s)."),
    }));
    events.publish(DomainEvent::CleanupFinished {
      removed_candidate_ids: removed_candidate_ids.clone(),
      failed_candidate_ids: failed_candidate_ids.clone(),
    });

    Ok(CleanupResponse {
      removed_candidate_ids,
      failed_candidate_ids,
    })
  }

  fn store_scan(
    &self,
    scan_id: ScanId,
    candidates: HashMap<CandidateId, ScannedCandidate>,
  ) -> Result<(), PortError> {
    let mut catalog = self.lock_catalog()?;

    while catalog.scan_order.len() >= MAX_STORED_SCANS {
      if let Some(expired_scan_id) = catalog.scan_order.pop_front() {
        catalog.scans.remove(&expired_scan_id);
        let expired_previews = catalog
          .previews
          .iter()
          .filter(|(_, preview)| preview.scan_id == expired_scan_id)
          .map(|(preview_id, _)| preview_id.clone())
          .collect::<Vec<_>>();
        for preview_id in expired_previews {
          catalog.previews.remove(&preview_id);
          catalog
            .preview_order
            .retain(|stored_id| stored_id != &preview_id);
        }
      }
    }

    catalog.scan_order.push_back(scan_id.clone());
    catalog.scans.insert(scan_id, StoredScan { candidates });
    Ok(())
  }

  fn store_preview(&self, preview_id: PreviewId, preview: StoredPreview) -> Result<(), PortError> {
    let mut catalog = self.lock_catalog()?;

    while catalog.preview_order.len() >= MAX_STORED_PREVIEWS {
      if let Some(expired_preview_id) = catalog.preview_order.pop_front() {
        catalog.previews.remove(&expired_preview_id);
      }
    }

    catalog.preview_order.push_back(preview_id.clone());
    catalog.previews.insert(preview_id, preview);
    Ok(())
  }

  fn take_preview(&self, preview_id: &PreviewId) -> Result<StoredPreview, PortError> {
    let mut catalog = self.lock_catalog()?;
    let preview = catalog
      .previews
      .remove(preview_id)
      .ok_or(PortError::CandidateChanged)?;
    catalog
      .preview_order
      .retain(|stored_id| stored_id != preview_id);
    Ok(preview)
  }

  fn lock_catalog(&self) -> Result<MutexGuard<'_, Catalog>, PortError> {
    self.catalog.lock().map_err(|_| PortError::OperationFailed {
      message: "The in-process cleanup catalog is unavailable.".to_string(),
    })
  }

  #[cfg(test)]
  fn with_roots(roots: Vec<AllowedRoot>, unsupported_categories: Vec<CandidateCategory>) -> Self {
    Self {
      roots,
      unsupported_categories,
      catalog: Mutex::new(Catalog::default()),
    }
  }
}

impl Default for FileCleaner {
  fn default() -> Self {
    Self::new()
  }
}

fn walk_root(
  root: &ResolvedRoot,
  max_entries: usize,
  max_candidates: usize,
  warnings: &mut WarningCollector,
) -> RootWalk {
  let mut pending_directories = vec![root.canonical_path.clone()];
  let mut candidates = Vec::new();
  let mut visited_entries = 0;
  let mut limit_reached = false;

  'walk: while let Some(directory) = pending_directories.pop() {
    let entries = match fs::read_dir(&directory) {
      Ok(entries) => entries,
      Err(error) => {
        warnings.push(io_warning(root.spec.label, &error));
        continue;
      }
    };

    for entry_result in entries {
      if visited_entries >= max_entries || candidates.len() >= max_candidates {
        limit_reached = true;
        break 'walk;
      }
      visited_entries += 1;

      let entry = match entry_result {
        Ok(entry) => entry,
        Err(error) => {
          warnings.push(io_warning(root.spec.label, &error));
          continue;
        }
      };
      let path = entry.path();
      let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) => {
          warnings.push(io_warning(root.spec.label, &error));
          continue;
        }
      };

      if is_link_or_reparse_point(&metadata) {
        warnings.push(format!(
          "A symbolic link or reparse point in the approved {} root was skipped.",
          root.spec.label
        ));
        continue;
      }

      if metadata.is_dir() {
        match inspect_directory(&path, root) {
          Ok(Some(canonical_directory)) => pending_directories.push(canonical_directory),
          Ok(None) => {}
          Err(error) => warnings.push(error.to_string()),
        }
        continue;
      }

      if !metadata.is_file() {
        continue;
      }

      match inspect_file(&path, root, warnings) {
        Ok(Some(candidate)) => candidates.push(candidate),
        Ok(None) => {}
        Err(error) => warnings.push(error.to_string()),
      }
    }
  }

  RootWalk {
    candidates,
    visited_entries,
    limit_reached,
  }
}

fn inspect_directory(path: &Path, root: &ResolvedRoot) -> Result<Option<PathBuf>, PortError> {
  let metadata = fs::symlink_metadata(path).map_err(io_to_operation_error)?;
  if is_link_or_reparse_point(&metadata) || !metadata.is_dir() {
    return Ok(None);
  }

  let canonical_path = fs::canonicalize(path).map_err(io_to_operation_error)?;
  if !canonical_path.starts_with(&root.canonical_path) {
    return Err(PortError::UnsafeTarget);
  }

  Ok(Some(canonical_path))
}

fn inspect_file(
  path: &Path,
  root: &ResolvedRoot,
  warnings: &mut WarningCollector,
) -> Result<Option<ScannedCandidate>, PortError> {
  let first_metadata = fs::symlink_metadata(path).map_err(io_to_operation_error)?;
  if is_link_or_reparse_point(&first_metadata) || !first_metadata.is_file() {
    return Ok(None);
  }

  let canonical_path = fs::canonicalize(path).map_err(io_to_operation_error)?;
  if canonical_path == root.canonical_path || !canonical_path.starts_with(&root.canonical_path) {
    return Err(PortError::UnsafeTarget);
  }
  if canonical_path.to_str().is_none() {
    warnings.push(format!(
      "A file with a non-UTF-8 path in the approved {} root was skipped.",
      root.spec.label
    ));
    return Ok(None);
  }

  let current_metadata = fs::symlink_metadata(path).map_err(io_to_operation_error)?;
  if is_link_or_reparse_point(&current_metadata) || !current_metadata.is_file() {
    return Ok(None);
  }

  let first_fingerprint = FileFingerprint::from_metadata(&first_metadata)?;
  let current_fingerprint = FileFingerprint::from_metadata(&current_metadata)?;
  if first_fingerprint != current_fingerprint {
    return Ok(None);
  }

  let candidate = RemovalCandidate {
    id: CandidateId(Uuid::new_v4().to_string()),
    category: root.spec.category,
    path: canonical_path,
    size_bytes: current_fingerprint.size_bytes,
    reason: root.spec.reason.to_string(),
    risk: root.spec.risk,
  };
  Ok(Some(ScannedCandidate {
    candidate,
    root: root.clone(),
    fingerprint: current_fingerprint,
  }))
}

fn validate_candidate(candidate: &ScannedCandidate) -> Result<(), PortError> {
  if candidate.candidate.category != candidate.root.spec.category
    || !candidate.candidate.path.is_absolute()
  {
    return Err(PortError::UnsafeTarget);
  }

  let current_root = resolve_root(&candidate.root.spec).map_err(|issue| match issue {
    RootIssue::Unsafe => PortError::UnsafeTarget,
    RootIssue::Missing | RootIssue::Inaccessible => PortError::CandidateChanged,
  })?;
  if current_root.canonical_path != candidate.root.canonical_path {
    return Err(PortError::UnsafeTarget);
  }

  let metadata =
    fs::symlink_metadata(&candidate.candidate.path).map_err(|_| PortError::CandidateChanged)?;
  if is_link_or_reparse_point(&metadata) || !metadata.is_file() {
    return Err(PortError::UnsafeTarget);
  }

  let canonical_path =
    fs::canonicalize(&candidate.candidate.path).map_err(|_| PortError::CandidateChanged)?;
  if canonical_path != candidate.candidate.path
    || canonical_path == current_root.canonical_path
    || !canonical_path.starts_with(&current_root.canonical_path)
  {
    return Err(PortError::UnsafeTarget);
  }

  let fingerprint = FileFingerprint::from_metadata(&metadata)?;
  if candidate.candidate.size_bytes != candidate.fingerprint.size_bytes
    || fingerprint != candidate.fingerprint
  {
    return Err(PortError::CandidateChanged);
  }

  Ok(())
}

fn remove_file(mode: &RemovalMode, path: &Path) -> Result<(), PortError> {
  let result = match mode {
    RemovalMode::Trash => trash::delete(path).map_err(|error| error.to_string()),
    RemovalMode::Permanent => fs::remove_file(path).map_err(|error| error.to_string()),
  };

  result.map_err(|message| PortError::OperationFailed { message })
}

impl FileFingerprint {
  fn from_metadata(metadata: &Metadata) -> Result<Self, PortError> {
    let modified = metadata
      .modified()
      .map_err(|_| PortError::CandidateChanged)?;

    Ok(Self {
      size_bytes: metadata.len(),
      modified,
    })
  }
}

fn io_warning(label: &str, error: &std::io::Error) -> String {
  if error.kind() == ErrorKind::PermissionDenied {
    format!("An inaccessible entry in the approved {label} root was skipped.")
  } else {
    format!("An entry in the approved {label} root could not be inspected.")
  }
}

fn io_to_operation_error(error: std::io::Error) -> PortError {
  PortError::OperationFailed {
    message: format!("A file-system entry could not be safely inspected: {error}"),
  }
}

fn selection_error(message: &str) -> PortError {
  PortError::OperationFailed {
    message: message.to_string(),
  }
}

fn category_name(category: CandidateCategory) -> &'static str {
  match category {
    CandidateCategory::UserCache => "user_cache",
    CandidateCategory::UserTemporary => "user_temporary",
    CandidateCategory::DiagnosticLog => "diagnostic_log",
  }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
