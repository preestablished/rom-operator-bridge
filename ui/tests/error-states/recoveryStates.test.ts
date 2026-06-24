// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import {
  mountOperatorApp,
  renderOperatorApp,
  type RuntimeEventClient,
  type RuntimeInputClient,
  type RuntimePreviewClient,
  type RuntimeRunClient
} from "../../src/app";
import type { AuthSessionState, RuntimeSessionClient } from "../../src/authSession";
import {
  RuntimeApiError,
  type CaptureJobResponse,
  type RunStatusResponse,
  type RuntimeErrorCode,
  type RuntimeWsMessage
} from "../../src/runtimeClient";
import type { RuntimeConfig } from "../../src/runtimeConfig";

const config: RuntimeConfig = {
  schema_version: 1,
  api_base_path: "/api",
  ws_base_path: "/ws",
  allow_persistence: false
};

const capabilities = {
  input: true,
  preview: true,
  capture: true,
  labels: true,
  privileged_features: false,
  validation_runner: false
};

const unsafeText =
  "private failure at /home/operator/private.env with raw command output, feature bytes, validation report, and screenshot";

describe("browser-safe recovery states", () => {
  it("renders locked-session recovery states without leaking private details", () => {
    const states: Array<readonly [AuthSessionState["status"], RuntimeErrorCode, string]> = [
      ["auth_rejected", "auth_rejected", "Authentication rejected"],
      ["origin_rejected", "origin_rejected", "Origin rejected"],
      ["session_active_elsewhere", "session_active_elsewhere", "Session active elsewhere"],
      ["faulted", "backend_unavailable", "Bridge unavailable"]
    ];

    for (const [status, code, title] of states) {
      const root = render(authState(status, code, unsafeText));

      expect(recovery(root, code === "backend_unavailable" ? "bridge_unavailable" : code)?.textContent).toContain(
        title
      );
      expectSafe(root);
    }
  });

  it("renders active recovery states for backend, frame, capture, labels, and validation", () => {
    const backend = render(activeState(errorDisplay("backend_unavailable", unsafeText)));
    expect(recovery(backend, "backend_unavailable")?.textContent).toContain("Backend unavailable");
    expect(backend.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(true);
    expectSafe(backend);

    const frameStale = render(activeState(null, { preview_stale: true }));
    expect(recovery(frameStale, "frame_stale")?.textContent).toContain("Framebuffer stale");
    expect(frameStale.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(true);

    const captureProgress = render(activeState(null, { active_capture_job_id: "job-001" }), {
      capturePending: true
    });
    expect(recovery(captureProgress, "capture_in_progress")?.textContent).toContain("Capture in progress");

    const captureFailed = render(activeState(), {
      captureError: unsafeText,
      captureErrorCode: "capture_failed"
    });
    expect(recovery(captureFailed, "capture_failed")?.textContent).toContain("Capture failed");
    expectSafe(captureFailed);

    const captureFailedWithPath = render(activeState(), {
      captureError: "layout mismatch at /var/tmp/operator-report.txt",
      captureErrorCode: "capture_failed"
    });
    expect(recovery(captureFailedWithPath, "capture_failed")?.textContent).toContain(
      "Keep the failed capture visible"
    );
    expect(captureFailedWithPath.textContent ?? "").not.toContain("/var/tmp/operator-report.txt");

    const labelConflict = render(activeState(errorDisplay("label_conflict", "first_boss conflicts with rejected")));
    expect(recovery(labelConflict, "label_conflict")?.textContent).toContain(
      "first_boss conflicts with rejected"
    );
    expect(labelConflict.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(false);

    const validationFailed = render(activeState(), {
      validationStatus: {
        status: "failed",
        command_class: "phase4_score_plan",
        started_at: "2026-06-24T09:00:00Z",
        completed_at: "2026-06-24T09:00:03Z",
        summary: "Validation failed.",
        issue_summaries: [unsafeText, "Goal route mismatch."]
      }
    });
    expect(recovery(validationFailed, "validation_failed")?.textContent).toContain(
      "private server-side report"
    );
    expect(validationFailed.textContent ?? "").toContain("phase4_score_plan");
    expect(validationFailed.textContent ?? "").toContain("Validation failed.");
    expect(validationFailed.textContent ?? "").toContain("Goal route mismatch.");
    expectSafe(validationFailed);

    const validationPassed = render(activeState(), {
      validationStatus: {
        status: "passed",
        command_class: "redaction_scan",
        started_at: "2026-06-24T09:05:00Z",
        completed_at: "2026-06-24T09:05:01Z",
        summary: "Validation passed.",
        issue_summaries: []
      }
    });
    expect(validationPassed.textContent ?? "").toContain("redaction_scan");
    expect(validationPassed.textContent ?? "").toContain("Validation passed.");
    expect(recovery(validationPassed, "validation_failed")).toBeNull();
    expectSafe(validationPassed);
  });

  it("renders gamepad disconnect and WebSocket reconnect recovery without disabling keyboard input", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const socketClient = mockSocketClient();
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false))
    });
    const root = document.createElement("div");
    document.body.append(root);

    try {
      mountOperatorApp(root, config, client, socketClient.client);
      await flushPromises();

      window.dispatchEvent(new Event("gamepaddisconnected"));
      expect(recovery(root, "gamepad_disconnected")?.textContent).toContain("keyboard input remains available");
      expect(root.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(false);

      socketClient.triggerReconnect();
      expect(recovery(root, "websocket_reconnect")?.textContent).toContain("Input was cleared");
      expectSafe(root);

      await flushPromises();
      expect(recovery(root, "websocket_reconnect")).toBeNull();
    } finally {
      root.remove();
      vi.restoreAllMocks();
    }
  });

  it("turns unavailable bridge and active backend failures into sanitized recovery states", async () => {
    const bridgeRoot = document.createElement("div");
    mountOperatorApp(
      bridgeRoot,
      config,
      mockClient({ sessionStatus: vi.fn().mockRejectedValue(new Error(unsafeText)) }),
      null
    );
    await flushPromises();
    expect(recovery(bridgeRoot, "bridge_unavailable")?.textContent).toContain("Bridge unavailable");
    expectSafe(bridgeRoot);

    const activeRoot = document.createElement("div");
    mountOperatorApp(
      activeRoot,
      config,
      mockClient({
        sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
        runStatus: vi.fn().mockRejectedValue(new Error(unsafeText)),
        currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false))
      }),
      null
    );
    await flushPromises();
    expect(recovery(activeRoot, "backend_unavailable")?.textContent).toContain("Backend unavailable");
    expectSafe(activeRoot);

    const currentFrame = vi
      .fn()
      .mockRejectedValueOnce(new Error(unsafeText))
      .mockResolvedValueOnce(frameCurrentResponse(13, false));
    const previewRoot = document.createElement("div");
    const previewSocket = mockSocketClient();
    mountOperatorApp(
      previewRoot,
      config,
      mockClient({
        sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
        runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
        currentFrame
      }),
      previewSocket.client
    );
    await flushPromises();
    expect(recovery(previewRoot, "backend_unavailable")?.textContent).toContain("Backend unavailable");
    expect(previewRoot.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(true);
    expectSafe(previewRoot);

    previewSocket.emitEvent(runUpdated());
    await flushPromises();
    expect(recovery(previewRoot, "backend_unavailable")).toBeNull();
    expect(previewRoot.querySelector<HTMLButtonElement>("[data-pad-button='A']")?.disabled).toBe(false);
  });

  it("renders capture in-progress and capture failed API recovery without leaking raw details", async () => {
    const inProgressRoot = document.createElement("div");
    mountOperatorApp(
      inProgressRoot,
      config,
      mockClient({
        sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
        runStatus: vi
          .fn()
          .mockResolvedValue(runStatusResponse({ preview_stale: false, active_capture_job_id: "job-001" })),
        currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false)),
        triggerCapture: vi.fn().mockRejectedValue(runtimeApiError("capture_in_progress", "Capture already running.")),
        captureJob: vi.fn().mockResolvedValue(captureJobResponse())
      }),
      null
    );
    await flushPromises();
    inProgressRoot.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();
    expect(recovery(inProgressRoot, "capture_in_progress")?.textContent).toContain("Capture in progress");
    expect(inProgressRoot.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.disabled).toBe(true);

    const failedRoot = document.createElement("div");
    mountOperatorApp(
      failedRoot,
      config,
      mockClient({
        sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
        runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
        currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false)),
        triggerCapture: vi.fn().mockRejectedValue(runtimeApiError("capture_failed", unsafeText))
      }),
      null
    );
    await flushPromises();
    failedRoot.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();
    expect(recovery(failedRoot, "capture_failed")?.textContent).toContain("Capture failed");
    expectSafe(failedRoot);
  });

  it("blocks input after session-inactive input rejection", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const socketClient = mockSocketClient();
    const root = document.createElement("div");
    document.body.append(root);

    try {
      mountOperatorApp(
        root,
        config,
        mockClient({
          sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
          runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
          currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false))
        }),
        socketClient.client
      );
      await flushPromises();

      socketClient.emitInput(inputReject("session_inactive", "Session is no longer active."));
      await flushPromises();

      expect(recovery(root, "session_inactive")?.textContent).toContain("Session is no longer active");
      const button = root.querySelector<HTMLButtonElement>("[data-pad-button='A']");
      expect(button?.disabled).toBe(true);
      const sendsAfterReject = socketClient.sendInput.mock.calls.length;
      button?.click();
      expect(socketClient.sendInput).toHaveBeenCalledTimes(sendsAfterReject);

      const requestAnimationFrame = vi.fn();
      const getGamepads = vi.fn();
      Object.defineProperty(globalThis, "requestAnimationFrame", {
        configurable: true,
        value: requestAnimationFrame
      });
      Object.defineProperty(globalThis, "cancelAnimationFrame", {
        configurable: true,
        value: vi.fn()
      });
      Object.defineProperty(navigator, "getGamepads", {
        configurable: true,
        value: getGamepads
      });
      root.querySelector<HTMLElement>("[data-input-focus-surface]")?.focus();
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("gamepadconnected"));
      expect(requestAnimationFrame).not.toHaveBeenCalled();
      expect(getGamepads).not.toHaveBeenCalled();
    } finally {
      root.remove();
      vi.restoreAllMocks();
    }
  });
});

