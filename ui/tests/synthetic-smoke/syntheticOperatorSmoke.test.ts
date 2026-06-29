// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mountOperatorApp } from "../../src/app";
import type {
  RuntimeEventClient,
  RuntimeInputClient,
  RuntimePreviewClient,
  RuntimeRunClient
} from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
import {
  RuntimeApiClient,
  RuntimeApiError,
  type CaptureDetailResponse,
  type CaptureRecentResponse,
  type LabelsSnapshotResponse,
  type RunStatusResponse,
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

let navigatorGetGamepadsDescriptor: PropertyDescriptor | undefined;

describe("synthetic operator smoke", () => {
  beforeEach(() => {
    navigatorGetGamepadsDescriptor = Object.getOwnPropertyDescriptor(navigator, "getGamepads");
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
  });

  afterEach(() => {
    if (navigatorGetGamepadsDescriptor) {
      Object.defineProperty(navigator, "getGamepads", navigatorGetGamepadsDescriptor);
    } else {
      Reflect.deleteProperty(navigator, "getGamepads");
    }
    document.body.replaceChildren();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("walks the mounted UI through connection, input, preview, capture retry, labels, reconnect, and stop", async () => {
    const raf = installAnimationFrame();
    let currentGamepads: Array<Gamepad | null> = [standardGamepad({ pressed: [] })];
    Object.defineProperty(navigator, "getGamepads", {
      configurable: true,
      value: vi.fn(() => currentGamepads)
    });
    const socketClient = mockSocketClient();
    const triggerCapture = vi
      .fn()
      .mockRejectedValueOnce(
        new RuntimeApiError({
          code: "capture_failed",
          message: "Capture failed.",
          retryable: true,
          details: {}
        })
      )
      .mockResolvedValue(captureTriggerResponse());
    const runStatus = vi
      .fn()
      .mockResolvedValueOnce(runStatusResponse({ preview_stale: false, current_frame: 42 }))
      .mockResolvedValueOnce(runStatusResponse({ preview_stale: false, current_frame: 45 }))
      .mockResolvedValue(runStatusResponse({ preview_stale: false, current_frame: 45 }));
    const currentFrame = vi
      .fn()
      .mockResolvedValueOnce(frameCurrentResponse(42, false))
      .mockResolvedValueOnce(frameCurrentResponse(42, false))
      .mockResolvedValueOnce(frameCurrentResponse(43, true))
      .mockResolvedValueOnce(frameCurrentResponse(44, false))
      .mockResolvedValue(frameCurrentResponse(45, false));
    const updateLabels = vi.fn().mockResolvedValue({
      schema_version: 1,
      applied: false,
      label_revision: 1,
      conflicts: [
        {
          code: "label_conflict",
          message: "conflict at /home/operator/private/captures/index.jsonl",
          retryable: false,
          details: {}
        }
      ]
    });
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
      startSession: vi.fn().mockResolvedValue(startSessionResponse()),
      stopSession: vi.fn().mockResolvedValue(stopSessionResponse()),
      runStatus,
      currentFrame,
      triggerCapture,
      captureJob: vi.fn().mockResolvedValue(captureJobResponse()),
      recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
      captureDetail: vi.fn().mockResolvedValue(captureDetailResponse()),
      labelsSnapshot: vi.fn().mockResolvedValue(labelsSnapshotResponse()),
      updateLabels
    });
    const root = document.createElement("div");
    document.body.append(root);

    mountOperatorApp(root, config, client, socketClient.client);
    await flushPromises();
    root.querySelector<HTMLInputElement>("input[name='operator_credential']")!.value =
      "synthetic-operator-credential";
    root
      .querySelector<HTMLFormElement>("form[data-session-form='start']")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(client.startSession).toHaveBeenCalledWith({
      operatorCredential: "synthetic-operator-credential",
      backendMode: "synthetic",
      requestedCapabilities: ["input", "preview", "capture", "labels", "privileged_features"]
    });
    expect(root.textContent).toContain("session-001");
    expect(root.querySelector<HTMLImageElement>("[data-preview-image]")?.getAttribute("src")).toBe(
      "/api/frame/current/image?frame=42"
    );
    socketClient.emitEvent(runUpdated(1, { preview_stale: false, current_frame: 42 }));
    await flushPromises();
    expect(socketClient.client.inputSocket).toHaveBeenCalledWith(
      "session-001",
      "combined",
      expect.any(Object)
    );

    const keyboardButton = root.querySelector<HTMLButtonElement>("[data-pad-button='A']");
    expect(keyboardButton).not.toBeNull();
    keyboardButton!.focus();
    const focusedKeyboardButton = root.querySelector<HTMLButtonElement>("[data-pad-button='A']");
    expect(focusedKeyboardButton).not.toBeNull();
    expect(focusedKeyboardButton!.disabled).toBe(false);
    dispatchKey(focusedKeyboardButton!, "keydown", "Enter");
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );
    dispatchKey(window, "keyup", "Enter");
    socketClient.sendInput.mockClear();
    currentGamepads = [standardGamepad({ pressed: [1] })];
    raf.runNext();
    expect(socketClient.sendInput).toHaveBeenLastCalledWith(
      expect.objectContaining({ source: "combined", buttons: ["A"] })
    );

    socketClient.emitEvent(runUpdated(2, { preview_stale: true, current_frame: 43 }));
    await flushPromises();
    expect(root.textContent).toContain("stale");
    const staleCaptureButton = root.querySelector<HTMLButtonElement>("[data-run-action='capture']");
    expect(staleCaptureButton?.disabled).toBe(true);
    expect(
      Array.from(root.querySelectorAll<HTMLButtonElement>("[data-pad-button]")).every(
        (button) => button.disabled
      )
    ).toBe(true);
    socketClient.sendInput.mockClear();
    staleCaptureButton?.click();
    const stalePadButton = root.querySelector<HTMLButtonElement>("[data-pad-button='A']");
    expect(stalePadButton).not.toBeNull();
    dispatchKey(stalePadButton!, "keydown", "Enter");
    currentGamepads = [standardGamepad({ pressed: [1] })];
    raf.runNext();
    expect(triggerCapture).not.toHaveBeenCalled();
    expect(socketClient.sendInput).not.toHaveBeenCalled();

    socketClient.emitEvent(runUpdated(3, { preview_stale: false, current_frame: 44 }));
    await flushPromises();
    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();
    expect(root.textContent).toContain("Capture failed.");
    expect(root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.textContent).toContain(
      "Retry"
    );
    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();
    expect(triggerCapture).toHaveBeenCalledTimes(2);
    expect(client.captureJob).toHaveBeenCalledWith("job-001");
    expect(root.textContent).toContain("capture-001");

    root.querySelector<HTMLInputElement>("input[name='label_rejected']")!.checked = true;
    root
      .querySelector<HTMLFormElement>("[data-label-drawer-form='capture']")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();
    expect(updateLabels).toHaveBeenCalledWith(
      expect.objectContaining({
        sessionId: "session-001",
        updates: expect.arrayContaining([
          expect.objectContaining({ capture_id: "capture-001", role: "rejected" })
        ])
      })
    );
    expect(root.querySelector("[data-label-conflicts]")?.textContent).toContain(
      "Resolve the conflicting label role"
    );
    expect(root.textContent ?? "").not.toMatch(/\/home\/|private\/captures|index\.jsonl/i);

    const runStatusCallsBeforeReconnect = runStatus.mock.calls.length;
    const currentFrameCallsBeforeReconnect = currentFrame.mock.calls.length;
    socketClient.triggerEventReconnect();
    expect(root.textContent).toContain("WebSocket reconnect");
    await flushPromises();
    expect(runStatus).toHaveBeenCalledTimes(runStatusCallsBeforeReconnect + 1);
    expect(currentFrame).toHaveBeenCalledTimes(currentFrameCallsBeforeReconnect + 1);
    expect(root.querySelector<HTMLImageElement>("[data-preview-image]")?.getAttribute("src")).toBe(
      "/api/frame/current/image?frame=45"
    );
    expect(root.textContent).not.toContain("WebSocket reconnect");
    expect(root.textContent).toContain("fresh");

    root.querySelector<HTMLButtonElement>("[data-session-action='logout']")?.click();
    await flushPromises();
    expect(client.stopSession).toHaveBeenCalledWith("session-001");
    expect(root.querySelector("form[data-session-form='start']")).not.toBeNull();
    expect(root.textContent ?? "").not.toMatch(/synthetic-operator-credential|private\.env/i);
  });

  it("redacts failed synthetic auth during the smoke startup path", async () => {
    const client = mockClient({
      startSession: vi.fn().mockRejectedValue(
        new RuntimeApiError({
          code: "auth_rejected",
          message: "bad credential at /home/operator/private.env with synthetic-operator-credential",
          retryable: false,
          details: {}
        })
      )
    });
    const root = document.createElement("div");
    document.body.append(root);

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root.querySelector<HTMLInputElement>("input[name='operator_credential']")!.value =
      "synthetic-operator-credential";
    root
      .querySelector<HTMLFormElement>("form[data-session-form='start']")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(root.querySelector("[data-session-alert]")?.textContent).toContain("Request failed.");
    expect(root.textContent ?? "").not.toMatch(
      /\/home\/|private\.env|synthetic-operator-credential/i
    );
  });
});

