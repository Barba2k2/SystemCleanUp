use std::path::PathBuf;

use thiserror::Error;

use crate::{
  model::{
    ApplicationDiscoveryRequest, ApplicationDiscoveryResponse, ApplicationId, CandidateId,
    InstalledApplication, PreviewId, PreviewRequest, RemovalCandidate, ScanId, ScanRequest,
    ScanResponse, UninstallRequest, UninstallResponse,
  },
  DomainEvent,
};

#[derive(Debug, Error)]
pub enum PortError {
  #[error("No platform adapter is registered for {operation}.")]
  AdapterUnavailable { operation: &'static str },
  #[error("The selected candidate is no longer available.")]
  CandidateChanged,
  #[error("The candidate target is outside its approved category root.")]
  UnsafeTarget,
  #[error("The operation requires explicit user confirmation.")]
  ConfirmationRequired,
  #[error("The platform operation failed: {message}")]
  OperationFailed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevalidatedTarget {
  pub candidate_id: CandidateId,
  pub canonical_path: PathBuf,
}

pub trait EventPublisher: Send + Sync {
  fn publish(&self, event: DomainEvent);
}

pub trait CandidateDiscoveryPort: Send + Sync {
  fn scan(
    &self,
    request: &ScanRequest,
    events: &dyn EventPublisher,
  ) -> Result<ScanResponse, PortError>;
}

pub trait CandidateRepositoryPort: Send + Sync {
  fn candidates_for_scan(&self, scan_id: &ScanId) -> Result<Vec<RemovalCandidate>, PortError>;
}

pub trait CandidateRevalidationPort: Send + Sync {
  fn revalidate(&self, candidate: &RemovalCandidate) -> Result<RevalidatedTarget, PortError>;
}

pub trait CandidateRemovalPort: Send + Sync {
  fn remove(&self, target: &RevalidatedTarget) -> Result<(), PortError>;
}

pub trait ApplicationDiscoveryPort: Send + Sync {
  fn discover(
    &self,
    request: &ApplicationDiscoveryRequest,
    events: &dyn EventPublisher,
  ) -> Result<ApplicationDiscoveryResponse, PortError>;
}

pub trait NativeUninstallPort: Send + Sync {
  fn uninstall(
    &self,
    application: &InstalledApplication,
    request: &UninstallRequest,
    events: &dyn EventPublisher,
  ) -> Result<UninstallResponse, PortError>;
}

pub trait ApplicationRepositoryPort: Send + Sync {
  fn get_application(
    &self,
    application_id: &ApplicationId,
  ) -> Result<InstalledApplication, PortError>;
}

pub trait PreviewRepositoryPort: Send + Sync {
  fn selected_candidates(&self, preview_id: &PreviewId)
    -> Result<Vec<RemovalCandidate>, PortError>;
}

pub trait PreviewPreparationPort: Send + Sync {
  fn prepare(&self, request: &PreviewRequest) -> Result<(), PortError>;
}
