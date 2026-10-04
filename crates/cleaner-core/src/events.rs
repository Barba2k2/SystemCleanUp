use serde::{Deserialize, Serialize};

use crate::model::{ApplicationId, CandidateId, PreviewId, ScanId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
  CandidateScan,
  CleanupPreview,
  Cleanup,
  ApplicationDiscovery,
  Uninstall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
  pub operation_id: String,
  pub operation: OperationKind,
  pub completed_units: u64,
  pub total_units: Option<u64>,
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
  Progress(ProgressEvent),
  CandidatesDiscovered {
    scan_id: ScanId,
    candidate_ids: Vec<CandidateId>,
  },
  CleanupPreviewPrepared {
    preview_id: PreviewId,
    selected_count: usize,
  },
  CleanupFinished {
    removed_candidate_ids: Vec<CandidateId>,
    failed_candidate_ids: Vec<CandidateId>,
  },
  ApplicationsDiscovered {
    application_ids: Vec<ApplicationId>,
  },
  NativeUninstallFinished {
    application_id: ApplicationId,
  },
}
