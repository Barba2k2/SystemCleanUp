import { useEffect, useMemo, useState } from "react";

import {
  discoverApplications,
  executeCleanup,
  prepareCleanupPreview,
  scanCandidates,
  subscribeToProgress,
  uninstallApplication,
  type ApplicationDiscoveryResponse,
  type CandidateCategory,
  type CleanupResponse,
  type InstalledApplication,
  type OperationProgress,
  type PreviewResponse,
  type RemovalMode,
  type RemovalCandidate,
  type ScanResponse,
  type UninstallResponse,
} from "./api/cleaner";

type View = "cleanup" | "applications";
type FeedbackKind = "info" | "success" | "warning" | "error";
type Feedback = { kind: FeedbackKind; message: string };
type ConfirmationAction =
  | { kind: "cleanup"; mode: RemovalMode }
  | { kind: "uninstall"; application: InstalledApplication };

const scanCategories: CandidateCategory[] = [
  "user_cache",
  "user_temporary",
  "diagnostic_log",
];
const candidatesPerPage = 100;

const categoryLabels: Record<CandidateCategory, string> = {
  user_cache: "User cache",
  user_temporary: "Temporary file",
  diagnostic_log: "Diagnostic log",
};

const applicationSourceLabels: Record<InstalledApplication["source"], string> = {
  mac_applications_directory: "macOS Applications directory",
  windows_registry: "Windows registry",
  windows_package_manager: "Windows package manager",
  linux_package_manager: "Linux package manager",
  linux_desktop_entry: "Linux desktop entry",
  other: "Other source",
};

