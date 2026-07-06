import {
  initialAuthSessionState,
  logoutSession,
  refreshSession,
  startOperatorSession,
  type AuthSessionState,
  type RuntimeSessionClient
} from "./authSession";
import {
  RuntimeApiClient,
  RuntimeApiError,
  RuntimeWebSocketClient,
  applyRunStatus,
  type CaptureDetailResponse,
  type CaptureFeaturesResponse,
  type CaptureJobResponse,
  type CaptureSummary,
  type CaptureStatus,
  type CaptureTriggerResponse,
  type DedupUpdate,
  type FrameCurrentResponse,
  type InputAckMessage,
  type InputRejectMessage,
  type LabelRole,
  type LabelsSnapshotResponse,
  type LabelUpdate,
  type PadButton,
  type RuntimeErrorCode,
  type RuntimeErrorDisplay,
  type RuntimeEventMessage,
  type RuntimeWsMessage,
  type ValidationStatusView,
  type ValidationUpdatedPayload
} from "./runtimeClient";
import {
  PAD_BUTTONS,
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  type BackendMode,
  type SessionState
} from "./runtimeContract";
import {
  buttonsFromStandardGamepad,
  keyboardButtonForCode,
  mergeInputButtons,
  samePadButtons,
  type NeutralizedDirection
} from "./inputUx";
import type { RuntimeConfig } from "./runtimeConfig";

type OperatorViewModel = {
  backendMode: BackendMode;
  sessionState: SessionState;
  currentFrame: number;
  lastAppliedInputFrame: number;
  lastPreviewFrame: number;
  activeCaptureJobId: string | null;
  previewState: "waiting" | "fresh" | "stale" | "unavailable";
  validationState: "idle" | "queued" | "passed" | "failed";
  validationStatus: ValidationStatusView;
  captureReview: CaptureReviewState;
  focusState: FocusState;
  recoveryNotices: RecoveryNotice[];
  captureJob: CaptureJobView | null;
  captureError: string | null;
  captureErrorCode: RuntimeErrorCode | null;
  sessionAction: SessionActionState;
  capturePending: boolean;
  controlsDisabled: boolean;
  pressedButtons: PadButton[];
  neutralizedDirections: NeutralizedDirection[];
  padlogTail: PadlogTailEntry[];
  config: RuntimeConfig;
  auth: AuthSessionState;
  preview: FrameCurrentResponse | null;
};

type OperatorRuntimeViewState = {
  validationState: OperatorViewModel["validationState"];
  validationStatus: ValidationStatusView;
  captureReview: CaptureReviewState;
  preview: FrameCurrentResponse | null;
  previewUnavailable: boolean;
  focusState: FocusState;
  recoveryEvents: RecoveryEvent[];
  captureJob: CaptureJobView | null;
  captureError: string | null;
  captureErrorCode: RuntimeErrorCode | null;
  sessionAction: SessionActionState;
  capturePending: boolean;
  pressedButtons: PadButton[];
  neutralizedDirections: NeutralizedDirection[];
  padlogTail: PadlogTailEntry[];
};

export type RuntimeEventClient = Pick<RuntimeWebSocketClient, "eventSocket"> &
  Partial<Pick<RuntimeWebSocketClient, "framesSocket">>;
export type RuntimeInputClient = Pick<RuntimeWebSocketClient, "inputSocket">;
export type RuntimePreviewClient = Pick<RuntimeApiClient, "currentFrame">;
type RuntimeHealthClient = Pick<RuntimeApiClient, "health">;
export type RuntimeRunClient = Pick<
  RuntimeApiClient,
  "runStatus" | "pauseRun" | "resumeRun" | "playRun" | "triggerCapture" | "captureJob"
>;
type RuntimeValidationClient = Pick<RuntimeApiClient, "validationStatus">;
type RuntimeCaptureReviewClient = Pick<
  RuntimeApiClient,
  "recentCaptures" | "captureDetail" | "captureFeatures" | "updateLabels" | "labelsSnapshot"
>;
type OperatorRuntimeClient = RuntimeSessionClient &
  Partial<
    RuntimeHealthClient &
      RuntimePreviewClient &
      RuntimeRunClient &
      RuntimeValidationClient &
      RuntimeCaptureReviewClient
  >;
type OperatorSocketClient = RuntimeEventClient & Partial<RuntimeInputClient>;

type FocusState = "focused" | "blurred" | "hidden";
type SessionActionState = "idle" | "pausing" | "resuming";
type RecoveryCode = RuntimeErrorCode | "bridge_unavailable" | "gamepad_disconnected" | "websocket_reconnect";
type RecoveryNotice = {
  code: RecoveryCode;
  title: string;
  message: string;
  severity: "info" | "warning" | "critical";
};
type RecoveryEvent = {
  code: RecoveryCode;
  message?: string;
};
type CaptureJobView = Pick<
  CaptureJobResponse,
  | "job_id"
  | "status"
  | "requested_frame"
  | "scheduled_frame"
  | "captured_frame"
  | "capture_id"
  | "labelable"
  | "has_preview"
  | "error"
>;
type CaptureReviewState = {
  captures: CaptureSummary[];
  selectedCaptureId: string | null;
  detail: CaptureDetailResponse | null;
  features: CaptureFeaturesResponse | null;
  featureError: string | null;
  labels: LabelsSnapshotResponse | null;
  loading: boolean;
  labelSaving: boolean;
  error: string | null;
  conflicts: RuntimeErrorDisplay[];
};
type DedupSelection =
  | { updates: DedupUpdate[]; error: null }
  | { updates: []; error: RuntimeErrorDisplay };
type PadlogTailEntry = {
  frame: number;
  padWord: number;
  status: "applied" | "queued" | "dropped";
  source: "keyboard" | "gamepad" | "combined";
  buttons: PadButton[];
};

const INITIAL_VALIDATION_STATUS: ValidationStatusView = {
  status: "not_run",
  command_class: null,
  started_at: null,
  completed_at: null,
  summary: "",
  issue_summaries: []
};

const EMPTY_CAPTURE_REVIEW: CaptureReviewState = {
  captures: [],
  selectedCaptureId: null,
  detail: null,
  features: null,
  featureError: null,
  labels: null,
  loading: false,
  labelSaving: false,
  error: null,
  conflicts: []
};

const INITIAL_VIEW_MODEL: Omit<OperatorViewModel, "config" | "auth"> = {
  backendMode: "synthetic",
  sessionState: "idle",
  currentFrame: 0,
  lastAppliedInputFrame: 0,
  lastPreviewFrame: 0,
  activeCaptureJobId: null,
  previewState: "waiting",
  validationState: "idle",
  validationStatus: INITIAL_VALIDATION_STATUS,
  captureReview: EMPTY_CAPTURE_REVIEW,
  focusState: "focused",
  recoveryNotices: [],
  captureJob: null,
  captureError: null,
  captureErrorCode: null,
  sessionAction: "idle",
  capturePending: false,
  controlsDisabled: true,
  pressedButtons: [],
  neutralizedDirections: [],
  padlogTail: [],
  preview: null
};

const EMPTY_RUNTIME_VIEW: OperatorRuntimeViewState = {
  validationState: INITIAL_VIEW_MODEL.validationState,
  validationStatus: INITIAL_VALIDATION_STATUS,
  captureReview: EMPTY_CAPTURE_REVIEW,
  preview: null,
  previewUnavailable: false,
  focusState: INITIAL_VIEW_MODEL.focusState,
  recoveryEvents: [],
  captureJob: null,
  captureError: null,
  captureErrorCode: null,
  sessionAction: "idle",
  capturePending: false,
  pressedButtons: [],
  neutralizedDirections: [],
  padlogTail: []
};

export function renderOperatorApp(
  config: RuntimeConfig,
  auth: AuthSessionState = initialAuthSessionState(),
  runtimeView: Partial<OperatorRuntimeViewState> = EMPTY_RUNTIME_VIEW
): string {
  const view = { ...EMPTY_RUNTIME_VIEW, ...runtimeView };
  const playing = auth.status === "active" && auth.session.state === "playing";
  const controlsDisabled =
    auth.status !== "active" ||
    (auth.session.state !== "running" && auth.session.state !== "playing") ||
    view.focusState === "hidden" ||
    inputBlockingError(auth.error?.code) ||
    // During continuous Play frames stream live, so a "stale" pulled preview
    // must not block input; only gate on staleness while stepping (running).
    (!playing && Boolean(auth.session.preview_stale || view.preview?.stale));
  const validationStatus = validationStatusForView(view.validationStatus, view.validationState);
  const model: OperatorViewModel = {
    ...INITIAL_VIEW_MODEL,
    backendMode: auth.session.backend_mode,
    sessionState: auth.session.state,
    currentFrame: auth.session.current_frame,
    lastAppliedInputFrame: auth.session.last_applied_input_frame,
    lastPreviewFrame: view.preview?.frame ?? auth.session.last_preview_frame,
    activeCaptureJobId: auth.session.active_capture_job_id,
    previewState: auth.session.active
      ? view.previewUnavailable
        ? "unavailable"
        : auth.session.preview_stale || view.preview?.stale
          ? "stale"
          : view.preview
            ? "fresh"
            : "waiting"
      : "waiting",
    validationState: validationStateToViewState(validationStatus.status),
    validationStatus,
    captureReview: captureReviewForView(view.captureReview),
    focusState: view.focusState,
    recoveryNotices: [],
    captureJob: view.captureJob,
    captureError: view.captureError,
    captureErrorCode: view.captureErrorCode,
    sessionAction: view.sessionAction,
    capturePending: view.capturePending,
    controlsDisabled,
    pressedButtons: view.pressedButtons,
    neutralizedDirections: view.neutralizedDirections,
    padlogTail: view.padlogTail,
    config,
    auth,
    preview: view.preview
  };
  model.recoveryNotices = recoveryNoticesForModel(model, view.recoveryEvents);

  return `
    <main class="shell" aria-label="ROM Operator Bridge">
      <section class="topbar" aria-labelledby="app-title">
        <div>
          <p class="eyebrow">Private Runtime</p>
          <h1 id="app-title">ROM Operator Bridge</h1>
        </div>
        <dl class="runtime-grid" aria-label="Runtime endpoints">
          ${runtimeStat("API", model.config.api_base_path)}
          ${runtimeStat("WebSocket", model.config.ws_base_path)}
          ${runtimeStat("Schema", String(model.config.schema_version))}
        </dl>
      </section>

      <section class="workspace" aria-label="Operator workspace">
        ${renderSessionPanel(model)}
        ${renderRecoveryPanel(model.recoveryNotices)}

        <article class="panel preview-panel">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Preview</p>
              <h2>${previewLabel(model.previewState)}</h2>
            </div>
            <span class="frame-counter">#${model.currentFrame}</span>
          </div>
          <div class="preview-surface" aria-label="Framebuffer preview">
            ${renderPreviewImage(model)}
          </div>
          <dl class="preview-meta" aria-label="Preview metadata">
            <div><dt>Guest frame</dt><dd>${model.currentFrame}</dd></div>
            <div><dt>Preview frame</dt><dd>${model.lastPreviewFrame}</dd></div>
            <div><dt>Focus</dt><dd>${focusLabel(model.focusState)}</dd></div>
          </dl>
        </article>

        <article class="panel status-panel">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Status</p>
              <h2>${stateLabel(model.sessionState)}</h2>
            </div>
            <span class="status-pill">${model.auth.session.active ? "active" : "locked"}</span>
          </div>
          <ul class="status-list">
            <li><span>Runtime API</span><strong>v${model.config.schema_version}</strong></li>
            <li><span>Applied input</span><strong>#${model.lastAppliedInputFrame}</strong></li>
            <li><span>Capture job</span><strong>${escapeHtml(captureStatusSummary(model))}</strong></li>
            <li><span>Validation</span><strong>${validationLabel(model.validationState)}</strong></li>
            <li><span>Verifier</span><strong>${escapeHtml(validationCommandLabel(model.validationStatus))}</strong></li>
            <li><span>Checked</span><strong>${escapeHtml(validationTimestampLabel(model.validationStatus))}</strong></li>
          </ul>
          ${renderValidationSummary(model.validationStatus)}
        </article>

        <article
          class="panel input-panel"
          aria-disabled="${model.controlsDisabled}"
          aria-label="Input surface"
          tabindex="0"
          data-input-focus-surface
        >
          <div class="panel-header">
            <div>
              <p class="eyebrow">Input</p>
              <h2>${PAD_LAYOUT_ID}</h2>
            </div>
            <span class="status-pill">v${PAD_LAYOUT_VERSION}</span>
          </div>
          <div class="pad-grid" aria-label="Pad layout">
            ${PAD_BUTTONS.map(
              (button) => `<button
                type="button"
                class="${model.pressedButtons.includes(button) ? "pressed" : ""}"
                data-pad-button="${button}"
                aria-pressed="${model.pressedButtons.includes(button)}"
                ${model.controlsDisabled ? "disabled" : ""}
              >${button}</button>`
            )
              .join("")}
          </div>
          <div class="pressed-summary" aria-label="Pressed buttons">
            ${renderPressedButtons(model.pressedButtons)}
          </div>
          ${renderNeutralizedDirections(model.neutralizedDirections)}
          ${renderPadlogTail(model.padlogTail)}
        </article>

        <article class="panel capture-panel" aria-busy="${model.capturePending}">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Capture</p>
              <h2>${capturePanelTitle(model)}</h2>
            </div>
            <button
              type="button"
              data-run-action="capture"
              ${captureButtonDisabled(model) ? "disabled" : ""}
            >${captureActionLabel(model)}</button>
          </div>
          ${
            model.captureError
              ? `<p class="session-alert" role="alert" tabindex="-1" data-capture-alert>${escapeHtml(safeBrowserMessage(model.captureError))}</p>`
              : ""
          }
          ${renderCaptureJob(model)}
        </article>

        ${renderCaptureReviewPanel(model)}
      </section>
    </main>
  `;
}