type MockRuntimeClient = RuntimeSessionClient & Partial<RuntimePreviewClient & RuntimeRunClient>;

function render(
  auth: AuthSessionState,
  runtimeView: Parameters<typeof renderOperatorApp>[2] = {}
): HTMLElement {
  const root = document.createElement("div");
  root.innerHTML = renderOperatorApp(config, auth, runtimeView);
  return root;
}

function recovery(root: ParentNode, code: string): HTMLElement | null {
  return root.querySelector<HTMLElement>(`[data-recovery-code="${code}"]`);
}

function expectSafe(root: ParentNode): void {
  expect(root.textContent ?? "").not.toMatch(
    /\/home\/|private\.env|raw command|command output|feature bytes|validation report|screenshot|operator-secret/i
  );
}

function authState(
  status: AuthSessionState["status"],
  code: RuntimeErrorCode,
  message: string
): AuthSessionState {
  return {
    status,
    session: inactiveSession(),
    error: errorDisplay(code, message)
  };
}

function activeState(
  error = null as ReturnType<typeof errorDisplay> | null,
  overrides: Partial<AuthSessionState["session"]> = {}
): AuthSessionState {
  return {
    status: "active",
    error,
    session: {
      ...inactiveSession(),
      active: true,
      session_id: "session-001",
      run_id: "run-001",
      state: "running",
      current_frame: 12,
      last_applied_input_frame: 10,
      last_preview_frame: 11,
      preview_stale: false,
      capabilities,
      ...overrides
    }
  };
}

