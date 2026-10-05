import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

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
import LanguageSelector from "./design-system/LanguageSelector";
import { Languages } from "./i18n/languages";
import { useLanguageStore } from "./stores/languageStore";

type View = "cleanup" | "applications";
type FeedbackKind = "info" | "success" | "warning" | "error";
type Feedback = {
  kind: FeedbackKind;
  key?: string;
  params?: Record<string, unknown>;
  raw?: string;
};
type ConfirmationAction =
  | { kind: "cleanup"; mode: RemovalMode }
  | { kind: "uninstall"; application: InstalledApplication };

const scanCategories: CandidateCategory[] = [
  "user_cache",
  "user_temporary",
  "diagnostic_log",
];
const candidatesPerPage = 100;

const permanentConfirmationWord = "PERMANENT";

function getErrorFeedback(error: unknown): Feedback {
  if (typeof error === "string") {
    return { kind: "error", raw: error };
  }

  if (error instanceof Error) {
    return { kind: "error", raw: error.message };
  }

  return { kind: "error", key: "feedback.genericError" };
}

function formatBytes(bytes: number, locale: string): string {
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

  const formatted = new Intl.NumberFormat(locale, {
    maximumFractionDigits: value >= 10 ? 0 : 1,
    minimumFractionDigits: value >= 10 ? 0 : 1,
  }).format(value);

  return `${formatted} ${units[unitIndex]}`;
}