export function mountOperatorApp(
  root: HTMLElement,
  config: RuntimeConfig,
  client: OperatorRuntimeClient = new RuntimeApiClient(config),
  eventClient: OperatorSocketClient | null =
    typeof globalThis.WebSocket === "function" ? new RuntimeWebSocketClient(config) : null
): void {
  let auth = initialAuthSessionState();
  let authRequestSeq = 0;
  let previewRequestSeq = 0;
  let runStatusRequestSeq = 0;
  let validationRequestSeq = 0;
  let captureReviewRequestSeq = 0;
  let sessionAction: SessionActionState = "idle";
  let validationStatus = INITIAL_VALIDATION_STATUS;
  let captureReview = EMPTY_CAPTURE_REVIEW;
  let preview: FrameCurrentResponse | null = null;
  let previewUnavailable = false;
  let focusState: FocusState = currentFocusState();
  let recoveryEvents: RecoveryEvent[] = [];
  let captureJob: CaptureJobView | null = null;
  let captureError: string | null = null;
  let captureErrorCode: RuntimeErrorCode | null = null;
  let capturePending = false;
  let captureRequestSeq = 0;
  let pressedButtons: PadButton[] = [];
  let manualButtons: PadButton[] = [];
  let keyboardButtons: PadButton[] = [];
  let keyboardPadButtons: PadButton[] = [];
  let gamepadButtons: PadButton[] = [];
  let neutralizedDirections: NeutralizedDirection[] = [];
  let padlogTail: PadlogTailEntry[] = [];
  let serviceBackendMode: BackendMode = auth.session.backend_mode;
  let eventSocket: ReturnType<RuntimeEventClient["eventSocket"]> | null = null;
  let eventSessionId: string | null = null;
  let inputSocket: ReturnType<RuntimeInputClient["inputSocket"]> | null = null;
  let inputSessionId: string | null = null;
  let framesSocket: ReturnType<NonNullable<RuntimeEventClient["framesSocket"]>> | null = null;
  let framesSessionId: string | null = null;
  // Live-play frame delivery. Frames stream over the frames socket as
  // [u64 LE frame_counter][PNG]; the newest bitmap is drawn onto a <canvas> via
  // createImageBitmap (no object/data URLs — those are blocked by the privacy
  // boundary since they could persist framebuffer pixels). `lastDisplayedFrame`
  // is the ordering key (deterministic pv-pad frame_counter): a frame renders
  // only if strictly newer. frame_counter restarts at 0 each run, so it resets
  // on run_id change. `liveBitmap` is retained so the canvas can be repainted
  // after a re-render without waiting for the next frame.
  let liveBitmap: ImageBitmap | null = null;
  let lastDisplayedFrame = -1;
  let liveFrameRunId: string | null = null;
  let gamepadPollCancel: (() => void) | null = null;
  root.innerHTML = '<div data-operator-app></div><p class="session-live" aria-live="polite"></p>';
  const appRegion = root.querySelector<HTMLElement>("[data-operator-app]");
  const liveRegion = root.querySelector<HTMLElement>(".session-live");

  const render = (focusTarget?: "alert" | "start" | "logout") => {
    if (!appRegion || !liveRegion) {
      return;
    }
    syncInputCaptureLifecycle();
    const shouldRestoreInputFocus = !focusTarget && inputSurfaceFocused();
    const focusedPadButton = shouldRestoreInputFocus
      ? padButtonFromElement(globalThis.document?.activeElement)
      : null;
    appRegion.innerHTML = renderOperatorApp(config, auth, {
      validationState: validationStateToViewState(validationStatus.status),
      validationStatus,
      captureReview,
      preview,
      previewUnavailable,
      focusState,
      recoveryEvents,
      captureJob,
      captureError,
      captureErrorCode,
      sessionAction,
      capturePending,
      pressedButtons,
      neutralizedDirections,
      padlogTail
    });
    liveRegion.textContent = sessionStatusLabel(auth);
    // The re-render replaced the preview canvas; repaint the retained live frame
    // so continuous Play does not flicker to black between frames.
    paintLiveFrame();
    if (focusTarget) {
      focusSessionTarget(appRegion, focusTarget);
    } else if (shouldRestoreInputFocus) {
      focusInputSurface(appRegion, focusedPadButton);
    }
  };

  const applyServiceBackendMode = (backendMode: BackendMode) => {
    serviceBackendMode = backendMode;
    if (!auth.session.active && auth.session.backend_mode !== backendMode) {
      auth = {
        ...auth,
        session: {
          ...auth.session,
          backend_mode: backendMode
        }
      };
      render();
    }
  };

  const refreshServiceBackendMode = (): Promise<BackendMode> => {
    const health = client.health?.bind(client);
    if (!health) {
      return Promise.resolve(serviceBackendMode);
    }
    return health()
      .then((response) => {
        applyServiceBackendMode(response.backend_mode);
        return response.backend_mode;
      })
      .catch(() => serviceBackendMode);
  };

  const applyAuthResult = (requestSeq: number, next: AuthSessionState) => {
    if (requestSeq !== authRequestSeq) {
      return;
    }
    const previousSessionId = auth.session.session_id;
    const sessionWillReset = !next.session.active || next.session.session_id !== previousSessionId;
    if (sessionWillReset) {
      closeInputStream(true);
    }
    if (next.session.active) {
      serviceBackendMode = next.session.backend_mode;
      auth = next;
    } else {
      auth = {
        ...next,
        session: {
          ...next.session,
          backend_mode: serviceBackendMode
        }
      };
    }
    if (sessionWillReset) {
      preview = null;
      previewUnavailable = false;
      previewRequestSeq += 1;
      validationRequestSeq += 1;
      captureReviewRequestSeq += 1;
      captureJob = null;
      captureError = null;
      captureErrorCode = null;
      capturePending = false;
      captureRequestSeq += 1;
      recoveryEvents = [];
      pressedButtons = [];
      manualButtons = [];
      keyboardButtons = [];
      keyboardPadButtons = [];
      gamepadButtons = [];
      neutralizedDirections = [];
      padlogTail = [];
      validationStatus = INITIAL_VALIDATION_STATUS;
      captureReview = EMPTY_CAPTURE_REVIEW;
    }
    sessionAction = "idle";
    syncEventStream();
    syncInputStream();
    syncFramesStream();
    render(focusTargetForAuth(auth));
    refreshRunStatus();
    refreshValidationStatus();
    refreshCaptureReview();
    refreshPreview();
  };

  function closeEventStream() {
    eventSocket?.close();
    eventSocket = null;
    eventSessionId = null;
  }

  function closeInputStream(sendRelease = false) {
    clearInputSources(sendRelease);
    stopGamepadPolling();
    inputSocket?.close();
    inputSocket = null;
    inputSessionId = null;
  }

  function syncEventStream() {
    if (!eventClient) {
      return;
    }
    const nextSessionId =
      (auth.status === "active" || auth.status === "stopping") && auth.session.session_id
        ? auth.session.session_id
        : null;
    if (!nextSessionId) {
      closeEventStream();
      return;
    }
    if (eventSocket && eventSessionId === nextSessionId) {
      return;
    }

    closeEventStream();
    eventSessionId = nextSessionId;
    eventSocket = eventClient.eventSocket({
      onMessage: handleRuntimeEvent,
      onError: (error) => {
        recordRecoveryEvent({ code: error.display.code, message: error.display.message });
        render();
      },
      onReconnect: () => {
        recordRecoveryEvent({ code: "websocket_reconnect" });
        refreshRunStatus();
        refreshValidationStatus();
        refreshCaptureReview();
        refreshPreview();
        render();
      }
    });
  }

  function closeFramesStream() {
    framesSocket?.close();
    framesSocket = null;
    framesSessionId = null;
    liveBitmap?.close();
    liveBitmap = null;
    lastDisplayedFrame = -1;
    liveFrameRunId = null;
  }

  // Paint the retained live frame onto the current preview canvas. Called both
  // when a new frame decodes and after a re-render (which replaces the canvas).
  function paintLiveFrame() {
    if (!liveBitmap) {
      return;
    }
    const canvas = appRegion?.querySelector<HTMLCanvasElement>("[data-preview-canvas]");
    if (!canvas) {
      return;
    }
    if (canvas.width !== liveBitmap.width) {
      canvas.width = liveBitmap.width;
    }
    if (canvas.height !== liveBitmap.height) {
      canvas.height = liveBitmap.height;
    }
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.drawImage(liveBitmap, 0, 0);
    }
  }

  async function handleLiveFrame(buffer: ArrayBuffer) {
    if (buffer.byteLength < 8) {
      return;
    }
    const view = new DataView(buffer);
    const frameCounter = Number(view.getBigUint64(0, true));
    const runId = auth.status === "active" ? (auth.session.run_id ?? null) : null;
    if (runId !== liveFrameRunId) {
      liveFrameRunId = runId;
      lastDisplayedFrame = -1;
    }
    // Newest-by-frame_counter: drop any older/reordered frame.
    if (frameCounter <= lastDisplayedFrame) {
      return;
    }
    // Mark received-newest up front (so a concurrent older decode is dropped
    // post-await) and remember which run this frame belongs to.
    lastDisplayedFrame = frameCounter;
    const decodeRunId = liveFrameRunId;
    let bitmap: ImageBitmap;
    try {
      bitmap = await createImageBitmap(new Blob([buffer.slice(8)], { type: "image/png" }));
    } catch {
      return;
    }
    // Discard if a newer frame won the race (don't paint backwards) OR the run
    // changed while decoding — a Stop->Start could otherwise paint a previous
    // run's frame onto the new run's canvas.
    if (frameCounter < lastDisplayedFrame || decodeRunId !== liveFrameRunId) {
      bitmap.close();
      return;
    }
    liveBitmap?.close();
    liveBitmap = bitmap;
    paintLiveFrame();
  }

  function syncFramesStream() {
    if (!eventClient?.framesSocket) {
      return;
    }
    const nextSessionId =
      auth.status === "active" && auth.session.session_id ? auth.session.session_id : null;
    if (!nextSessionId) {
      closeFramesStream();
      return;
    }
    if (framesSocket && framesSessionId === nextSessionId) {
      return;
    }
    closeFramesStream();
    framesSessionId = nextSessionId;
    framesSocket = eventClient.framesSocket({ onBinary: handleLiveFrame });
  }

  function syncInputStream() {
    if (!eventClient?.inputSocket) {
      return;
    }
    const nextSessionId =
      auth.status === "active" && auth.session.session_id ? auth.session.session_id : null;
    if (!nextSessionId) {
      closeInputStream(true);
      return;
    }
    if (inputSocket && inputSessionId === nextSessionId) {
      return;
    }

    closeInputStream(true);
    inputSessionId = nextSessionId;
    inputSocket = eventClient.inputSocket(nextSessionId, "combined", {
      onMessage: handleRuntimeEvent,
      onError: () => undefined,
      onClose: clearDisconnectedInputState,
      onReconnect: () => {
        recordRecoveryEvent({ code: "websocket_reconnect" });
        clearDisconnectedInputState();
        syncInputCaptureLifecycle();
        refreshRunStatus();
        refreshCaptureReview();
        refreshPreview();
        render();
      }
    });
    startGamepadPolling();
  }

  function handleRuntimeEvent(message: RuntimeWsMessage) {
    if (message.session_id !== auth.session.session_id) {
      return;
    }

    if (message.type === "input_ack") {
      applyInputAck(message);
      render();
      return;
    }

    if (message.type === "input_reject") {
      applyInputReject(message);
      render();
      return;
    }

    if (!isRuntimeEvent(message)) {
      return;
    }

    if (runtimeEventStopsSession(message)) {
      closeInputStream(true);
    }

    if (message.type === "validation_updated") {
      validationStatus = validationStatusFromPayload(message.payload);
      render();
      return;
    }

    if (message.type === "session_updated") {
      auth = {
        ...auth,
        status: message.payload.state === "stopped" ? "locked" : auth.status,
        session: {
          ...auth.session,
          active: message.payload.state !== "stopped",
          state: message.payload.state,
          backend_mode: message.payload.backend_mode,
          current_frame: message.payload.current_frame,
          capabilities: message.payload.capabilities
        },
        error: null
      };
    }

    if (message.type === "run_updated") {
      auth = {
        ...auth,
        status: message.payload.state === "stopped" ? "locked" : auth.status,
        session: {
          ...auth.session,
          active: message.payload.state !== "stopped",
          state: message.payload.state,
          current_frame: message.payload.current_frame,
          preview_stale: message.payload.preview_stale,
          active_capture_job_id: message.payload.active_capture_job_id
        },
        error: null
      };
      syncCaptureFromActiveJob(message.payload.active_capture_job_id);
    }

    if (message.type === "capture_updated") {
      auth = {
        ...auth,
        session: {
          ...auth.session,
          active_capture_job_id:
            message.payload.status === "requested" || message.payload.status === "capturing"
              ? message.payload.job_id
              : null
        }
      };
      captureJob = mergeCaptureUpdate(captureJob, message.payload);
      capturePending =
        message.payload.status === "requested" || message.payload.status === "capturing";
      captureError = null;
      captureErrorCode = null;
      void refreshCaptureJob(message.payload.job_id);
      if (!isActiveCaptureStatus(message.payload.status)) {
        void refreshCaptureReview(message.payload.capture_id ?? captureReview.selectedCaptureId);
      }
    }

    if (message.type === "label_updated") {
      captureReview = {
        ...captureReview,
        conflicts: [],
        error: null
      };
      void refreshCaptureReview();
    }

    if (!auth.session.active) {
      closeEventStream();
      closeInputStream(true);
      auth = initialAuthSessionState();
      validationStatus = INITIAL_VALIDATION_STATUS;
      validationRequestSeq += 1;
      captureReview = EMPTY_CAPTURE_REVIEW;
      captureReviewRequestSeq += 1;
      preview = null;
      previewUnavailable = false;
      previewRequestSeq += 1;
      sessionAction = "idle";
      captureJob = null;
      captureError = null;
      captureErrorCode = null;
      capturePending = false;
      captureRequestSeq += 1;
      recoveryEvents = [];
      clearInputSources(false);
      padlogTail = [];
      render("start");
      return;
    }

    render();
    if (message.type === "session_updated" || message.type === "run_updated") {
      syncInputStream();
      // During continuous Play the newest frame already arrives over /ws/frames;
      // skip the per-frame HTTP pull (which fires once per Play frame and would
      // defeat the live stream). Pull only when paused/single-step/idle.
      if (!(auth.status === "active" && auth.session.state === "playing")) {
        refreshPreview();
      }
    }
  }

  function applyInputAck(message: InputAckMessage) {
    const nextInputState = applyInputAckToState(message, padlogTail);
    pressedButtons = nextInputState.pressedButtons;
    padlogTail = nextInputState.padlogTail;
    auth = {
      ...auth,
      session: {
        ...auth.session,
        last_applied_input_frame: Math.max(
          auth.session.last_applied_input_frame,
          message.payload.assigned_frame
        )
      },
      error: null
    };
  }

  function applyInputReject(message: InputRejectMessage) {
    clearInputSources(false);
    padlogTail = applyInputRejectToState(message, padlogTail);
    auth = { ...auth, error: message.payload.error };
    if (message.payload.error.code === "session_inactive") {
      closeInputStream(false);
    }
  }

  function refreshRunStatus() {
    const runStatus = client.runStatus?.bind(client);
    if (!runStatus || auth.status !== "active" || !auth.session.session_id) {
      return;
    }
    const requestSeq = ++runStatusRequestSeq;
    const sessionId = auth.session.session_id;
    runStatus()
      .then((status) => {
        if (
          requestSeq !== runStatusRequestSeq ||
          auth.status !== "active" ||
          auth.session.session_id !== sessionId ||
          status.session_id !== sessionId
        ) {
          return;
        }
        auth = { ...auth, session: applyRunStatus(auth.session, status), error: null };
        clearRecoveryEvents("backend_unavailable", "websocket_reconnect");
        syncCaptureFromActiveJob(status.active_capture_job_id);
        render();
      })
      .catch((error) => {
        if (requestSeq !== runStatusRequestSeq || auth.status !== "active" || auth.session.session_id !== sessionId) {
          return;
        }
        auth = { ...auth, error: runtimeDisplayError(error, "backend_unavailable") };
        render("alert");
      });
  }

  function refreshValidationStatus() {
    const validationStatusRequest = client.validationStatus?.bind(client);
    if (!validationStatusRequest || auth.status !== "active" || !auth.session.session_id) {
      return;
    }
    const requestSeq = ++validationRequestSeq;
    const sessionId = auth.session.session_id;
    validationStatusRequest()
      .then((status) => {
        if (
          requestSeq !== validationRequestSeq ||
          auth.status !== "active" ||
          auth.session.session_id !== sessionId
        ) {
          return;
        }
        validationStatus = validationStatusFromPayload(status);
        clearRecoveryEvents("backend_unavailable", "websocket_reconnect");
        render();
      })
      .catch((error) => {
        if (
          requestSeq !== validationRequestSeq ||
          auth.status !== "active" ||
          auth.session.session_id !== sessionId
        ) {
          return;
        }
        const display = runtimeDisplayError(error, "backend_unavailable");
        recordRecoveryEvent({
          code: display.code,
          message: display.message
        });
        render();
      });
  }

  async function refreshCaptureReview(preferredCaptureId = captureReview.selectedCaptureId) {
    const recentCaptures = client.recentCaptures?.bind(client);
    const captureDetail = client.captureDetail?.bind(client);
    const captureFeatures = client.captureFeatures?.bind(client);
    const labelsSnapshot = client.labelsSnapshot?.bind(client);
    if (
      !recentCaptures ||
      !captureDetail ||
      !labelsSnapshot ||
      auth.status !== "active" ||
      !auth.session.session_id
    ) {
      return;
    }

    const requestSeq = ++captureReviewRequestSeq;
    const sessionId = auth.session.session_id;
    captureReview = { ...captureReview, loading: true, error: null, featureError: null };
    render();
    try {
      const [recent, labels] = await Promise.all([
        recentCaptures({ limit: 8 }),
        labelsSnapshot()
      ]);
      if (
        requestSeq !== captureReviewRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      const selectedCaptureId = selectCaptureIdForReview(
        recent.captures,
        preferredCaptureId ?? captureReview.selectedCaptureId
      );
      const detail = selectedCaptureId ? await captureDetail(selectedCaptureId) : null;
      if (
        requestSeq !== captureReviewRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      let features: CaptureFeaturesResponse | null = null;
      let featureError: string | null = null;
      if (detail?.privileged_features_available) {
        if (!auth.session.capabilities?.privileged_features) {
          features = null;
        } else if (!captureFeatures) {
          featureError = "Privileged feature map unavailable.";
        } else {
          try {
            features = await captureFeatures(detail.capture_id);
            if (features.capture_id !== detail.capture_id) {
              featureError = "Privileged feature map unavailable.";
              features = null;
            }
          } catch (error) {
            featureError = runtimeDisplayError(error, "backend_unavailable").message;
          }
        }
      }
      if (
        requestSeq !== captureReviewRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      captureReview = captureReviewForView({
        captures: recent.captures,
        selectedCaptureId,
        detail,
        features,
        featureError,
        labels,
        loading: false,
        labelSaving: false,
        error: null,
        conflicts: captureReview.conflicts
      });
      clearRecoveryEvents("backend_unavailable");
      render();
    } catch (error) {
      if (
        requestSeq !== captureReviewRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      const display = runtimeDisplayError(error, "backend_unavailable");
      captureReview = {
        ...captureReview,
        loading: false,
        labelSaving: false,
        error: display.message,
        conflicts: []
      };
      recordRecoveryEvent({ code: display.code, message: display.message });
      render();
    }
  }

  function syncCaptureFromActiveJob(jobId: string | null) {
    if (!jobId) {
      if (captureJob && isActiveCaptureStatus(captureJob.status)) {
        captureJob = null;
      }
      if (captureErrorCode === "capture_in_progress") {
        captureError = null;
        captureErrorCode = null;
      }
      capturePending = false;
      return;
    }

    capturePending = true;
    captureError = null;
    captureErrorCode = null;
    if (captureJob?.job_id !== jobId) {
      captureJob = mergeCaptureUpdate(null, {
        job_id: jobId,
        status: "capturing",
        capture_id: null
      });
    }
    void refreshCaptureJob(jobId);
  }

  function refreshPreview() {
    const currentFrame = client.currentFrame?.bind(client);
    if (!currentFrame || auth.status !== "active" || !auth.session.session_id) {
      return;
    }
    const requestSeq = ++previewRequestSeq;
    const sessionId = auth.session.session_id;
    currentFrame()
      .then((nextPreview) => {
        if (
          requestSeq !== previewRequestSeq ||
          auth.status !== "active" ||
          auth.session.session_id !== sessionId
        ) {
          return;
        }
        preview = nextPreview;
        previewUnavailable = false;
        auth = {
          ...auth,
          error: auth.error?.code === "backend_unavailable" ? null : auth.error,
          session: {
            ...auth.session,
            last_preview_frame: nextPreview.frame,
            preview_stale: nextPreview.stale
          }
        };
        clearRecoveryEvents("backend_unavailable");
        render();
      })
      .catch((error) => {
        if (requestSeq !== previewRequestSeq) {
          return;
        }
        preview = null;
        if (error instanceof RuntimeApiError && error.display.code === "frame_unavailable") {
          // The backend has no frame to show yet (e.g. the guest has not
          // produced one); the session itself is healthy, so keep the
          // preview panel calm instead of raising a recovery alert.
          previewUnavailable = true;
          render();
          return;
        }
        auth = { ...auth, error: runtimeDisplayError(error, "backend_unavailable") };
        render();
      });
  }

  function refreshCaptureJob(jobId: string): Promise<void> {
    const captureJobRequest = client.captureJob?.bind(client);
    if (!captureJobRequest || auth.status !== "active") {
      return Promise.resolve();
    }
    const requestSeq = captureRequestSeq;
    return captureJobRequest(jobId)
      .then((nextJob) => {
        if (requestSeq !== captureRequestSeq || auth.status !== "active") {
          return;
        }
        captureJob = toCaptureJobView(nextJob);
        capturePending = isActiveCaptureStatus(nextJob.status);
        captureError = nextJob.error?.message ? safeBrowserMessage(nextJob.error.message) : null;
        captureErrorCode = nextJob.error?.code ?? null;
        auth = {
          ...auth,
          session: {
            ...auth.session,
            active_capture_job_id: isActiveCaptureStatus(nextJob.status) ? nextJob.job_id : null
          }
        };
        if (!isActiveCaptureStatus(nextJob.status)) {
          void refreshCaptureReview(nextJob.capture_id ?? captureReview.selectedCaptureId);
        }
        render();
      })
      .catch((error) => {
        if (requestSeq !== captureRequestSeq) {
          return;
        }
        capturePending = false;
        const displayError = runtimeDisplayError(error, "capture_failed");
        captureError = displayError.message;
        captureErrorCode = displayError.code;
        render();
      });
  }

  root.addEventListener("submit", (event) => {
    const form = event.target instanceof HTMLFormElement ? event.target : null;
    if (!form || form.dataset.sessionForm !== "start") {
      return;
    }
    event.preventDefault();
    if (auth.status === "starting" || auth.status === "stopping") {
      return;
    }

    const requestSeq = ++authRequestSeq;
    auth = { ...auth, status: "starting", error: null };
    validationStatus = INITIAL_VALIDATION_STATUS;
    validationRequestSeq += 1;
    captureReview = EMPTY_CAPTURE_REVIEW;
    captureReviewRequestSeq += 1;
    recoveryEvents = [];
    preview = null;
    previewUnavailable = false;
    previewRequestSeq += 1;
    render();
    refreshServiceBackendMode()
      .then((backendMode) => startOperatorSession(auth, client, backendMode))
      .then((next) => {
        applyAuthResult(requestSeq, next);
      });
  });

  root.addEventListener("submit", (event) => {
    const form = event.target instanceof HTMLFormElement ? event.target : null;
    if (!form || form.dataset.labelDrawerForm !== "capture") {
      return;
    }
    event.preventDefault();
    void submitCaptureLabels(form);
  });

  root.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const logoutButton = target?.closest<HTMLButtonElement>("[data-session-action='logout']");
    if (!logoutButton || logoutButton.disabled || auth.status !== "active") {
      return;
    }

    const stateToStop = auth;
    const requestSeq = ++authRequestSeq;
    closeInputStream(true);
    auth = { ...auth, status: "stopping", error: null };
    sessionAction = "idle";
    preview = null;
    previewUnavailable = false;
    previewRequestSeq += 1;
    syncEventStream();
    render();
    logoutSession(stateToStop, client).then((next) => {
      applyAuthResult(requestSeq, next);
    });
  });

  root.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const reviewButton = target?.closest<HTMLButtonElement>("[data-capture-review-action]");
    if (!reviewButton || reviewButton.disabled) {
      return;
    }
    const action = reviewButton.dataset.captureReviewAction;
    if (action === "refresh") {
      void refreshCaptureReview();
      return;
    }
    if (action === "select") {
      const captureId = reviewButton.dataset.captureId;
      if (captureId) {
        captureReview = {
          ...captureReview,
          selectedCaptureId: captureId,
          detail: null,
          features: null,
          featureError: null,
          conflicts: [],
          error: null
        };
        render();
        void refreshCaptureReview(captureId);
      }
    }
  });

  root.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const action = target?.closest<HTMLButtonElement>("[data-run-action]")?.dataset.runAction;
    if (!action || auth.status !== "active" || !auth.session.session_id) {
      return;
    }

    if (action === "play") {
      void transitionRun("resuming", client.playRun?.bind(client));
      return;
    }
    if (action === "pause") {
      void transitionRun("pausing", client.pauseRun?.bind(client));
      return;
    }
    if (action === "resume") {
      void transitionRun("resuming", client.resumeRun?.bind(client));
      return;
    }
    if (action === "capture") {
      captureError = null;
      captureErrorCode = null;
      void triggerCapture();
    }
  });

  root.addEventListener("pointerdown", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest<HTMLButtonElement>("[data-pad-button]");
    const padButton = button?.dataset.padButton;
    if (!button || button.disabled || !isPadButton(padButton) || inputControlsDisabled()) {
      return;
    }

    event.preventDefault();
    manualButtons = appendPadButton(manualButtons, padButton);
    publishMergedInputState();
  });

  root.addEventListener("keydown", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest<HTMLButtonElement>("[data-pad-button]");
    const padButton = button?.dataset.padButton;
    if (
      !button ||
      button.disabled ||
      event.repeat ||
      !isPadActivationKey(event.key) ||
      !isPadButton(padButton) ||
      inputControlsDisabled()
    ) {
      return;
    }

    event.preventDefault();
    manualButtons = appendPadButton(manualButtons, padButton);
    keyboardPadButtons = appendPadButton(keyboardPadButtons, padButton);
    publishMergedInputState();
    return;
  });

  root.addEventListener("keydown", (event) => {
    const button = keyboardButtonForCode(event.code);
    if (
      !button ||
      !inputSurfaceFocused() ||
      isTextInputTarget(event.target) ||
      isVisiblePadActivation(event)
    ) {
      return;
    }

    event.preventDefault();
    if (event.repeat || inputControlsDisabled()) {
      return;
    }
    keyboardButtons = appendPadButton(keyboardButtons, button);
    publishMergedInputState();
  });

  const releaseMappedKeyboardButton = (event: KeyboardEvent) => {
    const button = keyboardButtonForCode(event.code);
    if (!button || isTextInputTarget(event.target) || isVisiblePadActivation(event)) {
      return;
    }

    if (inputSurfaceFocused() || keyboardButtons.includes(button)) {
      event.preventDefault();
    }
    if (!keyboardButtons.includes(button)) {
      return;
    }
    keyboardButtons = removePadButton(keyboardButtons, button);
    publishMergedInputState();
  };
  globalThis.addEventListener?.("keyup", releaseMappedKeyboardButton);

  const releaseKeyboardPadButtons = (event: KeyboardEvent) => {
    if (!isPadActivationKey(event.key) || keyboardPadButtons.length === 0) {
      return;
    }

    event.preventDefault();
    manualButtons = removePadButtons(manualButtons, keyboardPadButtons);
    keyboardPadButtons = [];
    publishMergedInputState();
  };
  globalThis.addEventListener?.("keyup", releaseKeyboardPadButtons);

  const releasePadButtons = () => {
    if (manualButtons.length === 0) {
      return;
    }
    manualButtons = [];
    keyboardPadButtons = [];
    publishMergedInputState();
  };
  root.addEventListener("pointercancel", releasePadButtons);
  root.addEventListener("pointerleave", releasePadButtons);
  globalThis.addEventListener?.("pointerup", releasePadButtons);
  globalThis.addEventListener?.("gamepadconnected", () => {
    const changed = clearRecoveryEvents("gamepad_disconnected");
    startGamepadPolling();
    if (changed) {
      render();
    }
  });
  globalThis.addEventListener?.("gamepaddisconnected", () => {
    stopGamepadPolling();
    if (auth.status === "active" && auth.session.session_id) {
      recordRecoveryEvent({ code: "gamepad_disconnected" });
    }
    if (gamepadButtons.length === 0) {
      render();
      return;
    }
    gamepadButtons = [];
    publishMergedInputState();
  });

  async function transitionRun(
    nextAction: Exclude<SessionActionState, "idle">,
    transition: ((sessionId: string) => Promise<{ state: SessionState; current_frame: number }>) | undefined
  ) {
    if (!transition || sessionAction !== "idle" || auth.status !== "active" || !auth.session.session_id) {
      return;
    }
    const sessionId = auth.session.session_id;
    const requestSeq = ++authRequestSeq;
    sessionAction = nextAction;
    auth = { ...auth, error: null };
    render();
    try {
      const response = await transition(sessionId);
      if (
        requestSeq !== authRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      auth = {
        ...auth,
        session: {
          ...auth.session,
          state: response.state,
          current_frame: response.current_frame
        },
        error: null
      };
      sessionAction = "idle";
      render();
      refreshRunStatus();
      refreshPreview();
    } catch (error) {
      if (requestSeq !== authRequestSeq) {
        return;
      }
      sessionAction = "idle";
      const displayError = runtimeDisplayError(error, "backend_unavailable");
      auth = {
        ...auth,
        error: displayError
      };
      render("alert");
    }
  }

  async function submitCaptureLabels(form: HTMLFormElement) {
    const updateLabels = client.updateLabels?.bind(client);
    const detail = captureReview.detail;
    if (
      !updateLabels ||
      auth.status !== "active" ||
      !auth.session.session_id ||
      !detail ||
      !captureIsLabelable(detail)
    ) {
      return;
    }
    const formData = new FormData(form);
    const selectedRoles = LABEL_DRAWER_ROLES.filter(
      (role) => formData.get(`label_${role}`) === "on"
    );
    const currentRoles = detail.labels.filter(isLabelRole);
    const note = safePrivateNote(String(formData.get("private_note") ?? ""));
    const updates = labelUpdatesForSelection(detail.capture_id, currentRoles, selectedRoles, note);
    const dedupSelection = dedupUpdatesForSelection(detail.capture_id, formData);
    if (dedupSelection.error) {
      captureReview = {
        ...captureReview,
        conflicts: [dedupSelection.error],
        error: null
      };
      render();
      return;
    }
    const dedupUpdates = dedupSelection.updates;
    if (updates.length === 0 && dedupUpdates.length === 0) {
      captureReview = {
        ...captureReview,
        conflicts: [],
        error: null
      };
      render();
      return;
    }

    const requestSeq = ++captureReviewRequestSeq;
    const sessionId = auth.session.session_id;
    captureReview = { ...captureReview, labelSaving: true, conflicts: [], error: null };
    render();
    try {
      const response = await updateLabels({
        sessionId,
        idempotencyKey: createIdempotencyKey(),
        updates,
        dedupUpdates
      });
      if (
        requestSeq !== captureReviewRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      const conflicts = response.conflicts.map(sanitizeRuntimeDisplayError);
      captureReview = {
        ...captureReview,
        labelSaving: false,
        conflicts,
        error: null
      };
      if (response.applied) {
        form.reset();
        void refreshCaptureReview(detail.capture_id);
      } else {
        if (conflicts.some((conflict) => conflict.code === "label_conflict")) {
          recordRecoveryEvent({ code: "label_conflict", message: "Resolve the conflicting label role before saving the draft." });
        }
        render();
      }
    } catch (error) {
      if (requestSeq !== captureReviewRequestSeq) {
        return;
      }
      const display = runtimeDisplayError(error, "label_conflict");
      captureReview = {
        ...captureReview,
        labelSaving: false,
        conflicts: [display],
        error: display.message
      };
      recordRecoveryEvent({ code: display.code, message: display.message });
      render();
    }
  }

  async function triggerCapture() {
    const trigger = client.triggerCapture?.bind(client);
    if (
      !trigger ||
      capturePending ||
      auth.status !== "active" ||
      !auth.session.session_id ||
      auth.session.active_capture_job_id !== null ||
      auth.session.preview_stale ||
      !preview ||
      preview.stale ||
      focusState === "hidden"
    ) {
      return;
    }
    const requestSeq = ++captureRequestSeq;
    const sessionId = auth.session.session_id;
    capturePending = true;
    captureError = null;
    captureErrorCode = null;
    render();
    try {
      const response = await trigger({
        sessionId,
        idempotencyKey: createIdempotencyKey(),
        observedPreviewFrame: preview.frame
      });
      if (
        requestSeq !== captureRequestSeq ||
        auth.status !== "active" ||
        auth.session.session_id !== sessionId
      ) {
        return;
      }
      captureJob = captureJobFromTrigger(response);
      capturePending = isActiveCaptureStatus(response.status);
      captureErrorCode = null;
      auth = {
        ...auth,
        session: {
          ...auth.session,
          active_capture_job_id: isActiveCaptureStatus(response.status) ? response.job_id : null
        }
      };
      render();
      void refreshCaptureJob(response.job_id);
    } catch (error) {
      if (requestSeq !== captureRequestSeq) {
        return;
      }
      const displayError = runtimeDisplayError(error, "capture_failed");
      capturePending = displayError.code === "capture_in_progress";
      captureError = displayError.message;
      captureErrorCode = displayError.code;
      if (displayError.code === "capture_in_progress") {
        refreshRunStatus();
      }
      render();
    }
  }

  function inputControlsDisabled(): boolean {
    const state = auth.status === "active" ? auth.session.state : null;
    // Input is accepted while single-stepping (running) or continuously playing.
    if (state !== "running" && state !== "playing") {
      return true;
    }
    if (focusState === "hidden" || inputBlockingError(auth.error?.code)) {
      return true;
    }
    // During live play, frames arrive over /ws/frames, so preview staleness is
    // not a gate. In single-step mode the existing stale gates apply.
    if (state === "playing") {
      return false;
    }
    return auth.session.preview_stale || Boolean(preview?.stale);
  }

  function sendInputState(buttons: PadButton[]) {
    if (!inputSocket || auth.status !== "active" || !auth.session.session_id) {
      return;
    }
    inputSocket.sendInput({
      clientEventId: createIdempotencyKey(),
      clientTimeMs: Date.now(),
      source: "combined",
      buttons
    });
  }

  function publishMergedInputState() {
    const previousButtons = pressedButtons;
    const previousNeutralizedDirections = neutralizedDirections;
    const merged = mergeInputButtons([manualButtons, keyboardButtons, gamepadButtons]);
    if (
      samePadButtons(previousButtons, merged.buttons) &&
      sameNeutralizedDirections(previousNeutralizedDirections, merged.neutralizedDirections)
    ) {
      return;
    }
    pressedButtons = merged.buttons;
    neutralizedDirections = merged.neutralizedDirections;
    sendInputState(pressedButtons);
    render();
  }

  function syncInputCaptureLifecycle() {
    if (inputControlsDisabled()) {
      clearInputSources();
      stopGamepadPolling();
      return;
    }
    startGamepadPolling();
  }

  function clearDisconnectedInputState() {
    const hadInput = clearInputSources(false);
    stopGamepadPolling();
    if (hadInput) {
      render();
    }
  }

  function recordRecoveryEvent(event: RecoveryEvent) {
    recoveryEvents = [
      ...recoveryEvents.filter((current) => current.code !== event.code),
      { code: event.code, message: event.message ? safeBrowserMessage(event.message) : undefined }
    ].slice(-4);
  }

  function clearRecoveryEvents(...codes: RecoveryCode[]): boolean {
    const nextEvents = recoveryEvents.filter((event) => !codes.includes(event.code));
    if (nextEvents.length === recoveryEvents.length) {
      return false;
    }
    recoveryEvents = nextEvents;
    return true;
  }

  function clearInputSources(sendRelease = true): boolean {
    const hadInput = pressedButtons.length > 0 || neutralizedDirections.length > 0;
    manualButtons = [];
    keyboardButtons = [];
    keyboardPadButtons = [];
    gamepadButtons = [];
    pressedButtons = [];
    neutralizedDirections = [];
    if (sendRelease && hadInput) {
      sendInputState([]);
    }
    return hadInput;
  }

  function startGamepadPolling() {
    if (gamepadPollCancel || !canPollGamepad()) {
      return;
    }
    const requestAnimationFrame = globalThis.requestAnimationFrame?.bind(globalThis);
    const cancelAnimationFrame = globalThis.cancelAnimationFrame?.bind(globalThis);
    if (!requestAnimationFrame || !cancelAnimationFrame) {
      return;
    }

    let frameId = 0;
    let active = true;
    const poll = () => {
      if (!active) {
        return;
      }
      if (!canPollGamepad()) {
        gamepadPollCancel = null;
        if (gamepadButtons.length > 0) {
          gamepadButtons = [];
          publishMergedInputState();
        }
        return;
      }

      const nextButtons = readGamepadButtons();
      if (!samePadButtons(gamepadButtons, nextButtons)) {
        gamepadButtons = nextButtons;
        publishMergedInputState();
      }
      frameId = requestAnimationFrame(poll);
    };

    frameId = requestAnimationFrame(poll);
    gamepadPollCancel = () => {
      active = false;
      cancelAnimationFrame(frameId);
      gamepadPollCancel = null;
    };
  }

  function stopGamepadPolling() {
    gamepadPollCancel?.();
  }

  function canPollGamepad(): boolean {
    return (
      auth.status === "active" &&
      auth.session.state === "running" &&
      !auth.session.preview_stale &&
      !inputBlockingError(auth.error?.code) &&
      !preview?.stale &&
      currentFocusState() === "focused" &&
      inputSurfaceFocused()
    );
  }

  function readGamepadButtons(): PadButton[] {
    const getGamepads = globalThis.navigator?.getGamepads?.bind(globalThis.navigator);
    const gamepads = getGamepads?.() ?? [];
    const gamepad = Array.from(gamepads).find(
      (candidate): candidate is Gamepad =>
        Boolean(candidate && candidate.mapping === "standard" && candidate.connected !== false)
    );
    return buttonsFromStandardGamepad(gamepad);
  }

  function inputSurfaceFocused(): boolean {
    const activeElement = globalThis.document?.activeElement;
    return (
      activeElement instanceof Element &&
      root.contains(activeElement) &&
      Boolean(activeElement.closest("[data-input-focus-surface]"))
    );
  }

  root.addEventListener(
    "error",
    (event) => {
      const target = event.target instanceof Element ? event.target : null;
      if (!target?.matches("[data-preview-image]") || auth.status !== "active") {
        return;
      }
      preview = null;
      previewUnavailable = false;
      previewRequestSeq += 1;
      render();
      const refreshSeq = ++authRequestSeq;
      refreshSession(auth, client).then((next) => {
        applyAuthResult(refreshSeq, next);
      });
    },
    true
  );

  const updateFocusState = (forcedFocusState?: FocusState) => {
    const nextFocusState = forcedFocusState ?? operatorFocusState();
    const focusChanged = focusState !== nextFocusState;
    focusState = nextFocusState;
    let inputStateCleared = false;
    if (focusState === "hidden" || focusState === "blurred") {
      inputStateCleared = clearInputSources();
      stopGamepadPolling();
    } else {
      startGamepadPolling();
    }
    if (focusChanged || inputStateCleared) {
      render();
    }
  };
  const queueFocusStateUpdate = () => {
    globalThis.queueMicrotask?.(() => updateFocusState());
  };
  root.addEventListener("focusin", () => updateFocusState());
  root.addEventListener("focusout", queueFocusStateUpdate);
  globalThis.addEventListener?.("focus", () => updateFocusState());
  globalThis.addEventListener?.("blur", () => updateFocusState("blurred"));
  globalThis.document?.addEventListener?.("visibilitychange", () => updateFocusState());
  globalThis.addEventListener?.("pagehide", () => updateFocusState("hidden"));

  void refreshServiceBackendMode();
  render("start");
  const refreshSeq = ++authRequestSeq;
  refreshSession(auth, client).then((next) => {
    applyAuthResult(refreshSeq, next);
  });
}

function sameNeutralizedDirections(
  left: NeutralizedDirection[],
  right: NeutralizedDirection[]
): boolean {
  if (left.length !== right.length) {
    return false;
  }
  return left.every((direction, index) => direction === right[index]);
}

function operatorFocusState(): FocusState {
  const baseFocusState = currentFocusState();
  if (baseFocusState !== "focused") {
    return baseFocusState;
  }
  const activeElement = globalThis.document?.activeElement;
  return activeElement instanceof Element && activeElement.closest("[data-input-focus-surface]")
    ? "focused"
    : "blurred";
}

function recoveryNoticesForModel(
  model: OperatorViewModel,
  recoveryEvents: RecoveryEvent[]
): RecoveryNotice[] {
  const notices: RecoveryNotice[] = [];
  const addNotice = (notice: RecoveryNotice) => {
    if (!notices.some((current) => current.code === notice.code)) {
      notices.push(notice);
    }
  };

  if (model.auth.error) {
    addNotice(recoveryNoticeFromError(model.auth.error, model.auth.status === "faulted"));
  } else {
    const authNotice = recoveryNoticeFromAuthStatus(model.auth.status);
    if (authNotice) {
      addNotice(authNotice);
    }
  }

  if (model.previewState === "stale") {
    addNotice(recoveryNotice("frame_stale"));
  }
  if (model.capturePending || model.activeCaptureJobId) {
    addNotice(recoveryNotice("capture_in_progress"));
  }
  if (model.captureError || model.captureJob?.status === "failed") {
    addNotice(
      recoveryNotice(model.captureErrorCode ?? "capture_failed", model.captureError ?? model.captureJob?.error?.message)
    );
  }
  if (model.validationState === "failed") {
    addNotice(recoveryNotice("validation_failed"));
  }

  for (const event of recoveryEvents) {
    addNotice(recoveryNotice(event.code, event.message));
  }

  return notices;
}

function recoveryNoticeFromAuthStatus(status: AuthSessionState["status"]): RecoveryNotice | null {
  switch (status) {
    case "auth_rejected":
      return recoveryNotice("auth_rejected");
    case "origin_rejected":
      return recoveryNotice("origin_rejected");
    case "session_active_elsewhere":
      return recoveryNotice("session_active_elsewhere");
    case "faulted":
      return recoveryNotice("bridge_unavailable");
    default:
      return null;
  }
}

function recoveryNoticeFromError(error: RuntimeErrorDisplay, bridgeContext = false): RecoveryNotice {
  if (bridgeContext && error.code === "backend_unavailable") {
    return recoveryNotice("bridge_unavailable", error.message);
  }
  return recoveryNotice(error.code, error.message);
}

function recoveryNotice(code: RecoveryCode, message?: string): RecoveryNotice {
  const defaultMessage = recoveryDefaultMessage(code);
  return {
    code,
    title: recoveryTitle(code),
    message: safeBrowserMessage(message ?? defaultMessage, defaultMessage),
    severity: recoverySeverity(code)
  };
}

function recoveryTitle(code: RecoveryCode): string {
  switch (code) {
    case "bridge_unavailable":
      return "Bridge unavailable";
    case "auth_rejected":
      return "Authentication rejected";
    case "origin_rejected":
      return "Origin rejected";
    case "session_active_elsewhere":
      return "Session active elsewhere";
    case "backend_unavailable":
      return "Backend unavailable";
    case "frame_stale":
      return "Framebuffer stale";
    case "frame_unavailable":
      return "Frame not available yet";
    case "capture_in_progress":
      return "Capture in progress";
    case "capture_failed":
      return "Capture failed";
    case "label_conflict":
      return "Label conflict";
    case "validation_failed":
      return "Validation failed";
    case "gamepad_disconnected":
      return "Gamepad disconnected";
    case "websocket_reconnect":
      return "WebSocket reconnect";
    case "bad_request":
      return "Request rejected";
    case "session_inactive":
      return "Session expired";
  }
}

function recoveryDefaultMessage(code: RecoveryCode): string {
  switch (code) {
    case "bridge_unavailable":
      return "Keep the UI open and retry session status when the bridge is reachable.";
    case "auth_rejected":
      return "Return to the locked screen and start a new authenticated session.";
    case "origin_rejected":
      return "Open the operator UI from an allowed origin.";
    case "session_active_elsewhere":
      return "Another operator session is active; this UI will not overwrite it.";
    case "backend_unavailable":
      return "Controls are disabled while the backend is unavailable; retry status when it recovers.";
    case "frame_stale":
      return "Preview and input are paused until a fresh frame is available.";
    case "frame_unavailable":
      return "The guest has not produced a frame yet; the preview will connect when one exists.";
    case "capture_in_progress":
      return "The active capture is still running; duplicate capture requests are disabled.";
    case "capture_failed":
      return "Keep the failed capture visible and retry with a new request when ready.";
    case "label_conflict":
      return "Resolve the conflicting label role before saving the draft.";
    case "validation_failed":
      return "Validation failed; inspect the private server-side report.";
    case "gamepad_disconnected":
      return "Gamepad input was cleared; keyboard input remains available.";
    case "websocket_reconnect":
      return "Input was cleared and the UI will resume from current runtime status.";
    case "bad_request":
      return "The request was rejected without exposing private details.";
    case "session_inactive":
      return "The session is no longer active; return to the locked screen.";
  }
}

function recoverySeverity(code: RecoveryCode): RecoveryNotice["severity"] {
  switch (code) {
    case "auth_rejected":
    case "origin_rejected":
    case "session_active_elsewhere":
    case "backend_unavailable":
    case "bridge_unavailable":
    case "frame_stale":
    case "capture_failed":
    case "label_conflict":
    case "validation_failed":
      return "critical";
    case "capture_in_progress":
    case "frame_unavailable":
    case "gamepad_disconnected":
    case "websocket_reconnect":
    case "bad_request":
    case "session_inactive":
      return "warning";
  }
}

function inputBlockingError(code: RuntimeErrorCode | undefined): boolean {
  return code === "backend_unavailable" || code === "frame_stale" || code === "session_inactive";
}

function applyInputAckToState(
  message: InputAckMessage,
  currentTail: PadlogTailEntry[]
): { pressedButtons: PadButton[]; padlogTail: PadlogTailEntry[] } {
  const buttons = padWordButtons(message.payload.pad_word);
  const entry: PadlogTailEntry = {
    frame: message.payload.assigned_frame,
    padWord: message.payload.pad_word,
    status: message.payload.status,
    source: sourceFromMessage(message),
    buttons
  };
  return {
    pressedButtons: buttons,
    padlogTail: [...currentTail, entry].slice(-5)
  };
}

function applyInputRejectToState(
  message: InputRejectMessage,
  currentTail: PadlogTailEntry[]
): PadlogTailEntry[] {
  const entry: PadlogTailEntry = {
    frame: 0,
    padWord: 0,
    status: "dropped",
    source: sourceFromMessage(message),
    buttons: []
  };
  return [...currentTail, entry].slice(-5);
}

function captureReviewForView(state: CaptureReviewState): CaptureReviewState {
  return {
    captures: state.captures,
    selectedCaptureId: state.selectedCaptureId,
    detail: state.detail,
    features: state.features,
    featureError: state.featureError ? safeBrowserMessage(state.featureError) : null,
    labels: state.labels,
    loading: state.loading,
    labelSaving: state.labelSaving,
    error: state.error ? safeBrowserMessage(state.error) : null,
    conflicts: state.conflicts.map(sanitizeRuntimeDisplayError)
  };
}

function selectCaptureIdForReview(
  captures: CaptureSummary[],
  preferredCaptureId: string | null
): string | null {
  if (preferredCaptureId && captures.some((capture) => capture.capture_id === preferredCaptureId)) {
    return preferredCaptureId;
  }
  return captures[0]?.capture_id ?? null;
}

function renderCaptureReviewPanel(model: OperatorViewModel): string {
  const review = model.captureReview;
  const revision = review.labels ? `r${review.labels.label_revision}` : "no draft";
  return `
    <article class="panel capture-review-panel" aria-busy="${review.loading || review.labelSaving}" data-capture-review>
      <div class="panel-header">
        <div>
          <p class="eyebrow">Capture Review</p>
          <h2>${review.detail ? statusLabel(review.detail.status) : "recent"}</h2>
        </div>
        <div class="panel-actions">
          <span class="status-pill">${escapeHtml(review.loading ? "loading" : revision)}</span>
          <button
            type="button"
            data-capture-review-action="refresh"
            ${model.auth.status !== "active" || review.loading ? "disabled" : ""}
          >Refresh</button>
        </div>
      </div>
      ${
        review.error
          ? `<p class="session-alert" role="alert" data-capture-review-alert>${escapeHtml(review.error)}</p>`
          : ""
      }
      ${renderRecentCaptures(review)}
      ${renderCaptureDetail(model)}
    </article>
  `;
}

function renderRecentCaptures(review: CaptureReviewState): string {
  if (review.captures.length === 0) {
    return `
      <div class="empty-state" data-capture-empty>
        <strong>No captures</strong>
        <span>Waiting for completed captures.</span>
      </div>
    `;
  }
  return `
    <ol class="capture-list" aria-label="Recent captures">
      ${review.captures.map((capture) => renderCaptureRow(capture, review.selectedCaptureId)).join("")}
    </ol>
  `;
}

function renderCaptureRow(capture: CaptureSummary, selectedCaptureId: string | null): string {
  const selected = capture.capture_id === selectedCaptureId;
  return `
    <li data-capture-status="${escapeHtml(capture.status)}">
      <button
        type="button"
        data-capture-review-action="select"
        data-capture-id="${escapeHtml(capture.capture_id)}"
        aria-pressed="${selected}"
      >
        <span>#${capture.frame} ${escapeHtml(statusLabel(capture.status))}</span>
        <strong>${escapeHtml(capture.capture_id)}</strong>
        <em>${capture.labelable ? "labelable" : "not labelable"}</em>
      </button>
    </li>
  `;
}

function renderCaptureDetail(model: OperatorViewModel): string {
  const review = model.captureReview;
  const detail = review.detail;
  if (!detail) {
    return "";
  }
  return `
    <section class="capture-detail" data-capture-detail>
      <div class="capture-detail-header">
        <div>
          <p class="eyebrow">Detail</p>
          <h3>${escapeHtml(detail.capture_id)}</h3>
        </div>
        <span class="status-pill">${escapeHtml(captureLabelableStatus(detail))}</span>
      </div>
      <div class="capture-detail-grid">
        ${renderCapturePreview(detail)}
        <dl class="provenance-list" aria-label="Sanitized provenance">
          <div><dt>Source</dt><dd>${escapeHtml(detail.sanitized_provenance.capture_source)}</dd></div>
          <div><dt>Layout</dt><dd>${escapeHtml(detail.sanitized_provenance.layout_hash)}</dd></div>
          <div><dt>Spec</dt><dd>${escapeHtml(detail.sanitized_provenance.capture_spec_hash)}</dd></div>
          <div><dt>Map</dt><dd>${escapeHtml(detail.sanitized_provenance.map_hash)}</dd></div>
        </dl>
      </div>
      ${renderPrivilegedFeatureShell(model)}
      ${renderLabelDrawer(model)}
    </section>
  `;
}

function renderCapturePreview(detail: CaptureDetailResponse): string {
  if (!detail.has_preview || !detail.preview_image_url) {
    return `
      <div class="capture-preview capture-preview-empty" role="img" aria-label="Capture preview unavailable">
        <span>Preview unavailable</span>
      </div>
    `;
  }
  return `
    <figure class="capture-preview">
      <img
        src="${escapeHtml(detail.preview_image_url)}"
        alt="Capture preview"
        width="256"
        height="224"
        loading="lazy"
        data-capture-preview
      />
    </figure>
  `;
}

function renderPrivilegedFeatureShell(model: OperatorViewModel): string {
  const detail = model.captureReview.detail;
  if (!detail) {
    return "";
  }
  const features = model.captureReview.features;
  const available =
    detail.privileged_features_available &&
    Boolean(model.auth.session.capabilities?.privileged_features) &&
    features?.available;
  const status = available ? "available" : detail.privileged_features_available ? "unavailable" : "none";
  const body = model.captureReview.featureError
    ? `<p class="session-alert" role="alert" data-feature-alert>${escapeHtml(model.captureReview.featureError)}</p>`
    : available && features.features.length > 0
      ? `<dl class="feature-list" aria-label="Privileged feature values">
          ${features.features.map(renderPrivilegedFeature).join("")}
        </dl>`
      : `<p class="feature-empty">${featureUnavailableMessage(detail, model)}</p>`;
  return `
    <section class="feature-shell" data-feature-panel>
      <div class="feature-shell-header">
        <div>
          <p class="eyebrow">Privileged Features</p>
          <h3>${escapeHtml(status)}</h3>
        </div>
        <span>${features?.features.length ?? 0}</span>
      </div>
      ${body}
    </section>
  `;
}

function renderPrivilegedFeature(feature: { name: string; value: number }): string {
  return `
    <div>
      <dt>${escapeHtml(feature.name)}</dt>
      <dd>${formatFeatureValue(feature.value)}</dd>
    </div>
  `;
}

function featureUnavailableMessage(detail: CaptureDetailResponse, model: OperatorViewModel): string {
  if (!detail.privileged_features_available) {
    return "No decoded feature map is available for this capture.";
  }
  if (!model.auth.session.capabilities?.privileged_features) {
    return "The active session does not grant privileged feature access.";
  }
  return "Decoded feature map returned no values.";
}

function formatFeatureValue(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(3).replace(/0+$/, "").replace(/\.$/, "");
}

function renderLabelDrawer(model: OperatorViewModel): string {
  const review = model.captureReview;
  const detail = review.detail;
  if (!detail) {
    return "";
  }
  const disabled = !captureIsLabelable(detail) || !model.auth.session.capabilities?.labels || review.labelSaving;
  const conflictHints = labelConflictHints(detail, review.labels);
  return `
    <form class="label-drawer" data-label-drawer-form="capture" autocomplete="off">
      <div class="capture-detail-header">
        <div>
          <p class="eyebrow">Label Drawer</p>
          <h3>${disabled ? "locked" : "draft"}</h3>
        </div>
        <button type="submit" ${disabled ? "disabled" : ""}>${review.labelSaving ? "Saving" : "Save"}</button>
      </div>
      ${renderLabelConflictList([...conflictHints, ...review.conflicts.map((conflict) => conflict.message)])}
      <fieldset class="label-options" ${disabled ? "disabled" : ""}>
        <legend>Roles</legend>
        ${LABEL_DRAWER_ROLES.map((role) => renderLabelOption(role, detail.labels.includes(role))).join("")}
      </fieldset>
      ${renderDedupEditorShell(review, detail)}
      <label class="private-note-field">
        Private note
        <textarea
          name="private_note"
          maxlength="512"
          rows="3"
          autocomplete="off"
          autocapitalize="none"
          spellcheck="false"
          data-private-note
          ${disabled ? "disabled" : ""}
        ></textarea>
      </label>
    </form>
  `;
}

function renderLabelOption(role: LabelRole, checked: boolean): string {
  return `
    <label>
      <input type="checkbox" name="label_${escapeHtml(role)}" ${checked ? "checked" : ""} />
      <span>${escapeHtml(labelRoleLabel(role))}</span>
    </label>
  `;
}

function renderLabelConflictList(messages: string[]): string {
  const safeMessages = messages.map((message) => safeBrowserMessage(message)).filter(Boolean);
  if (safeMessages.length === 0) {
    return "";
  }
  return `
    <ul class="label-conflicts" data-label-conflicts>
      ${safeMessages.map((message) => `<li>${escapeHtml(message)}</li>`).join("")}
    </ul>
  `;
}

function renderDedupEditorShell(
  review: CaptureReviewState,
  detail: CaptureDetailResponse
): string {
  const groups =
    review.labels?.dedup_groups.filter((group) => group.capture_ids.includes(detail.capture_id)) ?? [];
  return `
    <section class="dedup-editor" data-dedup-editor>
      <div class="capture-detail-header">
        <div>
          <p class="eyebrow">Dedup</p>
          <h3>${groups.length > 0 ? "candidate" : "new draft"}</h3>
        </div>
        <span class="status-pill">${groups.length}</span>
      </div>
      ${
        groups.length > 0
          ? `<ul class="dedup-list">${groups.map(renderDedupGroupSummary).join("")}</ul>`
          : ""
      }
      <div class="dedup-fields">
        <label>
          Relation
          <select name="dedup_relation" ${captureIsLabelable(detail) ? "" : "disabled"}>
            <option value="same_canonical_state">Same canonical state</option>
            <option value="distinct_stable_state">Distinct stable state</option>
          </select>
        </label>
        <label>
          Compare capture
          <input
            type="text"
            name="dedup_capture_id"
            autocomplete="off"
            spellcheck="false"
            ${captureIsLabelable(detail) ? "" : "disabled"}
          />
        </label>
        <label>
          Changed feature
          <input
            type="text"
            name="dedup_changed_feature"
            autocomplete="off"
            spellcheck="false"
            ${captureIsLabelable(detail) ? "" : "disabled"}
          />
        </label>
      </div>
    </section>
  `;
}

function renderDedupGroupSummary(group: LabelsSnapshotResponse["dedup_groups"][number]): string {
  return `
    <li>
      <strong>${escapeHtml(dedupRelationLabel(group.expected_relation))}</strong>
      <span>${group.capture_ids.length} captures, ${escapeHtml(group.status ?? "candidate")}</span>
    </li>
  `;
}

function renderPreviewImage(model: OperatorViewModel): string {
  if (!model.auth.session.active) {
    return "";
  }
  // During continuous Play the newest frame streams over the frames socket and
  // is painted onto this canvas (no <img>/object URL — see paintLiveFrame). The
  // canvas persists across re-renders so live frames have a stable draw target.
  if (model.sessionState === "playing") {
    return `<canvas
    data-preview-image
    data-preview-canvas
    role="img"
    aria-label="Live framebuffer preview"
  ></canvas>`;
  }
  if (!model.preview) {
    return "";
  }
  return `<img
    src="${escapeHtml(model.preview.image_url)}"
    width="${model.preview.width}"
    height="${model.preview.height}"
    alt="Framebuffer preview"
    data-preview-image
    data-preview-hash="${escapeHtml(model.preview.preview_hash)}"
  />`;
}

function renderValidationSummary(status: ValidationStatusView): string {
  const summary = status.summary.trim() ? safeBrowserMessage(status.summary) : "";
  const issues = status.issue_summaries.map((issue) => safeBrowserMessage(issue)).filter(Boolean);
  if (!summary && issues.length === 0) {
    return "";
  }
  return `
    <div class="validation-summary" data-validation-summary>
      ${summary ? `<p>${escapeHtml(summary)}</p>` : ""}
      ${
        issues.length > 0
          ? `<ul>${issues.map((issue) => `<li>${escapeHtml(issue)}</li>`).join("")}</ul>`
          : ""
      }
    </div>
  `;
}

function validationCommandLabel(status: ValidationStatusView): string {
  switch (status.command_class) {
    case "phase4_score_plan":
      return "score plan";
    case "redaction_scan":
      return "redaction scan";
    case "bundle_check":
      return "bundle check";
    case "context_check":
      return "context check";
    case "checksums":
      return "checksums";
    case "verifier":
      return "verifier";
    default:
      return "not selected";
  }
}

function validationTimestampLabel(status: ValidationStatusView): string {
  return status.completed_at ?? status.started_at ?? "not run";
}

function isRuntimeEvent(message: RuntimeWsMessage): message is RuntimeEventMessage {
  return (
    message.type === "session_updated" ||
    message.type === "run_updated" ||
    message.type === "capture_updated" ||
    message.type === "label_updated" ||
    message.type === "validation_updated"
  );
}

function runtimeEventStopsSession(message: RuntimeEventMessage): boolean {
  return (
    (message.type === "session_updated" || message.type === "run_updated") &&
    message.payload.state === "stopped"
  );
}

function validationStateToViewState(status: ValidationUpdatedPayload["status"]): OperatorViewModel["validationState"] {
  switch (status) {
    case "running":
      return "queued";
    case "passed":
      return "passed";
    case "failed":
      return "failed";
    case "not_run":
      return "idle";
  }
}

function validationStatusFromPayload(payload: ValidationStatusView): ValidationStatusView {
  return {
    status: payload.status,
    command_class: payload.command_class,
    started_at: payload.started_at,
    completed_at: payload.completed_at,
    summary: payload.summary.trim() ? safeBrowserMessage(payload.summary) : "",
    issue_summaries: payload.issue_summaries.map((issue) => safeBrowserMessage(issue)).filter(Boolean)
  };
}

function validationStatusForView(
  status: ValidationStatusView,
  state: OperatorViewModel["validationState"]
): ValidationStatusView {
  if (status.status !== "not_run" || state === "idle") {
    return validationStatusFromPayload(status);
  }
  return {
    ...INITIAL_VALIDATION_STATUS,
    status: validationStatusFromViewState(state)
  };
}

function validationStatusFromViewState(
  state: OperatorViewModel["validationState"]
): ValidationUpdatedPayload["status"] {
  switch (state) {
    case "queued":
      return "running";
    case "passed":
      return "passed";
    case "failed":
      return "failed";
    case "idle":
      return "not_run";
  }
}

const LABEL_DRAWER_ROLES: LabelRole[] = [
  "first_boss",
  "goal_positive",
  "goal_negative",
  "needs_review",
  "rejected"
];

function captureIsLabelable(detail: CaptureDetailResponse): boolean {
  return detail.labelable && detail.status !== "not_labelable" && detail.status !== "failed";
}

function captureLabelableStatus(detail: CaptureDetailResponse): string {
  if (detail.status === "not_labelable") {
    return "not labelable";
  }
  if (detail.status === "failed") {
    return "failed";
  }
  return detail.labelable ? "labelable" : "locked";
}

function labelUpdatesForSelection(
  captureId: string,
  currentRoles: LabelRole[],
  selectedRoles: LabelRole[],
  note: string | null
): LabelUpdate[] {
  const updates: LabelUpdate[] = [];
  for (const role of LABEL_DRAWER_ROLES) {
    const wasSelected = currentRoles.includes(role);
    const isSelected = selectedRoles.includes(role);
    if (isSelected && !wasSelected) {
      updates.push({ op: "upsert", capture_id: captureId, role, confidence: "candidate" });
    }
    if (!isSelected && wasSelected) {
      updates.push({ op: "delete", capture_id: captureId, role });
    }
  }
  if (note && selectedRoles.length > 0) {
    const noteTarget = updates.find((update) => update.op === "upsert") ?? {
      op: "upsert" as const,
      capture_id: captureId,
      role: selectedRoles[0]!,
      confidence: "candidate" as const
    };
    noteTarget.note = note;
    if (!updates.includes(noteTarget)) {
      updates.push(noteTarget);
    }
  }
  return updates;
}

function dedupUpdatesForSelection(captureId: string, formData: FormData): DedupSelection {
  const rawCompareCaptureId = String(formData.get("dedup_capture_id") ?? "").trim();
  const rawChangedFeature = String(formData.get("dedup_changed_feature") ?? "").trim();
  if (!rawCompareCaptureId && !rawChangedFeature) {
    return { updates: [], error: null };
  }
  const compareCaptureId = safeContractInput(rawCompareCaptureId);
  const changedFeature = safePublicFeatureName(rawChangedFeature);
  const relation = String(formData.get("dedup_relation") ?? "");
  if (
    !compareCaptureId ||
    !changedFeature ||
    compareCaptureId === captureId
  ) {
    return {
      updates: [],
      error: safeValidationError("Enter a different capture id and a public changed feature.")
    };
  }
  if (relation !== "same_canonical_state" && relation !== "distinct_stable_state") {
    return {
      updates: [],
      error: safeValidationError("Choose a dedup relation.")
    };
  }
  return {
    updates: [
      {
        op: "upsert",
        group_id: dedupGroupId(captureId, compareCaptureId),
        expected_relation: relation,
        capture_ids: [captureId, compareCaptureId],
        changed_features: [changedFeature],
        status: "candidate"
      }
    ],
    error: null
  };
}

function dedupGroupId(captureId: string, compareCaptureId: string): string {
  const pair = [captureId, compareCaptureId].sort();
  const left = pair[0]!;
  const right = pair[1]!;
  return `dedup-${left.slice(0, 48)}-${right.slice(0, 48)}`;
}

function safeContractInput(value: string): string | null {
  const normalized = value.trim();
  return /^[A-Za-z0-9._:-]{1,128}$/.test(normalized) ? normalized : null;
}

function safePublicFeatureName(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, " ");
  return /^[A-Za-z0-9 _.-]{1,128}$/.test(normalized) ? normalized : null;
}

