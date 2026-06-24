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
};

const INITIAL_VIEW_MODEL: Omit<OperatorViewModel, "config"> = {
  backendMode: "synthetic",
  sessionState: "idle",
  currentFrame: 0,
  previewState: "waiting",
  validationState: "idle"
};

export function renderOperatorApp(config: RuntimeConfig): string {
  const model: OperatorViewModel = {
    ...INITIAL_VIEW_MODEL,
    config
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
        <article class="panel session-panel">
          <div class="panel-header">
            <div>
              <p class="eyebrow">Session</p>
              <h2>${stateLabel(model.sessionState)}</h2>
            </div>
            <span class="status-pill">${model.backendMode}</span>
          </div>
          <form class="session-form" autocomplete="off">
            <label>
              Operator credential
              <input type="password" name="operator_credential" autocomplete="off" />
            </label>
            <div class="button-row">
              <button type="button">Start</button>
              <button type="button" disabled>Pause</button>
              <button type="button" disabled>Resume</button>
              <button type="button" class="danger" disabled>Stop</button>
            </div>
          </form>
        </article>

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

function runtimeStat(label: string, value: string): string {
  return `<div><dt>${escapeHtml(label)}</dt><dd>${escapeHtml(value)}</dd></div>`;
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
