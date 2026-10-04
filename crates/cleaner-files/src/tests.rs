use std::{
  fs,
  path::{Path, PathBuf},
  sync::Mutex,
  time::Duration,
};

use cleaner_core::{
  CandidateCategory, CandidateId, CandidatePreview, CandidateRisk, CleanupRequest, DomainEvent,
  EventPublisher, PortError, PreviewRequest, RemovalMode, ScanRequest,
};
use tempfile::TempDir;

use super::{FileCleaner, ScannedCandidate, MAX_WARNINGS_PER_SCAN};
use crate::roots::AllowedRoot;

#[derive(Default)]
struct RecordingPublisher;

impl EventPublisher for RecordingPublisher {
  fn publish(&self, _event: DomainEvent) {}
}

#[derive(Default)]
struct EventRecorder(Mutex<Vec<DomainEvent>>);

impl EventPublisher for EventRecorder {
  fn publish(&self, event: DomainEvent) {
    self.0.lock().unwrap().push(event);
  }
}

struct Fixture {
  _directory: TempDir,
  root: PathBuf,
  cleaner: FileCleaner,
}

fn fixture() -> Fixture {
  let directory = tempfile::tempdir().unwrap();
  let root = directory.path().join("user-cache");
  fs::create_dir(&root).unwrap();
  let allowed_root = AllowedRoot {
    category: CandidateCategory::UserCache,
    label: "test cache",
    path: Some(root.clone()),
    boundary: Some(directory.path().to_path_buf()),
    owner_anchor: None,
    required_leaf: None,
    minimum_relative_components: 0,
    required_relative_component_count: None,
    expected_relative_components: None,
    risk: CandidateRisk::Low,
    reason: "Test file under the approved cache root.",
  };
  let cleaner = FileCleaner::with_roots(vec![allowed_root], Vec::new());

  Fixture {
    _directory: directory,
    root,
    cleaner,
  }
}

fn add_file(root: &Path, name: &str, contents: &[u8]) -> PathBuf {
  let path = root.join(name);
  fs::write(&path, contents).unwrap();
  path
}

fn scan(fixture: &Fixture) -> cleaner_core::ScanResponse {
  fixture
    .cleaner
    .scan(
      ScanRequest {
        categories: vec![CandidateCategory::UserCache],
      },
      &RecordingPublisher,
    )
    .unwrap()
}

fn preview(
  fixture: &Fixture,
  scan: &cleaner_core::ScanResponse,
  id: CandidateId,
) -> cleaner_core::CleanupPreview {
  fixture
    .cleaner
    .prepare_preview(
      PreviewRequest {
        scan_id: scan.scan_id.clone(),
        selected_candidate_ids: vec![id],
      },
      &RecordingPublisher,
    )
    .unwrap()
}

fn cleanup_request(
  preview_id: cleaner_core::PreviewId,
  confirmed: bool,
  removal_mode: RemovalMode,
) -> CleanupRequest {
  CleanupRequest {
    preview_id,
    confirmed,
    removal_mode,
  }
}

fn only_candidate_id(scan: &cleaner_core::ScanResponse) -> CandidateId {
  assert_eq!(scan.candidates.len(), 1);
  scan.candidates[0].id.clone()
}

fn stored_candidate_mut<'a>(
  cleaner: &'a FileCleaner,
  scan_id: &cleaner_core::ScanId,
  candidate_id: &CandidateId,
) -> std::sync::MutexGuard<'a, super::Catalog> {
  let mut catalog = cleaner.catalog.lock().unwrap();
  let scanned_candidate: &mut ScannedCandidate = catalog
    .scans
    .get_mut(scan_id)
    .unwrap()
    .candidates
    .get_mut(candidate_id)
    .unwrap();
  let _ = scanned_candidate;
  catalog
}

#[test]
fn preview_only_contains_ids_selected_from_the_scan_catalog() {
  let fixture = fixture();
  add_file(&fixture.root, "first.cache", b"first");
  add_file(&fixture.root, "second.cache", b"second");
  let scan = scan(&fixture);
  let selected_id = scan.candidates[0].id.clone();

  let preview = preview(&fixture, &scan, selected_id.clone());

  assert_eq!(preview.entries.len(), 1);
  assert!(matches!(
    &preview.entries[0],
    CandidatePreview::Eligible { candidate } if candidate.id == selected_id
  ));
}