function safeValidationError(message: string): RuntimeErrorDisplay {
  return {
    code: "bad_request",
    message,
    retryable: false,
    details: {}
  };
}

function safePrivateNote(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, " ");
  if (!normalized || normalized.length > 512 || /[\u0000-\u001f\u007f]/.test(normalized)) {
    return null;
  }
  return normalized;
}

function labelConflictHints(
  detail: CaptureDetailResponse,
  labels: LabelsSnapshotResponse | null
): string[] {
  if (!labels) {
    return [];
  }
  const hints: string[] = [];
  for (const role of ["first_boss", "goal_positive", "goal_negative"] as const) {
    const assignedCapture = labels.target_labels[role];
    if (assignedCapture && assignedCapture !== detail.capture_id && detail.labels.includes(role)) {
      hints.push(`${labelRoleLabel(role)} is assigned to another capture.`);
    }
  }
  const rejected = labels.status_labels.some(
    (status) => status.capture_id === detail.capture_id && status.status === "rejected"
  );
  const hasTarget = detail.labels.some((role) =>
    ["first_boss", "goal_positive", "goal_negative"].includes(role)
  );
  if (rejected && hasTarget) {
    hints.push("Rejected captures cannot also be target labels.");
  }
  return hints;
}

function isLabelRole(value: string): value is LabelRole {
  return (LABEL_DRAWER_ROLES as readonly string[]).includes(value);
}