function inactiveSession(): AuthSessionState["session"] {
  return {
    active: false,
    session_id: null,
    run_id: null,
    state: "idle",
    backend_mode: "synthetic",
    current_frame: 0,
    last_applied_input_frame: 0,
    last_preview_frame: 0,
    preview_stale: true,
    active_capture_job_id: null,
    capabilities: null
  };
}

function errorDisplay(code: RuntimeErrorCode, message: string) {
  return { code, message, retryable: true, details: {} };
}

function runtimeApiError(code: RuntimeErrorCode, message: string): RuntimeApiError {
  return new RuntimeApiError(errorDisplay(code, message));
}

function mockClient(overrides: Partial<MockRuntimeClient> = {}): MockRuntimeClient {
  return {
    startSession: vi.fn(),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn(),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
    pauseRun: vi.fn(),
    resumeRun: vi.fn(),
    currentFrame: vi.fn(),
    triggerCapture: vi.fn(),
    captureJob: vi.fn(),
    ...overrides
  } as MockRuntimeClient;
}

function mockSocketClient(): {
  client: RuntimeEventClient & RuntimeInputClient;
  sendInput: ReturnType<typeof vi.fn>;
  triggerReconnect: () => void;
  emitEvent: (message: RuntimeWsMessage) => void;
  emitInput: (message: RuntimeWsMessage) => void;
} {
  let onEventMessage: ((message: RuntimeWsMessage) => void) | undefined;
  let onInputMessage: ((message: RuntimeWsMessage) => void) | undefined;
  let onInputReconnect: (() => void) | undefined;
  const sendInput = vi.fn();
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onEventMessage = handlers.onMessage;
        return { close: vi.fn() } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      }),
      inputSocket: vi.fn((_, __, handlers = {}) => {
        onInputMessage = handlers.onMessage;
        onInputReconnect = handlers.onReconnect;
        return {
          close: vi.fn(),
          sendInput
        } as unknown as ReturnType<RuntimeInputClient["inputSocket"]>;
      })
    },
    sendInput,
    triggerReconnect: () => onInputReconnect?.(),
    emitEvent: (message) => onEventMessage?.(message),
    emitInput: (message) => onInputMessage?.(message)
  };
}

