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
  RuntimeWebSocketClient,
  type FrameCurrentResponse,
  type RuntimeEventMessage,
  type RuntimeWsMessage,
  type ValidationUpdatedPayload
} from "./runtimeClient";
import {
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  type BackendMode,
  type SessionState
} from "./runtimeContract";
import type { RuntimeConfig } from "./runtimeConfig";

type OperatorViewModel = {
  backendMode: BackendMode;
  sessionState: SessionState;
  currentFrame: number;
  previewState: "waiting" | "fresh" | "stale";
  validationState: "idle" | "queued" | "passed" | "failed";
  config: RuntimeConfig;
  auth: AuthSessionState;
  preview: FrameCurrentResponse | null;
};

type OperatorRuntimeViewState = {
  validationState: OperatorViewModel["validationState"];
  preview: FrameCurrentResponse | null;
};

export type RuntimeEventClient = Pick<RuntimeWebSocketClient, "eventSocket">;
export type RuntimePreviewClient = Pick<RuntimeApiClient, "currentFrame">;
type OperatorRuntimeClient = RuntimeSessionClient & Partial<RuntimePreviewClient>;

const INITIAL_VIEW_MODEL: Omit<OperatorViewModel, "config" | "auth"> = {
  backendMode: "synthetic",
  sessionState: "idle",
  currentFrame: 0,
  previewState: "waiting",
  validationState: "idle",
  preview: null
};