function App() {
  const { t, i18n } = useTranslation();
  const language = useLanguageStore((state) => state.language);
  const setLanguage = useLanguageStore((state) => state.setLanguage);
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
    setFeedback({ kind: "info", key: "feedback.scanning" });

    try {
      const response = await scanCandidates({ categories: scanCategories });
      setScan(response);
      setFeedback({
        kind: "success",
        key: "feedback.scanDone",
        params: { count: response.candidates.length },
      });
    } catch (error) {
      setFeedback(getErrorFeedback(error));
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
    setFeedback({ kind: "info", key: "feedback.checking" });

    try {
      const response = await prepareCleanupPreview({
        scan_id: scan.scan_id,
        selected_candidate_ids: [...selectedCandidateIds],
      });
      setPreview(response);
      setFeedback({ kind: "success", key: "feedback.previewReady" });
    } catch (error) {
      setFeedback(getErrorFeedback(error));
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
    setFeedback({ kind: "info", key: "feedback.inventoryRequesting" });

    try {
      const response = await discoverApplications({});
      setApplicationInventory(response);
      setFeedback({
        kind: "success",
        key: "feedback.inventoryDone",
        params: { count: response.applications.length },
      });
    } catch (error) {
      setFeedback(getErrorFeedback(error));
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
        key:
          action.mode === "trash"
            ? "feedback.sendingToTrash"
            : "feedback.requestingPermanent",
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
                key: "feedback.cleanupPartial",
                params: {
                  removed: response.removed_candidate_ids.length,
                  failed: response.failed_candidate_ids.length,
                },
              }
            : {
                kind: "success",
                key: "feedback.cleanupDone",
                params: { removed: response.removed_candidate_ids.length },
              },
        );
      } catch (error) {
        setFeedback(getErrorFeedback(error));
      } finally {
        setIsBusy(false);
        setConfirmationAction(null);
        setConfirmationText("");
      }

      return;
    }

    setFeedback({
      kind: "info",
      key: "feedback.uninstallRequesting",
      params: { name: action.application.name },
    });

    try {
      const response = await uninstallApplication({
        application_id: action.application.id,
        confirmed: true,
      });
      setUninstallResponse(response);
      setFeedback(
        response.status === "completed"
          ? { kind: "success", key: "feedback.uninstallCompleted" }
          : { kind: "info", key: "feedback.uninstallDelegated" },
      );
    } catch (error) {
      setFeedback(getErrorFeedback(error));
    } finally {
      setIsBusy(false);
      setConfirmationAction(null);
      setConfirmationText("");
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      setFeedback({ kind: "success", key: "feedback.pathCopied" });
    } catch {
      setFeedback({ kind: "error", key: "feedback.pathCopyFailed" });
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
    confirmationText === permanentConfirmationWord;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">
            SC
          </div>
          <div>
            <p className="brand-name">System CleanUp</p>
            <p className="brand-caption">{t("brand.caption")}</p>
          </div>
        </div>

        <nav className="primary-nav" aria-label={t("nav.ariaLabel")}>
          <button
            className={view === "cleanup" ? "nav-item active" : "nav-item"}
            type="button"
            aria-current={view === "cleanup" ? "page" : undefined}
            onClick={() => setView("cleanup")}
          >
            <span className="nav-icon" aria-hidden="true">
              ◫
            </span>
            {t("nav.cleanup")}
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
            {t("nav.applications")}
          </button>
        </nav>

        <LanguageSelector
          label={t("language.label")}
          value={language}
          options={Languages.options.map((option) => ({
            value: option.code,
            label: option.name,
          }))}
          onChange={(value) => {
            if (Languages.isSupported(value)) {
              setLanguage(value);
            }
          }}
        />

        <div className="sidebar-footer">
          <span className="status-dot" aria-hidden="true" />
          {t("sidebar.footer")}
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <span className="eyebrow">{t("topbar.eyebrow")}</span>
          <span className="platform-label">{t("topbar.platform")}</span>
        </header>

        {view === "cleanup" ? (
          <section className="content-section" aria-labelledby="page-title">
            <div className="heading-row">
              <div>
                <p className="eyebrow">{t("cleanup.eyebrow")}</p>
                <h1 id="page-title">{t("cleanup.title")}</h1>
                <p className="page-description">{t("cleanup.description")}</p>
              </div>
              <div className="safe-badge">
                <span aria-hidden="true">✓</span>
                {t("cleanup.safeBadge")}
              </div>
            </div>

            <div className="section-label-row">
              <div>
                <h2>{t("cleanup.candidatesTitle")}</h2>
                <p>{t("cleanup.candidatesSubtitle")}</p>
              </div>
              <span className="count-pill">
                {scan
                  ? t("cleanup.candidateCount", { count: scan.candidates.length })
                  : t("cleanup.notScanned")}
              </span>
            </div>

            {scan?.warnings.length ? (
              <WarningList
                title={t("cleanup.warningsTitle")}
                warnings={scan.warnings}
              />
            ) : null}

            {scan ? (
              scan.candidates.length > 0 ? (
                <>
                  <div className="selection-toolbar">
                    <span>
                      {t("cleanup.selectedCount", {
                        count: selectedCandidateIds.size,
                      })}
                    </span>
                    <div className="toolbar-actions">
                      <button
                        className="text-button"
                        type="button"
                        disabled={isBusy || pageCandidates.length === 0}
                        onClick={selectCurrentPage}
                      >
                        {t("cleanup.selectPage")}
                      </button>
                      <button
                        className="text-button"
                        type="button"
                        disabled={isBusy || selectedCandidateIds.size === 0}
                        onClick={() => updateCandidateSelection(new Set())}
                      >
                        {t("cleanup.clearSelection")}
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
                  <h3>{t("cleanup.emptyNoCandidatesTitle")}</h3>
                  <p>{t("cleanup.emptyNoCandidatesBody")}</p>
                </div>
              )
            ) : (
              <div className="empty-state" role="status">
                <span className="empty-icon" aria-hidden="true">
                  ◫
                </span>
                <h3>{t("cleanup.emptyNoScanTitle")}</h3>
                <p>{t("cleanup.emptyNoScanBody")}</p>
              </div>
            )}

            <div className="action-row">
              <button
                className="primary-button"
                type="button"
                disabled={isBusy}
                onClick={() => void requestScan()}
              >
                {isBusy
                  ? t("common.working")
                  : scan
                    ? t("cleanup.scanAgain")
                    : t("cleanup.scan")}
                <span aria-hidden="true">→</span>
              </button>
              <span className="action-hint">{t("cleanup.scanHint")}</span>
            </div>

            {scan && scan.candidates.length > 0 ? (
              <section className="workflow-panel" aria-labelledby="preview-title">
                <div className="section-label-row panel-heading">
                  <div>
                    <h2 id="preview-title">{t("review.title")}</h2>
                    <p>{t("review.subtitle")}</p>
                  </div>
                </div>
                <div className="mode-picker" role="group" aria-label={t("review.modeAriaLabel")}>
                  <button
                    className={
                      cleanupMode === "trash" ? "mode-option selected" : "mode-option"
                    }
                    type="button"
                    aria-pressed={cleanupMode === "trash"}
                    disabled={isBusy || cleanupResponse !== null}
                    onClick={() => setCleanupMode("trash")}
                  >
                    <strong>{t("review.trashTitle")}</strong>
                    <span>{t("review.trashSubtitle")}</span>
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
                    <strong>{t("review.permanentTitle")}</strong>
                    <span>{t("review.permanentSubtitle")}</span>
                  </button>
                </div>
                <p className="mode-notice" role="note">
                  <strong>
                    {cleanupMode === "trash"
                      ? t("review.trashNoticeLabel")
                      : t("review.permanentNoticeLabel")}
                  </strong>{" "}
                  {cleanupMode === "trash"
                    ? t("review.trashNoticeBody")
                    : t("review.permanentNoticeBody")}
                </p>
                <div className="action-row review-actions">
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={isBusy || selectedCandidateIds.size === 0}
                    onClick={() => void requestPreview()}
                  >
                    {isBusy ? t("common.checking") : t("review.prepare")}
                  </button>
                  {!preview ? (
                    <span className="action-hint">{t("review.prepareHint")}</span>
                  ) : null}
                </div>

                {preview ? (
                  <div className="preview-results">
                    <div className="preview-summary">
                      <span className="eligible-count">
                        {t("review.eligibleCount", { count: eligibleCount })}
                      </span>
                      <span className="blocked-count">
                        {t("review.blockedCount", { count: blockedCount })}
                      </span>
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
                      <p className="inline-note">{t("review.noEntries")}</p>
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
                          ? t("review.moveToTrash")
                          : t("review.cleanPermanently")}
                      </button>
                      {cleanupResponse ? (
                        <span className="action-hint">
                          {t("review.scanAgainHint")}
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
                <p className="eyebrow">{t("applications.eyebrow")}</p>
                <h1 id="page-title">{t("applications.title")}</h1>
                <p className="page-description">
                  {t("applications.description")}
                </p>
              </div>
              <div className="safe-badge">
                <span aria-hidden="true">✓</span>
                {t("applications.safeBadge")}
              </div>
            </div>

            <div className="section-label-row">
              <div>
                <h2>{t("applications.listTitle")}</h2>
                <p>{t("applications.listSubtitle")}</p>
              </div>
              <span className="count-pill">
                {applicationInventory
                  ? t("applications.count", {
                      count: applicationInventory.applications.length,
                    })
                  : t("applications.notLoaded")}
              </span>
            </div>

            {applicationInventory?.warnings.length ? (
              <WarningList
                title={t("applications.warningsTitle")}
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
                  <h3>{t("applications.emptyNoneTitle")}</h3>
                  <p>{t("applications.emptyNoneBody")}</p>
                </div>
              )
            ) : (
              <div className="empty-state" role="status">
                <span className="empty-icon" aria-hidden="true">
                  ▦
                </span>
                <h3>{t("applications.emptyNotLoadedTitle")}</h3>
                <p>{t("applications.emptyNotLoadedBody")}</p>
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
                  ? t("common.working")
                  : applicationInventory
                    ? t("applications.refresh")
                    : t("applications.load")}
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
                {t("applications.reviewUninstall")}
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
          <span>{t("footer.workflow")}</span>
          <span>{t("footer.adapter")}</span>
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
                <p className="eyebrow">{t("confirm.cleanupEyebrow")}</p>
                <h2 id="confirm-title">
                  {confirmationAction.mode === "trash"
                    ? t("confirm.trashTitle")
                    : t("confirm.permanentTitle")}
                </h2>
                <p>
                  {t("confirm.cleanupBody", {
                    count: eligibleCount,
                    size: formatBytes(eligibleBytes, i18n.language),
                  })}
                </p>
                {confirmationAction.mode === "permanent" ? (
                  <>
                    <p className="permanent-warning">
                      {t("confirm.permanentWarning")}
                    </p>
                    <label className="confirm-field">
                      <span>
                        {t("confirm.typeToConfirm", {
                          word: permanentConfirmationWord,
                        })}
                      </span>
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
                <p className="eyebrow">{t("confirm.applicationEyebrow")}</p>
                <h2 id="confirm-title">
                  {t("confirm.uninstallTitle", {
                    name: confirmationAction.application.name,
                  })}
                </h2>
                <p>{t("confirm.uninstallBody")}</p>
                <dl className="confirm-meta">
                  <div>
                    <dt>{t("confirm.version")}</dt>
                    <dd>
                      {confirmationAction.application.version ??
                        t("common.notProvided")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("confirm.source")}</dt>
                    <dd>
                      {t(`source.${confirmationAction.application.source}`)}
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
                {t("common.cancel")}
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
                  ? t("common.working")
                  : confirmationAction.kind === "cleanup"
                    ? confirmationAction.mode === "trash"
                      ? t("confirm.confirmTrash")
                      : t("confirm.confirmPermanent")
                    : t("confirm.confirmUninstall")}
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
  const { t } = useTranslation();

  return (
    <nav className="candidate-pagination" aria-label={t("pagination.ariaLabel")}>
      <button
        className="pagination-button"
        type="button"
        disabled={disabled || currentPage <= 1}
        onClick={onPrevious}
      >
        {t("pagination.previous")}
      </button>
      <span className="page-count" aria-live="polite">
        {t("pagination.info", {
          current: currentPage,
          total: totalPages,
          first: firstItem,
          last: lastItem,
          totalItems,
        })}
      </span>
      <button
        className="pagination-button"
        type="button"
        disabled={disabled || currentPage >= totalPages}
        onClick={onNext}
      >
        {t("pagination.next")}
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
  const { t, i18n } = useTranslation();

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
          {t("candidate.selectForCleanup", { path: candidate.path })}
        </span>
      </label>
      <div className="candidate-content">
        <div className="candidate-heading">
          <div className="candidate-labels">
            <span className="category-pill">
              {t(`category.${candidate.category}`)}
            </span>
            <span className={`risk-pill ${candidate.risk}`}>
              {t(`risk.${candidate.risk}`)}
            </span>
          </div>
          <span className="candidate-size">{formatBytes(candidate.size_bytes, i18n.language)}</span>
        </div>
        <p className="candidate-reason">{candidate.reason}</p>
        <div className="path-row">
          <code className="candidate-path" title={candidate.path}>
            {candidate.path}
          </code>
          <button className="copy-button" type="button" onClick={onCopyPath}>
            {t("common.copyPath")}
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
  const { t, i18n } = useTranslation();
  const isEligible = entry.state === "eligible";

  return (
    <article
      className={
        isEligible ? "preview-entry eligible-entry" : "preview-entry blocked-entry"
      }
    >
      <div className="preview-entry-heading">
        <strong>{isEligible ? t("review.eligible") : t("review.blocked")}</strong>
        <span>{formatBytes(entry.candidate.size_bytes, i18n.language)}</span>
      </div>
      <div className="candidate-labels">
        <span className="category-pill">
          {t(`category.${entry.candidate.category}`)}
        </span>
        <span className={`risk-pill ${entry.candidate.risk}`}>
          {t(`risk.${entry.candidate.risk}`)}
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
          {t("common.copyPath")}
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
  const { t } = useTranslation();
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
          <span>
            {t("applications.version", {
              value: application.version ?? t("common.notProvided"),
            })}
          </span>
          <span>
            {t("applications.source", {
              value: t(`source.${application.source}`),
            })}
          </span>
        </span>
      </span>
      {!uninstallAvailable ? (
        <span className="uninstall-unavailable">
          {t("applications.uninstallUnavailable")}
        </span>
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
  const { t } = useTranslation();

  return (
    <section className="outcome-panel" aria-labelledby="cleanup-outcome-title">
      <h2 id="cleanup-outcome-title">{t("outcome.cleanupTitle")}</h2>
      <p>
        {t("outcome.cleanupBody", {
          removed: response.removed_candidate_ids.length,
          failed: response.failed_candidate_ids.length,
        })}
      </p>
      {response.removed_candidate_ids.length > 0 ? (
        <CandidateOutcomeList
          title={t("outcome.completedItems")}
          candidateIds={response.removed_candidate_ids}
          candidateById={candidateById}
        />
      ) : null}
      {response.failed_candidate_ids.length > 0 ? (
        <CandidateOutcomeList
          title={t("outcome.failedItems")}
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
  const { t } = useTranslation();
  const isCompleted = response.status === "completed";

  return (
    <section
      className={
        isCompleted ? "outcome-panel success-outcome" : "outcome-panel"
      }
      aria-labelledby="uninstall-outcome-title"
    >
      <h2 id="uninstall-outcome-title">{t("outcome.uninstallTitle")}</h2>
      <p className="outcome-status">
        <strong>
          {isCompleted ? t("outcome.completed") : t("outcome.delegated")}
        </strong>
        {application ? ` · ${application.name}` : ` · ${response.application_id}`}
      </p>
      {!isCompleted ? <p>{t("outcome.delegatedBody")}</p> : null}
    </section>
  );
}

type OperationStatusProps = {
  feedback: Feedback | null;
  progress: OperationProgress | null;
  isBusy: boolean;
};

function OperationStatus({ feedback, progress, isBusy }: OperationStatusProps) {
  const { t } = useTranslation();

  if (!feedback && !(isBusy && progress)) {
    return null;
  }

  return (
    <div
      className={`operation-status ${feedback?.kind ?? "info"}`}
      role={feedback?.kind === "error" ? "alert" : "status"}
      aria-live={feedback?.kind === "error" ? "assertive" : "polite"}
    >
      {feedback ? (
        <span>
          {feedback.raw ?? (feedback.key ? t(feedback.key, feedback.params) : "")}
        </span>
      ) : null}
      {isBusy && progress ? (
        <div className="progress-copy">
          <span>{progress.message}</span>
          {progress.total_units !== null && progress.total_units > 0 ? (
            <progress
              max={progress.total_units}
              value={Math.min(progress.completed_units, progress.total_units)}
              aria-label={t("common.operationProgress")}
            />
          ) : (
            <span>
              {t("common.progressCompleted", { count: progress.completed_units })}
            </span>
          )}
        </div>
      ) : null}
    </div>
  );
}

export default App;