#[test]
fn preview_does_not_mutate_a_selected_file() {
  let fixture = fixture();
  let path = add_file(&fixture.root, "keep.cache", b"still here");
  let scan = scan(&fixture);
  let id = only_candidate_id(&scan);

  let preview = preview(&fixture, &scan, id);

  assert_eq!(fs::read(&path).unwrap(), b"still here");
  assert!(matches!(
    preview.entries.as_slice(),
    [CandidatePreview::Eligible { .. }]
  ));
}

#[test]
fn preview_publishes_a_typed_domain_event() {
  let fixture = fixture();
  add_file(&fixture.root, "preview.cache", b"preview");
  let scan = scan(&fixture);
  let recorder = EventRecorder::default();

  fixture
    .cleaner
    .prepare_preview(
      PreviewRequest {
        scan_id: scan.scan_id.clone(),
        selected_candidate_ids: vec![only_candidate_id(&scan)],
      },
      &recorder,
    )
    .unwrap();

  assert!(matches!(
    recorder.0.lock().unwrap().as_slice(),
    [DomainEvent::CleanupPreviewPrepared {
      selected_count: 1,
      ..
    }]
  ));
}

#[test]
fn fabricated_candidate_ids_are_rejected() {
  let fixture = fixture();
  add_file(&fixture.root, "known.cache", b"known");
  let scan = scan(&fixture);

  let result = fixture.cleaner.prepare_preview(
    PreviewRequest {
      scan_id: scan.scan_id,
      selected_candidate_ids: vec![CandidateId("not-in-the-scan-catalog".to_string())],
    },
    &RecordingPublisher,
  );

  assert!(matches!(result, Err(PortError::CandidateChanged)));
}

#[test]
fn cleanup_requires_explicit_confirmation_and_keeps_preview_available() {
  let fixture = fixture();
  let path = add_file(&fixture.root, "confirm.cache", b"confirm");
  let scan = scan(&fixture);
  let candidate_id = only_candidate_id(&scan);
  let preview = preview(&fixture, &scan, candidate_id.clone());

  let result = fixture.cleaner.execute_cleanup(
    cleanup_request(preview.preview_id.clone(), false, RemovalMode::Permanent),
    &RecordingPublisher,
  );

  assert!(matches!(result, Err(PortError::ConfirmationRequired)));
  assert_eq!(fs::read(path).unwrap(), b"confirm");

  let confirmed_result = fixture
    .cleaner
    .execute_cleanup(
      cleanup_request(preview.preview_id, true, RemovalMode::Permanent),
      &RecordingPublisher,
    )
    .unwrap();

  assert_eq!(confirmed_result.removed_candidate_ids, vec![candidate_id]);
}

#[test]
fn permanent_cleanup_removes_only_the_selected_regular_file() {
  let fixture = fixture();
  let selected_path = add_file(&fixture.root, "selected.cache", b"remove me");
  let retained_path = add_file(&fixture.root, "retained.cache", b"keep me");
  let scan = scan(&fixture);
  let selected_id = scan
    .candidates
    .iter()
    .find(|candidate| candidate.path.ends_with("selected.cache"))
    .unwrap()
    .id
    .clone();
  let preview = preview(&fixture, &scan, selected_id.clone());

  let response = fixture
    .cleaner
    .execute_cleanup(
      cleanup_request(preview.preview_id, true, RemovalMode::Permanent),
      &RecordingPublisher,
    )
    .unwrap();

  assert_eq!(response.removed_candidate_ids, vec![selected_id]);
  assert!(response.failed_candidate_ids.is_empty());
  assert!(!selected_path.exists());
  assert_eq!(fs::read(retained_path).unwrap(), b"keep me");
}

#[cfg(unix)]
#[test]
fn scan_does_not_follow_directory_symlinks() {
  use std::os::unix::fs::symlink;

  let fixture = fixture();
  let outside = tempfile::tempdir().unwrap();
  add_file(outside.path(), "protected.cache", b"protected");
  symlink(outside.path(), fixture.root.join("linked-directory")).unwrap();

  let response = scan(&fixture);

  assert!(response.candidates.is_empty());
  assert!(response
    .warnings
    .iter()
    .any(|warning| warning.contains("symbolic link or reparse point")));
  assert_eq!(
    fs::read(outside.path().join("protected.cache")).unwrap(),
    b"protected"
  );
}