function labelRoleLabel(role: LabelRole): string {
  switch (role) {
    case "first_boss":
      return "first boss";
    case "goal_positive":
      return "goal positive";
    case "goal_negative":
      return "goal negative";
    case "needs_review":
      return "needs review";
    case "rejected":
      return "rejected";
  }
}

function dedupRelationLabel(relation: "same_canonical_state" | "distinct_stable_state"): string {
  switch (relation) {
    case "same_canonical_state":
      return "same state";
    case "distinct_stable_state":
      return "distinct state";
  }
}

function sanitizeRuntimeDisplayError(error: RuntimeErrorDisplay): RuntimeErrorDisplay {
  return {
    code: error.code,
    message: safeBrowserMessage(error.message, recoveryDefaultMessage(error.code)),
    retryable: error.retryable,
    details: {}
  };
}

function captureJobFromTrigger(response: CaptureTriggerResponse): CaptureJobView {
  return {
    job_id: response.job_id,
    status: response.status,
    requested_frame: response.requested_frame,
    scheduled_frame: response.scheduled_frame,
    captured_frame: null,
    capture_id: null,
    labelable: false,
    has_preview: false,
    error: null
  };
}

function toCaptureJobView(response: CaptureJobResponse): CaptureJobView {
  return {
    job_id: response.job_id,
    status: response.status,
    requested_frame: response.requested_frame,
    scheduled_frame: response.scheduled_frame,
    captured_frame: response.captured_frame,
    capture_id: response.capture_id,
    labelable: response.labelable,
    has_preview: response.has_preview,
    error: response.error
  };
}

