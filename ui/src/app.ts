import {
  initialAuthSessionState,
  logoutSession,
  refreshSession,
  submitCredential,
  type AuthSessionState,
  type RuntimeSessionClient
} from "./authSession";
import { RuntimeApiClient } from "./runtimeClient";
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
};

const INITIAL_VIEW_MODEL: Omit<OperatorViewModel, "config" | "auth"> = {
  backendMode: "synthetic",
  sessionState: "idle",
  currentFrame: 0,
  previewState: "waiting",
  validationState: "idle"
};

export function renderOperatorApp(
  config: RuntimeConfig,
  auth: AuthSessionState = initialAuthSessionState()
): string {
  const model: OperatorViewModel = {
    ...INITIAL_VIEW_MODEL,
    backendMode: auth.session.backend_mode,
    sessionState: auth.session.state,
    currentFrame: auth.session.current_frame,
    previewState: auth.session.active ? (auth.session.preview_stale ? "stale" : "fresh") : "waiting",
    config,
    auth
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
          <div class="preview-surface" aria-label="Framebuffer preview"></div>
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
  client: RuntimeSessionClient = new RuntimeApiClient(config)
): void {
  let auth = initialAuthSessionState();
  let authRequestSeq = 0;

  const render = () => {
    root.innerHTML = renderOperatorApp(config, auth);
  };

  const applyAuthResult = (requestSeq: number, next: AuthSessionState) => {
    if (requestSeq !== authRequestSeq) {
      return;
    }
    auth = next;
    render();
  };

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
    render();
    logoutSession(stateToStop, client).then((next) => {
      applyAuthResult(requestSeq, next);
    });
  });

  render();
  const refreshSeq = ++authRequestSeq;
  refreshSession(auth, client).then((next) => {
    applyAuthResult(refreshSeq, next);
  });
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
      <p class="session-live" aria-live="polite">${sessionStatusLabel(auth)}</p>
      ${auth.error ? `<p class="session-alert" role="alert">${escapeHtml(auth.error.message)}</p>` : ""}
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