type MockRuntimeClient = RuntimeSessionClient &
  Partial<
    RuntimePreviewClient &
      RuntimeRunClient &
      Pick<RuntimeApiClient, "recentCaptures" | "captureDetail" | "labelsSnapshot" | "updateLabels">
  >;

function mockClient(overrides: Partial<MockRuntimeClient> = {}): MockRuntimeClient {
  return {
    startSession: vi.fn().mockResolvedValue(startSessionResponse()),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn().mockResolvedValue(stopSessionResponse()),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
    pauseRun: vi.fn(),
    resumeRun: vi.fn(),
    currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
    triggerCapture: vi.fn().mockResolvedValue(captureTriggerResponse()),
    captureJob: vi.fn().mockResolvedValue(captureJobResponse()),
    recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
    captureDetail: vi.fn().mockResolvedValue(captureDetailResponse()),
    labelsSnapshot: vi.fn().mockResolvedValue(labelsSnapshotResponse()),
    updateLabels: vi.fn(),
    ...overrides
  } as MockRuntimeClient;
}

function mockSocketClient(): {
  client: RuntimeEventClient & RuntimeInputClient;
  sendInput: ReturnType<typeof vi.fn>;
  emitEvent: (message: RuntimeWsMessage) => void;
  triggerEventReconnect: () => void;
} {
  let onEventMessage: ((message: RuntimeWsMessage) => void) | undefined;
  let onEventReconnect: (() => void) | undefined;
  const sendInput = vi.fn();
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onEventMessage = handlers.onMessage;
        onEventReconnect = handlers.onReconnect;
        return { close: vi.fn() } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      }),
      inputSocket: vi.fn(() => {
        return {
          close: vi.fn(),
          sendInput
        } as unknown as ReturnType<RuntimeInputClient["inputSocket"]>;
      })
    },
    sendInput,
    emitEvent: (message) => onEventMessage?.(message),
    triggerEventReconnect: () => onEventReconnect?.()
  };
}

function startSessionResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 42,
    pad_layout: { layout_id: "console16-12btn-v1", layout_version: 1 },
    capabilities
  };
}

function stopSessionResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    state: "stopped",
    final_frame: 45
  };
}

function runStatusResponse(overrides: Partial<RunStatusResponse> = {}): RunStatusResponse {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    backend_mode: "synthetic",
    current_frame: 42,
    last_applied_input_frame: 40,
    last_preview_frame: 42,
    preview_stale: false,
    active_capture_job_id: null,
    capabilities,
    ...overrides
  };
}

function frameCurrentResponse(frame = 42, stale = false) {
  return {
    schema_version: 1,
    frame,
    captured_at: "1970-01-01T00:00:00Z",
    stale,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: `/api/frame/current/image?frame=${frame}`,
    preview_hash: "sha256:synthetic-smoke"
  };
}

function captureTriggerResponse() {
  return {
    schema_version: 1,
    job_id: "job-001",
    status: "requested",
    requested_frame: 42,
    scheduled_frame: 43
  };
}

function captureJobResponse() {
  return {
    ...captureTriggerResponse(),
    status: "completed",
    captured_frame: 43,
    capture_id: "capture-001",
    labelable: true,
    has_preview: true,
    error: null
  };
}

function captureRecentResponse(overrides: Partial<CaptureRecentResponse> = {}): CaptureRecentResponse {
  return {
    schema_version: 1,
    captures: [
      {
        capture_id: "capture-001",
        frame: 43,
        status: "completed",
        labelable: true,
        has_preview: true,
        labels: ["goal_positive"],
        created_at: "2026-06-24T10:00:00Z"
      }
    ],
    next_cursor: null,
    ...overrides
  };
}