function mergeCaptureUpdate(
  current: CaptureJobView | null,
  update: { job_id: string; status: CaptureStatus; capture_id: string | null }
): CaptureJobView {
  return {
    job_id: update.job_id,
    status: update.status,
    requested_frame: current?.requested_frame ?? 0,
    scheduled_frame: current?.scheduled_frame ?? 0,
    captured_frame:
      update.status === "completed" || update.status === "not_labelable"
        ? current?.captured_frame ?? null
        : current?.captured_frame ?? null,
    capture_id: update.capture_id,
    labelable: current?.labelable ?? update.status === "completed",
    has_preview: current?.has_preview ?? update.capture_id !== null,
    error: current?.error ?? null
  };
}

function isActiveCaptureStatus(status: CaptureStatus): boolean {
  return status === "requested" || status === "capturing";
}

const UNSAFE_BROWSER_TEXT_PATTERN =
  /credential|password|secret|token|private|(?:^|\s)\/[A-Za-z0-9._~/-]+|(?:^|\s)~\/|[A-Za-z]:\\|\.env|raw [a-z]+|command output|stdout|stderr|feature bytes|validation report|stack trace|traceback|screenshot|hex dump/i;
const SAFE_BROWSER_MESSAGE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9 .,:;_()'-]{0,159}$/;

