pub mod events;
pub mod model;
pub mod ports;

pub use events::{DomainEvent, OperationKind, ProgressEvent};
pub use model::{
  ApplicationDiscoveryRequest, ApplicationDiscoveryResponse, ApplicationId, ApplicationSource,
  CandidateCategory, CandidateId, CandidatePreview, CandidateRisk, CleanupPreview, CleanupRequest,
  CleanupResponse, InstalledApplication, PreviewId, PreviewRequest, RemovalCandidate, RemovalMode,
  ScanId, ScanRequest, ScanResponse, UninstallRequest, UninstallResponse, UninstallStatus,
};
pub use ports::{
  ApplicationDiscoveryPort, ApplicationRepositoryPort, CandidateDiscoveryPort,
  CandidateRemovalPort, CandidateRepositoryPort, CandidateRevalidationPort, EventPublisher,
  NativeUninstallPort, PortError, PreviewRepositoryPort, RevalidatedTarget,
};