export function renderOperatorApp(
  config: RuntimeConfig,
  auth: AuthSessionState = initialAuthSessionState(),
  runtimeView: OperatorRuntimeViewState = {
    validationState: INITIAL_VIEW_MODEL.validationState,
    preview: null
  }
): string {
  const model: OperatorViewModel = {
    ...INITIAL_VIEW_MODEL,
    backendMode: auth.session.backend_mode,
    sessionState: auth.session.state,
    currentFrame: runtimeView.preview?.frame ?? auth.session.current_frame,
    previewState: auth.session.active
      ? runtimeView.preview
        ? runtimeView.preview.stale
          ? "stale"
          : "fresh"
        : auth.session.preview_stale
          ? "stale"
          : "waiting"
      : "waiting",
    validationState: runtimeView.validationState,
    config,
    auth,
    preview: runtimeView.preview
  };

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
        ${renderSessionPanel(model.auth, model.backendMode)}

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
        </article>

        <article class="panel input-panel">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Input</p>
              <h2>${PAD_LAYOUT_ID}</h2>
            </div>
            <span class="status-pill">v${PAD_LAYOUT_VERSION}</span>
          </div>
          <div class="pad-grid" aria-label="Pad layout">
            ${["A", "B", "X", "Y", "L", "R", "Up", "Down", "Left", "Right", "Start", "Select"]
              .map((button) => `<span>${button}</span>`)
              .join("")}
          </div>
        </article>

        <article class="panel validation-panel">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Validation</p>
              <h2>${validationLabel(model.validationState)}</h2>
            </div>
            <span class="status-pill">sanitized</span>
          </div>
          <ul class="status-list">
            <li><span>Runtime API</span><strong>v${model.config.schema_version}</strong></li>
            <li><span>Capture queue</span><strong>idle</strong></li>
            <li><span>Labels</span><strong>draft</strong></li>
          </ul>
        </article>
      </section>
    </main>
  `;
}

export function mountOperatorApp(
  root: HTMLElement,
  config: RuntimeConfig,
  client: OperatorRuntimeClient = new RuntimeApiClient(config),
  eventClient: RuntimeEventClient | null =
    typeof globalThis.WebSocket === "function" ? new RuntimeWebSocketClient(config) : null
): void {
  let auth = initialAuthSessionState();
  let authRequestSeq = 0;
  let previewRequestSeq = 0;
  let validationState: OperatorViewModel["validationState"] = INITIAL_VIEW_MODEL.validationState;
  let preview: FrameCurrentResponse | null = null;
  let eventSocket: ReturnType<RuntimeEventClient["eventSocket"]> | null = null;
  let eventSessionId: string | null = null;
  root.innerHTML = '<div data-operator-app></div><p class="session-live" aria-live="polite"></p>';
  const appRegion = root.querySelector<HTMLElement>("[data-operator-app]");
  const liveRegion = root.querySelector<HTMLElement>(".session-live");

  const render = (focusTarget?: "alert" | "credential" | "logout") => {
    if (!appRegion || !liveRegion) {
      return;
    }
    appRegion.innerHTML = renderOperatorApp(config, auth, { validationState, preview });
    liveRegion.textContent = sessionStatusLabel(auth);
    if (focusTarget) {
      focusSessionTarget(appRegion, focusTarget);
    }
  };

  const applyAuthResult = (requestSeq: number, next: AuthSessionState) => {
    if (requestSeq !== authRequestSeq) {
      return;
    }
    const previousSessionId = auth.session.session_id;
    auth = next;
    if (!auth.session.active || auth.session.session_id !== previousSessionId) {
      preview = null;
      previewRequestSeq += 1;
    }
    syncEventStream();
    render(focusTargetForAuth(auth));
    refreshPreview();
  };

  function closeEventStream() {
    eventSocket?.close();
    eventSocket = null;
    eventSessionId = null;
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
      onError: () => undefined
    });
  }

  function handleRuntimeEvent(message: RuntimeWsMessage) {
    if (!isRuntimeEvent(message) || message.session_id !== auth.session.session_id) {
      return;
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
          preview_stale: message.payload.preview_stale
        },
        error: null
      };
    }

    if (!auth.session.active) {
      closeEventStream();
      auth = initialAuthSessionState();
      validationState = INITIAL_VIEW_MODEL.validationState;
      preview = null;
      previewRequestSeq += 1;
      render("credential");
      return;
    }

    render();
    if (message.type === "session_updated" || message.type === "run_updated") {
      refreshPreview();
    }
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
          session: {
            ...auth.session,
            current_frame: nextPreview.frame,
            last_preview_frame: nextPreview.frame,
            preview_stale: nextPreview.stale
          }
        };
        render();
      })
      .catch(() => {
        if (requestSeq === previewRequestSeq) {
          preview = null;
          render();
        }
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
    preview = null;
    previewRequestSeq += 1;
    render();
    submitCredential(auth, client, credential).then((next) => {
      applyAuthResult(requestSeq, next);
    });
  });

  root.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest<HTMLButtonElement>("[data-session-action='logout']");
    if (!button || button.disabled || auth.status !== "active") {
      return;
    }

    const stateToStop = auth;
    const requestSeq = ++authRequestSeq;
    auth = { ...auth, status: "stopping", error: null };
    preview = null;
    previewRequestSeq += 1;
    syncEventStream();
    render();
    logoutSession(stateToStop, client).then((next) => {
      applyAuthResult(requestSeq, next);
    });
  });

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

  render("credential");
  const refreshSeq = ++authRequestSeq;
  refreshSession(auth, client).then((next) => {
    applyAuthResult(refreshSeq, next);
  });
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

function renderSessionPanel(auth: AuthSessionState, backendMode: BackendMode): string {
  const showSession = auth.status === "active" || auth.status === "stopping";
  const busy = auth.status === "starting" || auth.status === "stopping";
  return `
    <article class="panel session-panel" aria-busy="${busy}">
      <div class="panel-header">
        <div>
          <p class="eyebrow">Session</p>
          <h2>${sessionStatusLabel(auth)}</h2>
        </div>
        <span class="status-pill">${escapeHtml(backendMode)}</span>
      </div>
      ${
        auth.error
          ? `<p class="session-alert" role="alert" tabindex="-1" data-session-alert>${escapeHtml(auth.error.message)}</p>`
          : ""
      }
      ${
        showSession
          ? `<dl class="session-meta" aria-label="Active session">
              <div><dt>Session</dt><dd>${escapeHtml(auth.session.session_id ?? "")}</dd></div>
              <div><dt>Run</dt><dd>${escapeHtml(auth.session.run_id ?? "")}</dd></div>
              <div><dt>Frame</dt><dd>${auth.session.current_frame}</dd></div>
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
              <button type="button" disabled>Pause</button>
              <button type="button" disabled>Resume</button>
              <button type="button" class="danger" data-session-action="logout" ${
                auth.status === "stopping" ? "disabled" : ""
              }>Logout</button>
            </div>`
          : ""
      }
    </article>
  `;
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