function runtimeDisplayError(error: unknown, fallbackCode: RuntimeErrorCode): RuntimeErrorDisplay {
  if (error instanceof RuntimeApiError) {
    return {
      ...error.display,
      message: safeBrowserMessage(error.display.message, recoveryDefaultMessage(error.display.code))
    };
  }
  return {
    code: fallbackCode,
    message: recoveryDefaultMessage(fallbackCode),
    retryable: true,
    details: {}
  };
}

function safeBrowserMessage(message: string, fallback = "Request failed."): string {
  const trimmedMessage = message.trim();
  return !SAFE_BROWSER_MESSAGE_PATTERN.test(trimmedMessage) ||
    UNSAFE_BROWSER_TEXT_PATTERN.test(trimmedMessage)
    ? fallback
    : trimmedMessage;
}

function padWordButtons(padWord: number): PadButton[] {
  return PAD_BUTTONS.filter((_button, index) => (padWord & (1 << index)) !== 0);
}

function isPadButton(value: string | undefined): value is PadButton {
  return typeof value === "string" && (PAD_BUTTONS as readonly string[]).includes(value);
}

function appendPadButton(buttons: PadButton[], button: PadButton): PadButton[] {
  return buttons.includes(button) ? buttons : [...buttons, button];
}

function removePadButton(buttons: PadButton[], button: PadButton): PadButton[] {
  return buttons.filter((pressed) => pressed !== button);
}

