// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { mountOperatorApp } from "../../src/app";
import type { RuntimeEventClient, RuntimePreviewClient, RuntimeRunClient } from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
import type { RuntimeWsMessage } from "../../src/runtimeClient";
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

describe("session and play surface", () => {
  it("pauses and resumes active synthetic sessions from the play surface", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false)),
      runStatus: vi
        .fn()
        .mockResolvedValueOnce(runStatusResponse({ preview_stale: false }))
        .mockResolvedValueOnce(runStatusResponse({ state: "paused", current_frame: 13 }))
        .mockResolvedValueOnce(runStatusResponse({ state: "running", current_frame: 14 })),
      pauseRun: vi.fn().mockResolvedValue({ schema_version: 1, state: "paused", current_frame: 13 }),
      resumeRun: vi.fn().mockResolvedValue({ schema_version: 1, state: "running", current_frame: 14 })
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root.querySelector<HTMLButtonElement>("[data-run-action='pause']")?.click();
    await flushPromises();
    root.querySelector<HTMLButtonElement>("[data-run-action='resume']")?.click();
    await flushPromises();

    expect(client.pauseRun).toHaveBeenCalledWith("session-001");
    expect(client.resumeRun).toHaveBeenCalledWith("session-001");
    expect(root.textContent).toContain("running");
    expect(root.textContent).toContain("#14");
  });

  it("disables input and capture controls when the preview is stale", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: true })),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(21, true))
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();

    expect(root.textContent).toContain("stale");
    expect(root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.disabled).toBe(
      true
    );
    expect(
      Array.from(root.querySelectorAll<HTMLButtonElement>("[data-pad-button]")).every(
        (button) => button.disabled
      )
    ).toBe(true);
    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    expect(client.triggerCapture).not.toHaveBeenCalled();
  });

  it("triggers capture with the observed preview frame and renders sanitized job status", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(42, false)),
      triggerCapture: vi.fn().mockResolvedValue(captureTriggerResponse()),
      captureJob: vi.fn().mockResolvedValue(captureJobResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();

    expect(client.triggerCapture).toHaveBeenCalledWith({
      sessionId: "session-001",
      idempotencyKey: expect.stringMatching(/^[0-9a-f-]{36}$/i),
      observedPreviewFrame: 42
    });
    expect(client.captureJob).toHaveBeenCalledWith("job-001");
    expect(root.textContent).toContain("completed");
    expect(root.textContent).toContain("capture-001");
    expect(root.textContent).not.toMatch(/\/home\/|operator-secret|raw payload|private\.env/i);
  });

  it("renders current pressed buttons and an in-memory padlog tail from input acknowledgements", async () => {
    const eventClient = mockEventClient();
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse({ preview_stale: false })),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(12, false))
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, eventClient.client);
    await flushPromises();
    eventClient.emit(inputAck(1, 1025));
    await flushPromises();

    expect(root.textContent).toContain("Pressed: A, Start");
    expect(root.textContent).toContain("#30 applied");
    expect(root.textContent).toContain("0401");
    expect(root.textContent).not.toMatch(/localStorage|sessionStorage|indexedDB/i);
  });
});

type MockRuntimeClient = RuntimeSessionClient & Partial<RuntimePreviewClient & RuntimeRunClient>;

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

function mockEventClient(): {
  client: RuntimeEventClient;
  emit: (message: RuntimeWsMessage) => void;
} {
  let onMessage: ((message: RuntimeWsMessage) => void) | undefined;
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onMessage = handlers.onMessage;
        return { close: vi.fn() } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      })
    },
    emit: (message) => onMessage?.(message)
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

function runStatusResponse(overrides: Partial<ReturnType<typeof baseRunStatusResponse>> = {}) {
  return { ...baseRunStatusResponse(), ...overrides };
}

function baseRunStatusResponse() {
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

function inputAck(serverSeq: number, padWord: number): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "input_ack",
    session_id: "session-001",
    client_seq: 1,
    source_id: "keyboard",
    server_seq: serverSeq,
    payload: {
      client_event_id: "00000000-0000-0000-0000-000000000001",
      assigned_frame: 30,
      pad_word: padWord,
      status: "applied"
    }
  };
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
  }
}
