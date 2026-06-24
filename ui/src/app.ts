import {
  initialAuthSessionState,
  logoutSession,
  refreshSession,
  submitCredential,
  type AuthSessionState,
  type RuntimeSessionClient
} from "./authSession";
import {
  RuntimeApiClient,
  RuntimeApiError,
  RuntimeWebSocketClient,
  applyRunStatus,
  type CaptureJobResponse,
  type CaptureStatus,
  type CaptureTriggerResponse,
  type FrameCurrentResponse,
  type InputAckMessage,
  type InputRejectMessage,
  type PadButton,
  type RuntimeErrorCode,
  type RuntimeErrorDisplay,
  type RuntimeEventMessage,
  type RuntimeWsMessage,
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
  previewState: "waiting" | "fresh" | "stale";
  validationState: "idle" | "queued" | "passed" | "failed";
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
  preview: FrameCurrentResponse | null;
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

export type RuntimeEventClient = Pick<RuntimeWebSocketClient, "eventSocket">;
export type RuntimeInputClient = Pick<RuntimeWebSocketClient, "inputSocket">;
export type RuntimePreviewClient = Pick<RuntimeApiClient, "currentFrame">;
export type RuntimeRunClient = Pick<
  RuntimeApiClient,
  "runStatus" | "pauseRun" | "resumeRun" | "triggerCapture" | "captureJob"
>;
type OperatorRuntimeClient = RuntimeSessionClient & Partial<RuntimePreviewClient & RuntimeRunClient>;
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
type PadlogTailEntry = {
  frame: number;
  padWord: number;
  status: "applied" | "queued" | "dropped";
  source: "keyboard" | "gamepad" | "combined";
  buttons: PadButton[];
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
  preview: null,
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
  const controlsDisabled =
    auth.status !== "active" ||
    auth.session.state !== "running" ||
    view.focusState === "hidden" ||
    inputBlockingError(auth.error?.code) ||
    Boolean(auth.session.preview_stale || view.preview?.stale);
  const model: OperatorViewModel = {
    ...INITIAL_VIEW_MODEL,
    backendMode: auth.session.backend_mode,
    sessionState: auth.session.state,
    currentFrame: auth.session.current_frame,
    lastAppliedInputFrame: auth.session.last_applied_input_frame,
    lastPreviewFrame: view.preview?.frame ?? auth.session.last_preview_frame,
    activeCaptureJobId: auth.session.active_capture_job_id,
    previewState: auth.session.active
      ? auth.session.preview_stale || view.preview?.stale
          ? "stale"
          : view.preview
            ? "fresh"
            : "waiting"
      : "waiting",
    validationState: view.validationState,
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
          </ul>
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
            >Trigger</button>
          </div>
          ${
            model.captureError
              ? `<p class="session-alert" role="alert" tabindex="-1" data-capture-alert>${escapeHtml(safeBrowserMessage(model.captureError))}</p>`
              : ""
          }
          ${renderCaptureJob(model)}
        </article>
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
  let sessionAction: SessionActionState = "idle";
  let validationState: OperatorViewModel["validationState"] = INITIAL_VIEW_MODEL.validationState;
  let preview: FrameCurrentResponse | null = null;
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
  let eventSocket: ReturnType<RuntimeEventClient["eventSocket"]> | null = null;
  let eventSessionId: string | null = null;
  let inputSocket: ReturnType<RuntimeInputClient["inputSocket"]> | null = null;
  let inputSessionId: string | null = null;
  let gamepadPollCancel: (() => void) | null = null;
  root.innerHTML = '<div data-operator-app></div><p class="session-live" aria-live="polite"></p>';
  const appRegion = root.querySelector<HTMLElement>("[data-operator-app]");
  const liveRegion = root.querySelector<HTMLElement>(".session-live");

  const render = (focusTarget?: "alert" | "credential" | "logout") => {
    if (!appRegion || !liveRegion) {
      return;
    }
    syncInputCaptureLifecycle();
    const shouldRestoreInputFocus = !focusTarget && inputSurfaceFocused();
    const focusedPadButton = shouldRestoreInputFocus
      ? padButtonFromElement(globalThis.document?.activeElement)
      : null;
    appRegion.innerHTML = renderOperatorApp(config, auth, {
      validationState,
      preview,
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
    if (focusTarget) {
      focusSessionTarget(appRegion, focusTarget);
    } else if (shouldRestoreInputFocus) {
      focusInputSurface(appRegion, focusedPadButton);
    }
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
    auth = next;
    if (sessionWillReset) {
      preview = null;
      previewRequestSeq += 1;
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
    }
    sessionAction = "idle";
    syncEventStream();
    syncInputStream();
    render(focusTargetForAuth(auth));
    refreshRunStatus();
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
        refreshPreview();
        render();
      }
    });
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
      validationState = validationStateFromEvent(message.payload.status);
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
    }

    if (!auth.session.active) {
      closeEventStream();
      closeInputStream(true);
      auth = initialAuthSessionState();
      validationState = INITIAL_VIEW_MODEL.validationState;
      preview = null;
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
      render("credential");
      return;
    }

    render();
    if (message.type === "session_updated" || message.type === "run_updated") {
      syncInputStream();
      refreshPreview();
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
        if (requestSeq === previewRequestSeq) {
          preview = null;
          auth = { ...auth, error: runtimeDisplayError(error, "backend_unavailable") };
          render();
        }
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

    const formData = new FormData(form);
    const credential = String(formData.get("operator_credential") ?? "");
    form.reset();
    const requestSeq = ++authRequestSeq;
    auth = { ...auth, status: "starting", error: null };
    validationState = INITIAL_VIEW_MODEL.validationState;
    recoveryEvents = [];
    preview = null;
    previewRequestSeq += 1;
    render();
    submitCredential(auth, client, credential).then((next) => {
      applyAuthResult(requestSeq, next);
    });
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
    previewRequestSeq += 1;
    syncEventStream();
    render();
    logoutSession(stateToStop, client).then((next) => {
      applyAuthResult(requestSeq, next);
    });
  });

  root.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const action = target?.closest<HTMLButtonElement>("[data-run-action]")?.dataset.runAction;
    if (!action || auth.status !== "active" || !auth.session.session_id) {
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
    return (
      auth.status !== "active" ||
      auth.session.state !== "running" ||
      auth.session.preview_stale ||
      focusState === "hidden" ||
      inputBlockingError(auth.error?.code) ||
      Boolean(preview?.stale)
    );
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

  render("credential");
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

function renderPreviewImage(model: OperatorViewModel): string {
  if (!model.auth.session.active || !model.preview) {
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

function validationStateFromEvent(
  status: ValidationUpdatedPayload["status"]
): OperatorViewModel["validationState"] {
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
  /credential|password|secret|token|private|\/home\/|\/run\/|\.env|[A-Za-z]:\\|raw payload|raw command|command output|feature bytes|validation report|screenshot/i;

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
  return UNSAFE_BROWSER_TEXT_PATTERN.test(message) ? fallback : message;
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
  const canPause = auth.status === "active" && auth.session.state === "running" && model.sessionAction === "idle";
  const canResume = auth.status === "active" && auth.session.state === "paused" && model.sessionAction === "idle";
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
              <label>
                Operator credential
                <input
                  type="password"
                  name="operator_credential"
                  data-credential-input
                  autocomplete="one-time-code"
                  autocapitalize="none"
                  spellcheck="false"
                  required
                />
              </label>
              <div class="button-row">
                <button type="submit" ${auth.status === "starting" ? "disabled" : ""}>Start</button>
                <button type="button" disabled>Pause</button>
                <button type="button" disabled>Resume</button>
                <button type="button" class="danger" disabled>Stop</button>
              </div>
            </form>`
      }
      ${
        showSession
          ? `<div class="button-row single-action">
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
    model.controlsDisabled ||
    model.capturePending ||
    model.activeCaptureJobId !== null ||
    model.captureErrorCode === "capture_in_progress" ||
    !model.preview ||
    !model.auth.session.capabilities?.capture
  );
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

function focusTargetForAuth(auth: AuthSessionState): "alert" | "credential" | "logout" {
  if (auth.error) {
    return "alert";
  }
  if (auth.status === "active") {
    return "logout";
  }
  return "credential";
}

function focusSessionTarget(root: HTMLElement, target: "alert" | "credential" | "logout"): void {
  const selector = {
    alert: "[data-session-alert]",
    credential: "[data-credential-input]",
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
  return state === "waiting" ? "not connected" : state;
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