function removePadButtons(buttons: PadButton[], releasedButtons: PadButton[]): PadButton[] {
  return releasedButtons.reduce(removePadButton, buttons);
}

function isPadActivationKey(key: string): boolean {
  return key === " " || key === "Enter";
}

function padWordHex(padWord: number): string {
  return padWord.toString(16).padStart(4, "0").slice(-4);
}

function sourceFromMessage(message: { source_id: string }): PadlogTailEntry["source"] {
  if (message.source_id === "keyboard" || message.source_id === "gamepad") {
    return message.source_id;
  }
  return "combined";
}

function currentFocusState(): FocusState {
  if (globalThis.document?.hidden) {
    return "hidden";
  }
  if (typeof globalThis.document?.hasFocus === "function" && !globalThis.document.hasFocus()) {
    return "blurred";
  }
  return "focused";
}

function createIdempotencyKey(): string {
  return globalThis.crypto?.randomUUID?.() ?? "00000000-0000-4000-8000-000000000000";
}

function renderRecoveryPanel(notices: RecoveryNotice[]): string {
  if (notices.length === 0) {
    return "";
  }
  return `
    <article class="panel recovery-panel" aria-label="Recovery states">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Recovery</p>
          <h2>${notices.length === 1 ? notices[0]?.title : "attention needed"}</h2>
        </div>
        <span class="status-pill">${notices.length}</span>
      </div>
      <ul class="recovery-list">
        ${notices
          .map(
            (notice) => `<li data-recovery-code="${escapeHtml(notice.code)}" data-recovery-severity="${notice.severity}">
              <strong>${escapeHtml(notice.title)}</strong>
              <span>${escapeHtml(notice.message)}</span>
            </li>`
          )
          .join("")}
      </ul>
    </article>
  `;
}