#[cfg(unix)]
#[cfg_attr(target_os = "macos", ignore = "filesystem rejects non-UTF-8 filenames")]
#[test]
fn scan_skips_non_utf8_candidate_paths_with_a_warning() {
  use std::os::unix::ffi::OsStringExt;

  let fixture = fixture();
  for index in 0..(MAX_WARNINGS_PER_SCAN + 5) {
    let mut invalid_name = format!("cache-{index}-").into_bytes();
    invalid_name.push(0xff);
    fs::write(
      fixture
        .root
        .join(std::ffi::OsString::from_vec(invalid_name)),
      b"unserializable path",
    )
    .unwrap();
  }

  let response = scan(&fixture);

  assert!(response.candidates.is_empty());
  assert_eq!(response.warnings.len(), MAX_WARNINGS_PER_SCAN + 1);
  assert!(response
    .warnings
    .iter()
    .any(|warning| warning.contains("non-UTF-8 path") && warning.contains("test cache")));
  assert!(response
    .warnings
    .last()
    .unwrap()
    .contains("additional scan warning(s) were omitted"));
}

#[test]
fn candidate_path_outside_the_approved_root_is_blocked() {
  let fixture = fixture();
  add_file(&fixture.root, "inside.cache", b"inside");
  let outside_path = fixture._directory.path().join("outside.cache");
  add_file(fixture._directory.path(), "outside.cache", b"outside");
  let scan = scan(&fixture);
  let candidate_id = only_candidate_id(&scan);

  {
    let mut catalog = stored_candidate_mut(&fixture.cleaner, &scan.scan_id, &candidate_id);
    catalog
      .scans
      .get_mut(&scan.scan_id)
      .unwrap()
      .candidates
      .get_mut(&candidate_id)
      .unwrap()
      .candidate
      .path = outside_path.clone();
  }

  let preview = preview(&fixture, &scan, candidate_id.clone());
  assert!(matches!(
    &preview.entries[0],
    CandidatePreview::Blocked { .. }
  ));

  let response = fixture
    .cleaner
    .execute_cleanup(
      cleanup_request(preview.preview_id, true, RemovalMode::Permanent),
      &RecordingPublisher,
    )
    .unwrap();

  assert_eq!(response.failed_candidate_ids, vec![candidate_id]);
  assert_eq!(fs::read(outside_path).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn symlink_replacement_is_blocked_without_touching_its_target() {
  use std::os::unix::fs::symlink;

  let fixture = fixture();
  let target = fixture._directory.path().join("outside-target");
  add_file(fixture._directory.path(), "outside-target", b"protected");
  let candidate_path = add_file(&fixture.root, "candidate.cache", b"original");
  let scan = scan(&fixture);
  let candidate_id = only_candidate_id(&scan);

  fs::remove_file(&candidate_path).unwrap();
  symlink(&target, &candidate_path).unwrap();

  let preview = preview(&fixture, &scan, candidate_id.clone());
  assert!(matches!(
    &preview.entries[0],
    CandidatePreview::Blocked { .. }
  ));

  let response = fixture
    .cleaner
    .execute_cleanup(
      cleanup_request(preview.preview_id, true, RemovalMode::Permanent),
      &RecordingPublisher,
    )
    .unwrap();

  assert_eq!(response.failed_candidate_ids, vec![candidate_id]);
  assert_eq!(fs::read(target).unwrap(), b"protected");
  assert!(fs::symlink_metadata(candidate_path)
    .unwrap()
    .file_type()
    .is_symlink());
}

#[test]
fn changed_file_is_rejected_between_preview_and_cleanup() {
  let fixture = fixture();
  let path = add_file(&fixture.root, "changed.cache", b"same");
  let scan = scan(&fixture);
  let candidate_id = only_candidate_id(&scan);
  let preview = preview(&fixture, &scan, candidate_id.clone());
  let scanned_modified = fs::metadata(&path).unwrap().modified().unwrap();
  let later_modified = scanned_modified + Duration::from_secs(20);
  fs::write(&path, b"same").unwrap();
  filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(later_modified)).unwrap();

  let response = fixture
    .cleaner
    .execute_cleanup(
      cleanup_request(preview.preview_id, true, RemovalMode::Permanent),
      &RecordingPublisher,
    )
    .unwrap();

  assert_eq!(response.failed_candidate_ids, vec![candidate_id]);
  assert_eq!(fs::read(path).unwrap(), b"same");
}

#[test]
fn scan_reports_categories_without_approved_roots() {
  let cleaner = FileCleaner::with_roots(Vec::new(), vec![CandidateCategory::UserTemporary]);

  let response = cleaner
    .scan(
      ScanRequest {
        categories: vec![CandidateCategory::UserTemporary],
      },
      &RecordingPublisher,
    )
    .unwrap();

  assert!(response.candidates.is_empty());
  assert!(response.warnings[0].contains("not supported"));
}