function getErrorMessage(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return "The request failed. No operation result was returned.";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unitIndex]}`;
}

function getRiskLabel(risk: RemovalCandidate["risk"]): string {
  switch (risk) {
    case "low":
      return "Low risk";
    case "moderate":
      return "Moderate risk";
    case "high":
      return "High risk";
  }
}

function App() {
  const [view, setView] = useState<View>("cleanup");
  const [scan, setScan] = useState<ScanResponse | null>(null);
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [candidatePage, setCandidatePage] = useState(1);
  const [preview, setPreview] = useState<PreviewResponse | null>(null);
  const [cleanupMode, setCleanupMode] = useState<RemovalMode>("trash");
  const [cleanupResponse, setCleanupResponse] =
    useState<CleanupResponse | null>(null);
  const [applicationInventory, setApplicationInventory] =
    useState<ApplicationDiscoveryResponse | null>(null);
  const [selectedApplicationId, setSelectedApplicationId] = useState<
    string | null
  >(null);
  const [uninstallResponse, setUninstallResponse] =
    useState<UninstallResponse | null>(null);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [progress, setProgress] = useState<OperationProgress | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [confirmationAction, setConfirmationAction] =
    useState<ConfirmationAction | null>(null);
  const [confirmationText, setConfirmationText] = useState("");
  const candidateCount = scan?.candidates.length ?? 0;
  const candidatePageCount = Math.max(
    1,
    Math.ceil(candidateCount / candidatesPerPage),
  );
  const pageStartIndex = (candidatePage - 1) * candidatesPerPage;
  const pageCandidates =
    scan?.candidates.slice(pageStartIndex, pageStartIndex + candidatesPerPage) ??
    [];

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let isDisposed = false;

    void subscribeToProgress(setProgress)
      .then((stopListening) => {
        if (isDisposed) {
          stopListening();
          return;
        }

        unlisten = stopListening;
      })
      .catch(() => undefined);

    return () => {
      isDisposed = true;
      unlisten?.();
    };
  }, []);

  async function requestScan() {
    setIsBusy(true);
    setProgress(null);
    setScan(null);
    setSelectedCandidateIds(new Set());
    setCandidatePage(1);
    setPreview(null);
    setCleanupResponse(null);
    setFeedback({ kind: "info", message: "Scanning requested categories…" });

    try {
      const response = await scanCandidates({ categories: scanCategories });
      setScan(response);
      setFeedback({
        kind: "success",
        message: `Scan returned ${response.candidates.length} candidate(s).`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: getErrorMessage(error) });
    } finally {
      setIsBusy(false);
    }
  }

  function updateCandidateSelection(candidateIds: Set<string>) {
    setSelectedCandidateIds(candidateIds);
    setPreview(null);
    setCleanupResponse(null);
  }

  function toggleCandidate(candidateId: string) {
    const nextSelection = new Set(selectedCandidateIds);
    if (nextSelection.has(candidateId)) {
      nextSelection.delete(candidateId);
    } else {
      nextSelection.add(candidateId);
    }

    updateCandidateSelection(nextSelection);
  }

  function selectCurrentPage() {
    if (!scan) {
      return;
    }

    const nextSelection = new Set(selectedCandidateIds);
    pageCandidates.forEach((candidate) => nextSelection.add(candidate.id));
    updateCandidateSelection(nextSelection);
  }

  async function requestPreview() {
    if (!scan || selectedCandidateIds.size === 0 || isBusy) {
      return;
    }

    setIsBusy(true);
    setProgress(null);
    setPreview(null);
    setCleanupResponse(null);
    setFeedback({ kind: "info", message: "Checking the selected candidates…" });

    try {
      const response = await prepareCleanupPreview({
        scan_id: scan.scan_id,
        selected_candidate_ids: [...selectedCandidateIds],
      });
      setPreview(response);
      setFeedback({
        kind: "success",
        message: "Review results are ready. Blocked items cannot be executed.",
      });
    } catch (error) {
      setFeedback({ kind: "error", message: getErrorMessage(error) });
    } finally {
      setIsBusy(false);
    }
  }

  async function requestApplicationInventory() {
    setIsBusy(true);
    setProgress(null);
    setApplicationInventory(null);
    setSelectedApplicationId(null);
    setUninstallResponse(null);
    setFeedback({
      kind: "info",
      message: "Requesting installed application inventory…",
    });

    try {
      const response = await discoverApplications({});
      setApplicationInventory(response);
      setFeedback({
        kind: "success",
        message: `Inventory returned ${response.applications.length} application(s).`,
      });
    } catch (error) {
      setFeedback({ kind: "error", message: getErrorMessage(error) });
    } finally {
      setIsBusy(false);
    }
  }

  function openCleanupConfirmation() {
    setConfirmationText("");
    setConfirmationAction({ kind: "cleanup", mode: cleanupMode });
  }

  function openUninstallConfirmation(application: InstalledApplication) {
    setConfirmationText("");
    setConfirmationAction({ kind: "uninstall", application });
  }

  async function confirmPendingAction() {
    if (!confirmationAction || isBusy) {
      return;
    }

    const action = confirmationAction;
    setIsBusy(true);
    setProgress(null);

    if (action.kind === "cleanup") {
      if (!preview) {
        setIsBusy(false);
        setConfirmationAction(null);
        return;
      }

      setFeedback({
        kind: "info",
        message:
          action.mode === "trash"
            ? "Sending eligible items to Trash…"
            : "Requesting permanent cleanup…",
      });

      try {
        const response = await executeCleanup({
          preview_id: preview.preview_id,
          confirmed: true,
          removal_mode: action.mode,
        });
        setCleanupResponse(response);
        setFeedback(
          response.failed_candidate_ids.length > 0
            ? {
                kind: "warning",
                message: `The operation reported ${response.removed_candidate_ids.length} completed item(s) and ${response.failed_candidate_ids.length} failure(s).`,
              }
            : {
                kind: "success",
                message: `The operation reported ${response.removed_candidate_ids.length} completed item(s).`,
              },
        );
      } catch (error) {
        setFeedback({ kind: "error", message: getErrorMessage(error) });
      } finally {
        setIsBusy(false);
        setConfirmationAction(null);
        setConfirmationText("");
      }

      return;
    }

    setFeedback({
      kind: "info",
      message: `Requesting uninstall for ${action.application.name}…`,
    });

    try {
      const response = await uninstallApplication({
        application_id: action.application.id,
        confirmed: true,
      });
      setUninstallResponse(response);
      setFeedback(
        response.status === "completed"
          ? {
              kind: "success",
              message: "The backend reports that uninstall completed.",
            }
          : {
              kind: "info",
              message: "The uninstall request was delegated to the system.",
            },
      );
    } catch (error) {
      setFeedback({ kind: "error", message: getErrorMessage(error) });
    } finally {
      setIsBusy(false);
      setConfirmationAction(null);
      setConfirmationText("");
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      setFeedback({ kind: "success", message: "Path copied to clipboard." });
    } catch {
      setFeedback({
        kind: "error",
        message: "The path could not be copied. Select the path text directly.",
      });
    }
  }

  const eligibleEntries =
    preview?.entries.filter((entry) => entry.state === "eligible") ?? [];
  const eligibleCount = eligibleEntries.length;
  const eligibleBytes = eligibleEntries.reduce(
    (total, entry) => total + entry.candidate.size_bytes,
    0,
  );
  const blockedCount =
    preview?.entries.filter((entry) => entry.state === "blocked").length ?? 0;
  const selectedApplication =
    applicationInventory?.applications.find(
      (application) => application.id === selectedApplicationId,
    ) ?? null;
  const candidateById = useMemo(() => {
    if (!cleanupResponse || !scan) {
      return new Map<string, RemovalCandidate>();
    }

    return new Map<string, RemovalCandidate>(
      scan.candidates.map((candidate) => [candidate.id, candidate] as const),
    );
  }, [cleanupResponse, scan]);
  const canConfirmPermanent =
    confirmationAction?.kind !== "cleanup" ||
    confirmationAction.mode !== "permanent" ||
    confirmationText === "PERMANENT";

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">
            SC
          </div>
          <div>
            <p className="brand-name">System CleanUp</p>
            <p className="brand-caption">Review first. Decide yourself.</p>
          </div>
        </div>

        <nav className="primary-nav" aria-label="Primary navigation">
          <button
            className={view === "cleanup" ? "nav-item active" : "nav-item"}
            type="button"
            aria-current={view === "cleanup" ? "page" : undefined}
            onClick={() => setView("cleanup")}
          >
            <span className="nav-icon" aria-hidden="true">
              ◫
            </span>
            File cleanup
          </button>
          <button
            className={
              view === "applications" ? "nav-item active" : "nav-item"
            }
            type="button"
            aria-current={view === "applications" ? "page" : undefined}
            onClick={() => setView("applications")}
          >
            <span className="nav-icon" aria-hidden="true">
              ▦
            </span>
            Applications
          </button>
        </nav>

        <div className="sidebar-footer">
          <span className="status-dot" aria-hidden="true" />
          Every action requires review
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <span className="eyebrow">SYSTEM MAINTENANCE</span>
          <span className="platform-label">Connected to native backend</span>
        </header>

        {view === "cleanup" ? (
          <section className="content-section" aria-labelledby="page-title">
            <div className="heading-row">
              <div>
                <p className="eyebrow">FILE REVIEW</p>
                <h1 id="page-title">Review files before cleanup.</h1>
                <p className="page-description">
                  Scan cache, temporary file, and diagnostic log categories.
                  Select candidates, inspect the preview, then confirm the
                  action.
                </p>
              </div>
              <div className="safe-badge">
                <span aria-hidden="true">✓</span>
                Nothing runs without confirmation
              </div>
            </div>

            <div className="section-label-row">
              <div>
                <h2>File candidates</h2>
                <p>
                  Candidates and warnings come from the platform adapter.
                </p>
              </div>
              <span className="count-pill">
                {scan ? `${scan.candidates.length} candidates` : "Not scanned"}
              </span>
            </div>

            {scan?.warnings.length ? (
              <WarningList title="Scan warnings" warnings={scan.warnings} />
            ) : null}

            {scan ? (
              scan.candidates.length > 0 ? (
                <>
                  <div className="selection-toolbar">
                    <span>
                      {selectedCandidateIds.size} selected across all pages
                    </span>
                    <div className="toolbar-actions">
                      <button
                        className="text-button"
                        type="button"
                        disabled={isBusy || pageCandidates.length === 0}
                        onClick={selectCurrentPage}
                      >
                        Select all on this page
                      </button>
                      <button
                        className="text-button"
                        type="button"
                        disabled={isBusy || selectedCandidateIds.size === 0}
                        onClick={() => updateCandidateSelection(new Set())}
                      >
                        Clear selection across all pages
                      </button>
                    </div>
                  </div>

                  <CandidatePagination
                    currentPage={candidatePage}
                    totalPages={candidatePageCount}
                    firstItem={pageStartIndex + 1}
                    lastItem={pageStartIndex + pageCandidates.length}
                    totalItems={candidateCount}
                    disabled={isBusy}
                    onPrevious={() =>
                      setCandidatePage((page) => Math.max(1, page - 1))
                    }
                    onNext={() =>
                      setCandidatePage((page) =>
                        Math.min(candidatePageCount, page + 1),
                      )
                    }
                  />

                  <div className="candidate-list">
                    {pageCandidates.map((candidate) => (
                      <CandidateCard
                        key={candidate.id}
                        candidate={candidate}
                        selected={selectedCandidateIds.has(candidate.id)}
                        disabled={isBusy}
                        onToggle={() => toggleCandidate(candidate.id)}
                        onCopyPath={() => void copyPath(candidate.path)}
                      />
                    ))}
                  </div>
                  <CandidatePagination
                    currentPage={candidatePage}
                    totalPages={candidatePageCount}
                    firstItem={pageStartIndex + 1}
                    lastItem={pageStartIndex + pageCandidates.length}
                    totalItems={candidateCount}
                    disabled={isBusy}
                    onPrevious={() =>
                      setCandidatePage((page) => Math.max(1, page - 1))
                    }
                    onNext={() =>
                      setCandidatePage((page) =>
                        Math.min(candidatePageCount, page + 1),
                      )
                    }
                  />
                </>
              ) : (
                <div className="empty-state" role="status">
                  <span className="empty-icon" aria-hidden="true">
                    ◫
                  </span>
                  <h3>No candidates returned</h3>
                  <p>
                    The connected adapter returned no file candidates for the
                    requested categories.
                  </p>
                </div>
              )
            ) : (
              <div className="empty-state" role="status">
                <span className="empty-icon" aria-hidden="true">
                  ◫
                </span>
                <h3>No scan results</h3>
                <p>
                  Run a scan to request file candidates from the connected
                  platform adapter.
                </p>
              </div>
            )}

            <div className="action-row">
              <button
                className="primary-button"
                type="button"
                disabled={isBusy}
                onClick={() => void requestScan()}
              >
                {isBusy ? "Working…" : scan ? "Scan again" : "Scan for candidates"}
                <span aria-hidden="true">→</span>
              </button>
              <span className="action-hint">
                Scanning only retrieves candidates; it changes no files.
              </span>
            </div>

            {scan && scan.candidates.length > 0 ? (
              <section className="workflow-panel" aria-labelledby="preview-title">
                <div className="section-label-row panel-heading">
                  <div>
                    <h2 id="preview-title">Review selection</h2>
                    <p>
                      The backend checks selected IDs before any cleanup can run.
                    </p>
                  </div>
                </div>
                <div className="mode-picker" role="group" aria-label="Cleanup mode">
                  <button
                    className={
                      cleanupMode === "trash" ? "mode-option selected" : "mode-option"
                    }
                    type="button"
                    aria-pressed={cleanupMode === "trash"}
                    disabled={isBusy || cleanupResponse !== null}
                    onClick={() => setCleanupMode("trash")}
                  >
                    <strong>Trash</strong>
                    <span>Default · recoverable</span>
                  </button>
                  <button
                    className={
                      cleanupMode === "permanent"
                        ? "mode-option selected"
                        : "mode-option"
                    }
                    type="button"
                    aria-pressed={cleanupMode === "permanent"}
                    disabled={isBusy || cleanupResponse !== null}
                    onClick={() => setCleanupMode("permanent")}
                  >
                    <strong>Permanent</strong>
                    <span>Irreversible cleanup</span>
                  </button>
                </div>
                <p className="mode-notice" role="note">
                  <strong>Trash mode:</strong> Items remain recoverable. Disk
                  space is not freed until you empty the Trash.
                </p>
                <div className="action-row review-actions">
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={isBusy || selectedCandidateIds.size === 0}
                    onClick={() => void requestPreview()}
                  >
                    {isBusy ? "Checking…" : "Prepare preview"}
                  </button>
                  {!preview ? (
                    <span className="action-hint">
                      Select candidates to prepare a backend review.
                    </span>
                  ) : null}
                </div>

                {preview ? (
                  <div className="preview-results">
                    <div className="preview-summary">
                      <span className="eligible-count">
                        {eligibleCount} eligible
                      </span>
                      <span className="blocked-count">{blockedCount} blocked</span>
                    </div>
                    {preview.entries.length > 0 ? (
                      <div className="preview-list">
                        {preview.entries.map((entry) => (
                          <PreviewEntryCard
                            key={entry.candidate.id}
                            entry={entry}
                            onCopyPath={() =>
                              void copyPath(entry.candidate.path)
                            }
                          />
                        ))}
                      </div>
                    ) : (
                      <p className="inline-note">
                        The backend returned no preview entries for the selected
                        IDs.
                      </p>
                    )}
                    <div className="action-row review-actions">
                      <button
                        className={
                          cleanupMode === "permanent"
                            ? "danger-button"
                            : "primary-button"
                        }
                        type="button"
                        disabled={
                          isBusy ||
                          eligibleCount === 0 ||
                          cleanupResponse !== null
                        }
                        onClick={openCleanupConfirmation}
                      >
                        {cleanupMode === "trash"
                          ? "Move eligible items to Trash"
                          : "Permanently clean eligible items"}
                      </button>
                      {cleanupResponse ? (
                        <span className="action-hint">
                          Scan again to start a new cleanup review.
                        </span>
                      ) : null}
                    </div>
                  </div>
                ) : null}
              </section>
            ) : null}

            {cleanupResponse ? (
              <CleanupOutcome
                response={cleanupResponse}
                candidateById={candidateById}
              />
            ) : null}

            <OperationStatus
              feedback={feedback}
              progress={progress}
              isBusy={isBusy}
            />
          </section>
        ) : (
          <section className="content-section" aria-labelledby="page-title">
            <div className="heading-row">
              <div>
                <p className="eyebrow">APPLICATION REVIEW</p>
                <h1 id="page-title">Choose an application to uninstall.</h1>
                <p className="page-description">
                  Review the installed application inventory, select one item,
                  then confirm its native uninstall request.
                </p>
              </div>
              <div className="safe-badge">
                <span aria-hidden="true">✓</span>
                No usage-based guesses
              </div>
            </div>

            <div className="section-label-row">
              <div>
                <h2>Installed applications</h2>
                <p>Name, version, and discovery source are returned by the backend.</p>
              </div>
              <span className="count-pill">
                {applicationInventory
                  ? `${applicationInventory.applications.length} applications`
                  : "Not loaded"}
              </span>
            </div>

            {applicationInventory?.warnings.length ? (
              <WarningList
                title="Inventory warnings"
                warnings={applicationInventory.warnings}
              />
            ) : null}

            {applicationInventory ? (
              applicationInventory.applications.length > 0 ? (
                <div className="application-list">
                  {applicationInventory.applications.map((application) => (
                    <ApplicationCard
                      key={application.id}
                      application={application}
                      selected={application.id === selectedApplicationId}
                      disabled={isBusy}
                      onSelect={() => {
                        setSelectedApplicationId(application.id);
                        setUninstallResponse(null);
                      }}
                    />
                  ))}
                </div>
              ) : (
                <div className="empty-state" role="status">
                  <span className="empty-icon" aria-hidden="true">
                    ▦
                  </span>
                  <h3>No applications returned</h3>
                  <p>
                    The connected adapter returned an empty installed application
                    inventory.
                  </p>
                </div>
              )
            ) : (
              <div className="empty-state" role="status">
                <span className="empty-icon" aria-hidden="true">
                  ▦
                </span>
                <h3>Application inventory not loaded</h3>
                <p>
                  Request an inventory from the connected platform adapter. If
                  the adapter is unsupported, its error will be shown here.
                </p>
              </div>
            )}

            <div className="action-row">
              <button
                className="primary-button"
                type="button"
                disabled={isBusy}
                onClick={() => void requestApplicationInventory()}
              >
                {isBusy
                  ? "Working…"
                  : applicationInventory
                    ? "Refresh inventory"
                    : "Load installed applications"}
                <span aria-hidden="true">→</span>
              </button>
              <button
                className="secondary-button"
                type="button"
                disabled={
                  isBusy || selectedApplication === null || uninstallResponse !== null
                }
                onClick={() =>
                  selectedApplication &&
                  openUninstallConfirmation(selectedApplication)
                }
              >
                Review uninstall
              </button>
            </div>

            {uninstallResponse ? (
              <UninstallOutcome
                response={uninstallResponse}
                application={
                  applicationInventory?.applications.find(
                    (application) =>
                      application.id === uninstallResponse.application_id,
                  ) ?? selectedApplication}
              />
            ) : null}

            <OperationStatus
              feedback={feedback}
              progress={progress}
              isBusy={isBusy}
            />
          </section>
        )}

        <footer className="main-footer">
          <span>Local review workflow · Explicit confirmation for actions</span>
          <span>Adapter support is reported by each operation</span>
        </footer>
      </main>

      {confirmationAction ? (
        <div className="confirm-backdrop">
          <section
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="confirm-title"
          >
            {confirmationAction.kind === "cleanup" ? (
              <>
                <p className="eyebrow">CLEANUP CONFIRMATION</p>
                <h2 id="confirm-title">
                  {confirmationAction.mode === "trash"
                    ? "Move eligible items to Trash?"
                    : "Permanently clean eligible items?"}
                </h2>
                <p>
                  {eligibleCount} eligible item(s), totaling {formatBytes(eligibleBytes)},
                  will be sent to the selected cleanup mode. Blocked items are
                  excluded.
                </p>
                {confirmationAction.mode === "permanent" ? (
                  <>
                    <p className="permanent-warning">
                      Permanent cleanup cannot be undone. Review the count and
                      total size above before continuing.
                    </p>
                    <label className="confirm-field">
                      <span>Type PERMANENT to confirm irreversible cleanup.</span>
                      <input
                        autoFocus
                        value={confirmationText}
                        onChange={(event) => setConfirmationText(event.target.value)}
                        autoComplete="off"
                        spellCheck={false}
                      />
                    </label>
                  </>
                ) : null}
              </>
            ) : (
              <>
                <p className="eyebrow">APPLICATION CONFIRMATION</p>
                <h2 id="confirm-title">
                  Request uninstall for {confirmationAction.application.name}?
                </h2>
                <p>
                  The platform adapter may complete the uninstall or delegate
                  the remaining steps to the operating system.
                </p>
                <dl className="confirm-meta">
                  <div>
                    <dt>Version</dt>
                    <dd>{confirmationAction.application.version ?? "Not provided"}</dd>
                  </div>
                  <div>
                    <dt>Source</dt>
                    <dd>
                      {applicationSourceLabels[
                        confirmationAction.application.source
                      ]}
                    </dd>
                  </div>
                </dl>
              </>
            )}
            <div className="confirm-actions">
              <button
                className="text-button"
                type="button"
                disabled={isBusy}
                onClick={() => {
                  setConfirmationAction(null);
                  setConfirmationText("");
                }}
              >
                Cancel
              </button>
              <button
                className={
                  confirmationAction.kind === "cleanup" &&
                  confirmationAction.mode === "permanent"
                    ? "danger-button"
                    : "primary-button"
                }
                type="button"
                disabled={isBusy || !canConfirmPermanent}
                onClick={() => void confirmPendingAction()}
              >
                {isBusy
                  ? "Working…"
                  : confirmationAction.kind === "cleanup"
                    ? confirmationAction.mode === "trash"
                      ? "Confirm move to Trash"
                      : "Confirm permanent cleanup"
                    : "Confirm uninstall request"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
}

type CandidateCardProps = {
  candidate: RemovalCandidate;
  selected: boolean;
  disabled: boolean;
  onToggle: () => void;
  onCopyPath: () => void;
};

type CandidatePaginationProps = {
  currentPage: number;
  totalPages: number;
  firstItem: number;
  lastItem: number;
  totalItems: number;
  disabled: boolean;
  onPrevious: () => void;
  onNext: () => void;
};

function CandidatePagination({
  currentPage,
  totalPages,
  firstItem,
  lastItem,
  totalItems,
  disabled,
  onPrevious,
  onNext,
}: CandidatePaginationProps) {
  return (
    <nav className="candidate-pagination" aria-label="Candidate pages">
      <button
        className="pagination-button"
        type="button"
        disabled={disabled || currentPage <= 1}
        onClick={onPrevious}
      >
        Previous
      </button>
      <span className="page-count" aria-live="polite">
        Page {currentPage} of {totalPages} · Showing {firstItem}–{lastItem} of{" "}
        {totalItems}
      </span>
      <button
        className="pagination-button"
        type="button"
        disabled={disabled || currentPage >= totalPages}
        onClick={onNext}
      >
        Next
      </button>
    </nav>
  );
}

function CandidateCard({
  candidate,
  selected,
  disabled,
  onToggle,
  onCopyPath,
}: CandidateCardProps) {
  return (
    <article className="candidate-card">
      <label className="candidate-select">
        <input
          type="checkbox"
          checked={selected}
          disabled={disabled}
          onChange={onToggle}
        />
        <span className="visually-hidden">
          Select {candidate.path} for cleanup
        </span>
      </label>
      <div className="candidate-content">
        <div className="candidate-heading">
          <div className="candidate-labels">
            <span className="category-pill">
              {categoryLabels[candidate.category]}
            </span>
            <span className={`risk-pill ${candidate.risk}`}>
              {getRiskLabel(candidate.risk)}
            </span>
          </div>
          <span className="candidate-size">{formatBytes(candidate.size_bytes)}</span>
        </div>
        <p className="candidate-reason">{candidate.reason}</p>
        <div className="path-row">
          <code className="candidate-path" title={candidate.path}>
            {candidate.path}
          </code>
          <button className="copy-button" type="button" onClick={onCopyPath}>
            Copy path
          </button>
        </div>
      </div>
    </article>
  );
}

type PreviewEntry = PreviewResponse["entries"][number];

type PreviewEntryCardProps = {
  entry: PreviewEntry;
  onCopyPath: () => void;
};

function PreviewEntryCard({ entry, onCopyPath }: PreviewEntryCardProps) {
  const isEligible = entry.state === "eligible";

  return (
    <article
      className={
        isEligible ? "preview-entry eligible-entry" : "preview-entry blocked-entry"
      }
    >
      <div className="preview-entry-heading">
        <strong>{isEligible ? "Eligible" : "Blocked"}</strong>
        <span>{formatBytes(entry.candidate.size_bytes)}</span>
      </div>
      <div className="candidate-labels">
        <span className="category-pill">
          {categoryLabels[entry.candidate.category]}
        </span>
        <span className={`risk-pill ${entry.candidate.risk}`}>
          {getRiskLabel(entry.candidate.risk)}
        </span>
      </div>
      <p className="candidate-reason">{entry.candidate.reason}</p>
      {entry.state === "blocked" ? (
        <p className="blocked-reason">{entry.reason}</p>
      ) : null}
      <div className="path-row">
        <code className="candidate-path" title={entry.candidate.path}>
          {entry.candidate.path}
        </code>
        <button className="copy-button" type="button" onClick={onCopyPath}>
          Copy path
        </button>
      </div>
    </article>
  );
}

type ApplicationCardProps = {
  application: InstalledApplication;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
};

function ApplicationCard({
  application,
  selected,
  disabled,
  onSelect,
}: ApplicationCardProps) {
  const uninstallAvailable = application.source !== "linux_desktop_entry";

  return (
    <label
      className={[
        "application-card",
        selected ? "selected" : "",
        uninstallAvailable ? "" : "unavailable",
      ]
        .filter(Boolean)
        .join(" ")}
      aria-disabled={!uninstallAvailable}
    >
      <input
        type="radio"
        name="selected-application"
        checked={selected}
        disabled={disabled || !uninstallAvailable}
        onChange={onSelect}
      />
      <span className="application-info">
        <strong>{application.name}</strong>
        <span className="application-meta">
          <span>Version: {application.version ?? "Not provided"}</span>
          <span>Source: {applicationSourceLabels[application.source]}</span>
        </span>
      </span>
      {!uninstallAvailable ? (
        <span className="uninstall-unavailable">Uninstall unavailable</span>
      ) : null}
    </label>
  );
}

type WarningListProps = {
  title: string;
  warnings: string[];
};

function WarningList({ title, warnings }: WarningListProps) {
  return (
    <section className="warning-panel" aria-label={title}>
      <h3>{title}</h3>
      <ul>
        {warnings.map((warning, index) => (
          <li key={`${warning}-${index}`}>{warning}</li>
        ))}
      </ul>
    </section>
  );
}

type CleanupOutcomeProps = {
  response: CleanupResponse;
  candidateById: Map<string, RemovalCandidate>;
};

function CleanupOutcome({ response, candidateById }: CleanupOutcomeProps) {
  return (
    <section className="outcome-panel" aria-labelledby="cleanup-outcome-title">
      <h2 id="cleanup-outcome-title">Cleanup response</h2>
      <p>
        The backend reported {response.removed_candidate_ids.length} completed
        item(s) and {response.failed_candidate_ids.length} failed item(s).
      </p>
      {response.removed_candidate_ids.length > 0 ? (
        <CandidateOutcomeList
          title="Completed items"
          candidateIds={response.removed_candidate_ids}
          candidateById={candidateById}
        />
      ) : null}
      {response.failed_candidate_ids.length > 0 ? (
        <CandidateOutcomeList
          title="Failed items"
          candidateIds={response.failed_candidate_ids}
          candidateById={candidateById}
        />
      ) : null}
    </section>
  );
}

type CandidateOutcomeListProps = {
  title: string;
  candidateIds: string[];
  candidateById: Map<string, RemovalCandidate>;
};

function CandidateOutcomeList({
  title,
  candidateIds,
  candidateById,
}: CandidateOutcomeListProps) {
  return (
    <div className="outcome-list">
      <h3>{title}</h3>
      <ul>
        {candidateIds.map((candidateId) => {
          const candidate = candidateById.get(candidateId);

          return (
            <li key={candidateId}>
              {candidate ? (
                <code>{candidate.path}</code>
              ) : (
                <code>{candidateId}</code>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

type UninstallOutcomeProps = {
  response: UninstallResponse;
  application: InstalledApplication | null;
};

function UninstallOutcome({ response, application }: UninstallOutcomeProps) {
  const isCompleted = response.status === "completed";

  return (
    <section
      className={
        isCompleted ? "outcome-panel success-outcome" : "outcome-panel"
      }
      aria-labelledby="uninstall-outcome-title"
    >
      <h2 id="uninstall-outcome-title">Uninstall response</h2>
      <p className="outcome-status">
        <strong>{isCompleted ? "Completed" : "Delegated to system"}</strong>
        {application ? ` · ${application.name}` : ` · ${response.application_id}`}
      </p>
      {!isCompleted ? (
        <p>
          The operating system owns the remaining uninstall steps. This response
          does not confirm that those steps have finished.
        </p>
      ) : null}
    </section>
  );
}

type OperationStatusProps = {
  feedback: Feedback | null;
  progress: OperationProgress | null;
  isBusy: boolean;
};

function OperationStatus({ feedback, progress, isBusy }: OperationStatusProps) {
  if (!feedback && !(isBusy && progress)) {
    return null;
  }

  return (
    <div
      className={`operation-status ${feedback?.kind ?? "info"}`}
      role={feedback?.kind === "error" ? "alert" : "status"}
      aria-live={feedback?.kind === "error" ? "assertive" : "polite"}
    >
      {feedback ? <span>{feedback.message}</span> : null}
      {isBusy && progress ? (
        <div className="progress-copy">
          <span>{progress.message}</span>
          {progress.total_units !== null && progress.total_units > 0 ? (
            <progress
              max={progress.total_units}
              value={Math.min(progress.completed_units, progress.total_units)}
              aria-label="Operation progress"
            />
          ) : (
            <span>{progress.completed_units} completed</span>
          )}
        </div>
      ) : null}
    </div>
  );
}

export default App;