function captureDetailResponse(overrides: Partial<CaptureDetailResponse> = {}): CaptureDetailResponse {
  return {
    schema_version: 1,
    capture_id: "capture-001",
    frame: 43,
    status: "completed",
    labelable: true,
    has_preview: true,
    preview_image_url: "/api/capture/capture-001/preview",
    privileged_features_available: false,
    labels: ["goal_positive"],
    sanitized_provenance: {
      capture_source: "synthetic",
      layout_hash: "sha256:layout-public",
      capture_spec_hash: "sha256:capture-spec-public",
      map_hash: "sha256:map-public"
    },
    ...overrides
  };
}

function labelsSnapshotResponse(overrides: Partial<LabelsSnapshotResponse> = {}): LabelsSnapshotResponse {
  return {
    schema_version: 1,
    label_revision: 1,
    target_labels: {
      first_boss: null,
      goal_positive: "capture-001",
      goal_negative: null
    },
    status_labels: [],
    dedup_groups: [],
    ...overrides
  };
}

function runUpdated(
  serverSeq: number,
  overrides: Partial<Extract<RuntimeWsMessage, { type: "run_updated" }>["payload"]> = {}
): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "run_updated",
    session_id: "session-001",
    client_seq: null,
    source_id: "server",
    server_seq: serverSeq,
    payload: {
      state: "running",
      current_frame: 42,
      preview_stale: false,
      active_capture_job_id: null,
      ...overrides
    }
  };
}

function dispatchKey(target: EventTarget, type: "keydown" | "keyup", code: string): KeyboardEvent {
  const event = new KeyboardEvent(type, {
    bubbles: true,
    cancelable: true,
    code,
    key: keyForCode(code)
  });
  target.dispatchEvent(event);
  return event;
}

function keyForCode(code: string): string {
  if (code === "Enter") {
    return "Enter";
  }
  if (code.startsWith("Arrow")) {
    return code;
  }
  return "";
}

function installAnimationFrame(): { runNext: () => void } {
  let nextId = 1;
  const callbacks = new Map<number, FrameRequestCallback>();
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn((callback: FrameRequestCallback) => {
      const id = nextId;
      nextId += 1;
      callbacks.set(id, callback);
      return id;
    })
  );
  vi.stubGlobal(
    "cancelAnimationFrame",
    vi.fn((id: number) => {
      callbacks.delete(id);
    })
  );
  return {
    runNext: () => {
      const [id, callback] = callbacks.entries().next().value ?? [];
      if (id !== undefined && callback) {
        callbacks.delete(id);
        callback(performance.now());
      }
    }
  };
}

function standardGamepad(input: { pressed?: number[] }): Gamepad {
  const pressedButtons = new Set(input.pressed ?? []);
  return {
    id: "standard-gamepad",
    index: 0,
    connected: true,
    mapping: "standard",
    timestamp: performance.now(),
    buttons: Array.from({ length: 16 }, (_, index) => ({
      pressed: pressedButtons.has(index),
      touched: pressedButtons.has(index),
      value: pressedButtons.has(index) ? 1 : 0
    })),
    axes: [0, 0, 0, 0],
    vibrationActuator: null
  } as unknown as Gamepad;
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 10; index += 1) {
    await Promise.resolve();
  }
}
