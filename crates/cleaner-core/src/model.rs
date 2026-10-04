use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScanId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PreviewId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApplicationId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateCategory {
  UserCache,
  UserTemporary,
  DiagnosticLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateRisk {
  Low,
  Moderate,
  High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalCandidate {
  pub id: CandidateId,
  pub category: CandidateCategory,
  pub path: PathBuf,
  pub size_bytes: u64,
  pub reason: String,
  pub risk: CandidateRisk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRequest {
  pub categories: Vec<CandidateCategory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResponse {
  pub scan_id: ScanId,
  pub candidates: Vec<RemovalCandidate>,
  pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRequest {
  pub scan_id: ScanId,
  pub selected_candidate_ids: Vec<CandidateId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CandidatePreview {
  Eligible {
    candidate: RemovalCandidate,
  },
  Blocked {
    candidate: RemovalCandidate,
    reason: String,
  },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupPreview {
  pub preview_id: PreviewId,
  pub entries: Vec<CandidatePreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupRequest {
  pub preview_id: PreviewId,
  pub removal_mode: RemovalMode,
  pub confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalMode {
  Trash,
  Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupResponse {
  pub removed_candidate_ids: Vec<CandidateId>,
  pub failed_candidate_ids: Vec<CandidateId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationSource {
  MacApplicationsDirectory,
  WindowsRegistry,
  WindowsPackageManager,
  LinuxPackageManager,
  LinuxDesktopEntry,
  Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledApplication {
  pub id: ApplicationId,
  pub name: String,
  pub version: Option<String>,
  pub source: ApplicationSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationDiscoveryRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationDiscoveryResponse {
  pub applications: Vec<InstalledApplication>,
  pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallRequest {
  pub application_id: ApplicationId,
  pub confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallStatus {
  Completed,
  DelegatedToSystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallResponse {
  pub application_id: ApplicationId,
  pub status: UninstallStatus,
}