function renderSessionPanel(model: OperatorViewModel): string {
  const auth = model.auth;
  const showSession = auth.status === "active" || auth.status === "stopping";
  const busy =
    auth.status === "starting" ||
    auth.status === "stopping" ||
    model.sessionAction === "pausing" ||
    model.sessionAction === "resuming";
  const idle = model.sessionAction === "idle";
  const state = auth.status === "active" ? auth.session.state : null;
  // Play/Resume from the post-start ready (running) or single-step (paused)
  // states; Pause freezes from either actively-emitting state (running/playing).
  const canPlay =
    auth.status === "active" && (state === "running" || state === "paused") && idle;
  const canPause =
    auth.status === "active" && (state === "running" || state === "playing") && idle;
  const canResume =
    auth.status === "active" && (state === "running" || state === "paused") && idle;
  return `
    <article class="panel session-panel" aria-busy="${busy}">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Session</p>
          <h2>${sessionStatusLabel(auth)}</h2>
        </div>
        <span class="status-pill">${escapeHtml(model.backendMode)}</span>
      </div>
      ${
        auth.error
          ? `<p class="session-alert" role="alert" tabindex="-1" data-session-alert>${escapeHtml(safeBrowserMessage(auth.error.message))}</p>`
          : ""
      }
      ${
        showSession
          ? `<dl class="session-meta" aria-label="Active session">
              <div><dt>Session</dt><dd>${escapeHtml(auth.session.session_id ?? "")}</dd></div>
              <div><dt>Run</dt><dd>${escapeHtml(auth.session.run_id ?? "")}</dd></div>
              <div><dt>Frame</dt><dd>${auth.session.current_frame}</dd></div>
              <div><dt>Focus</dt><dd>${focusLabel(model.focusState)}</dd></div>
            </dl>`
          : `<form class="session-form" data-session-form="start" autocomplete="off">
              <div class="button-row">
                <button type="submit" data-start-session-button ${auth.status === "starting" ? "disabled" : ""}>Start</button>
                <button type="button" disabled>Pause</button>
                <button type="button" disabled>Resume</button>
                <button type="button" class="danger" disabled>Stop</button>
              </div>
            </form>`
      }
      ${
        showSession
          ? `<div class="button-row single-action">
              <button type="button" data-run-action="play" ${canPlay ? "" : "disabled"}>Play</button>
              <button type="button" data-run-action="pause" ${canPause ? "" : "disabled"}>Pause</button>
              <button type="button" data-run-action="resume" ${canResume ? "" : "disabled"}>Resume</button>
              <button type="button" class="danger" data-session-action="logout" ${
                auth.status === "stopping" ? "disabled" : ""
              }>Stop</button>
            </div>`
          : ""
      }
    </article>
  `;
}

function renderPressedButtons(buttons: PadButton[]): string {
  if (buttons.length === 0) {
    return '<span class="muted">Pressed: none</span>';
  }
  return `<span>Pressed: ${buttons.map(escapeHtml).join(", ")}</span>`;
}

function renderNeutralizedDirections(directions: NeutralizedDirection[]): string {
  if (directions.length === 0) {
    return "";
  }
  return `<p class="neutralized-summary" aria-label="Neutralized directions">Neutralized: ${directions
    .map(escapeHtml)
    .join(", ")}</p>`;
}

function renderPadlogTail(entries: PadlogTailEntry[]): string {
  if (entries.length === 0) {
    return `
      <ol class="padlog-tail" aria-label="Recent padlog tail">
        <li><span>No input</span><strong>0000</strong></li>
      </ol>
    `;
  }
  return `
    <ol class="padlog-tail" aria-label="Recent padlog tail">
      ${entries
        .slice(-5)
        .reverse()
        .map(
          (entry) => `<li>
            <span>#${entry.frame} ${escapeHtml(entry.status)}</span>
            <strong>${padWordHex(entry.padWord)}</strong>
          </li>`
        )
        .join("")}
    </ol>
  `;
}

function renderCaptureJob(model: OperatorViewModel): string {
  const job = model.captureJob;
  if (!job) {
    return `
      <dl class="capture-job" aria-label="Capture job status">
        <div><dt>Job</dt><dd>${escapeHtml(model.activeCaptureJobId ?? "idle")}</dd></div>
        <div><dt>Status</dt><dd>${escapeHtml(model.activeCaptureJobId ? "capturing" : "idle")}</dd></div>
      </dl>
    `;
  }

  return `
    <dl class="capture-job" aria-label="Capture job status">
      <div><dt>Job</dt><dd>${escapeHtml(job.job_id)}</dd></div>
      <div><dt>Status</dt><dd>${escapeHtml(statusLabel(job.status))}</dd></div>
      <div><dt>Requested</dt><dd>#${job.requested_frame}</dd></div>
      <div><dt>Scheduled</dt><dd>#${job.scheduled_frame}</dd></div>
      <div><dt>Capture</dt><dd>${escapeHtml(job.capture_id ?? "pending")}</dd></div>
      <div><dt>Labelable</dt><dd>${job.labelable ? "yes" : "no"}</dd></div>
    </dl>
  `;
}

function capturePanelTitle(model: OperatorViewModel): string {
  if (model.capturePending || model.activeCaptureJobId) {
    return "capturing";
  }
  if (model.captureJob) {
    return statusLabel(model.captureJob.status);
  }
  if (model.previewState === "stale") {
    return "preview stale";
  }
  return "ready";
}

function captureButtonDisabled(model: OperatorViewModel): boolean {
  return (
    // Capture needs a settled (Paused) frame and seals the input log; during
    // continuous Play it would race the loop for the worker slot. Block it —
    // the operator pauses to capture.
    model.sessionState === "playing" ||
    model.controlsDisabled ||
    model.capturePending ||
    model.activeCaptureJobId !== null ||
    model.captureErrorCode === "capture_in_progress" ||
    !model.preview ||
    !model.auth.session.capabilities?.capture
  );
}

function captureActionLabel(model: OperatorViewModel): string {
  return model.captureError || model.captureJob?.status === "failed" ? "Retry" : "Trigger";
}

function captureStatusSummary(model: OperatorViewModel): string {
  if (model.capturePending || model.activeCaptureJobId) {
    return model.captureJob?.job_id ?? model.activeCaptureJobId ?? "capturing";
  }
  return model.captureJob ? statusLabel(model.captureJob.status) : "idle";
}

function statusLabel(status: CaptureStatus): string {
  return status.replace("_", " ");
}

function focusLabel(state: FocusState): string {
  switch (state) {
    case "focused":
      return "focused";
    case "blurred":
      return "not focused";
    case "hidden":
      return "hidden";
  }
}

function focusTargetForAuth(auth: AuthSessionState): "alert" | "start" | "logout" {
  if (auth.error) {
    return "alert";
  }
  if (auth.status === "active") {
    return "logout";
  }
  return "start";
}

function focusSessionTarget(root: HTMLElement, target: "alert" | "start" | "logout"): void {
  const selector = {
    alert: "[data-session-alert]",
    start: "[data-start-session-button]",
    logout: "[data-session-action='logout']"
  }[target];
  root.querySelector<HTMLElement>(selector)?.focus();
}

function focusInputSurface(root: HTMLElement, padButton: PadButton | null = null): void {
  if (padButton) {
    const button = Array.from(root.querySelectorAll<HTMLButtonElement>("[data-pad-button]")).find(
      (candidate) => candidate.dataset.padButton === padButton
    );
    if (button) {
      button.focus({ preventScroll: true });
      return;
    }
  }
  root.querySelector<HTMLElement>("[data-input-focus-surface]")?.focus({ preventScroll: true });
}

function isTextInputTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  return Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}

function isVisiblePadButtonTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  return Boolean(target.closest("[data-pad-button]"));
}

function padButtonFromElement(element: Element | null | undefined): PadButton | null {
  const padButton = element?.closest<HTMLElement>("[data-pad-button]")?.dataset.padButton;
  return isPadButton(padButton) ? padButton : null;
}

function isVisiblePadActivation(event: KeyboardEvent): boolean {
  return isPadActivationKey(event.key) && isVisiblePadButtonTarget(event.target);
}

function runtimeStat(label: string, value: string): string {
  return `<div><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(value)}</dd></div>`;
}

function sessionStatusLabel(auth: AuthSessionState): string {
  switch (auth.status) {
    case "auth_rejected":
      return "authentication rejected";
    case "session_active_elsewhere":
      return "active elsewhere";
    case "expired":
      return "expired";
    case "origin_rejected":
      return "origin rejected";
    case "starting":
      return "starting";
    case "stopping":
      return "stopping";
    case "faulted":
      return "faulted";
    case "active":
      return stateLabel(auth.session.state);
    case "locked":
      return "locked";
  }
}

function stateLabel(state: SessionState): string {
  return state.replace("_", " ");
}

function previewLabel(state: OperatorViewModel["previewState"]): string {
  if (state === "waiting") {
    return "not connected";
  }
  return state === "unavailable" ? "no frame yet" : state;
}

function validationLabel(state: OperatorViewModel["validationState"]): string {
  return state === "idle" ? "not queued" : state;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => {
    switch (character) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      case "'":
        return "&#39;";
      default:
        return character;
    }
  });
}