function activeSessionResponse() {
  return {
    schema_version: 1,
    active: true,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 12,
    backend_mode: "synthetic"
  };
}

function runStatusResponse(overrides: Partial<RunStatusResponse> = {}): RunStatusResponse {
  return { ...baseRunStatusResponse(), ...overrides };
}

function baseRunStatusResponse(): RunStatusResponse {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    backend_mode: "synthetic",
    current_frame: 12,
    last_applied_input_frame: 10,
    last_preview_frame: 11,
    preview_stale: false,
    active_capture_job_id: null,
    capabilities
  };
}

function frameCurrentResponse(frame = 21, stale = false) {
  return {
    schema_version: 1,
    frame,
    captured_at: "1970-01-01T00:00:00Z",
    stale,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: `/api/frame/current/image?frame=${frame}`,
    preview_hash: "sha256:0123456789abcdef"
  };
}

function captureJobResponse(overrides: Partial<CaptureJobResponse> = {}): CaptureJobResponse {
  return {
    schema_version: 1,
    job_id: "job-001",
    status: "capturing",
    requested_frame: 12,
    scheduled_frame: 13,
    captured_frame: null,
    capture_id: null,
    labelable: false,
    has_preview: false,
    error: null,
    ...overrides
  };
}

function runUpdated(overrides: Partial<Extract<RuntimeWsMessage, { type: "run_updated" }>["payload"]> = {}): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "run_updated",
    session_id: "session-001",
    client_seq: null,
    source_id: "server",
    server_seq: 1,
    payload: {
      state: "running",
      current_frame: 13,
      preview_stale: false,
      active_capture_job_id: null,
      ...overrides
    }
  };
}

function inputReject(code: RuntimeErrorCode, message: string): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "input_reject",
    session_id: "session-001",
    client_seq: 1,
    source_id: "combined",
    server_seq: null,
    payload: {
      schema_version: 1,
      error: errorDisplay(code, message)
    }
  };
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
  }
}
