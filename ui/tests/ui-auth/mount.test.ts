// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { mountOperatorApp } from "../../src/app";
import type { RuntimeEventClient, RuntimePreviewClient, RuntimeRunClient } from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
import { RuntimeApiClient, RuntimeApiError } from "../../src/runtimeClient";
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

describe("mounted auth/session screen", () => {
  it("refreshes active cookie-backed sessions on mount", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();

    expect(client.sessionStatus).toHaveBeenCalledTimes(1);
    expect(root.textContent).toContain("Stop");
    expect(root.textContent).toContain("session-001");
  });

  it("starts from the locked form and ignores stale refresh results", async () => {
    const refresh = deferred(activeSessionResponse("stale-session"));
    const start = deferred(startSessionResponse("session-002"));
    const client = mockClient({
      sessionStatus: vi.fn().mockReturnValue(refresh.promise),
      startSession: vi.fn().mockReturnValue(start.promise)
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    const form = root.querySelector<HTMLFormElement>("form[data-session-form='start']");
    expect(form).not.toBeNull();

    form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

    expect(root.querySelector<HTMLButtonElement>("button[type='submit']")?.disabled).toBe(true);
    await flushPromises();
    expect(client.startSession).toHaveBeenCalledTimes(1);

    refresh.resolve(activeSessionResponse("stale-session"));
    await flushPromises();
    expect(root.textContent).not.toContain("stale-session");

    start.resolve(startSessionResponse("session-002"));
    await flushPromises();
    expect(root.textContent).toContain("session-002");
  });

  it("uses the health backend mode when starting a session", async () => {
    const client = mockClient({
      health: vi.fn().mockResolvedValue(healthResponse("real")),
      startSession: vi.fn().mockResolvedValue(startSessionResponse("session-real"))
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    const form = root.querySelector<HTMLFormElement>("form[data-session-form='start']");

    form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();
    await flushPromises();

    expect(client.startSession).toHaveBeenCalledWith({
      backendMode: "real",
      requestedCapabilities: ["input", "preview", "capture", "labels", "privileged_features"]
    });
    expect(root.textContent).toContain("real");
    expect(root.textContent).toContain("session-real");
  });

  it("keeps logout pending in the active layout and prevents stale starts", async () => {
    const stop = deferred(stopSessionResponse());
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      stopSession: vi.fn().mockReturnValue(stop.promise)
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root.querySelector<HTMLButtonElement>("[data-session-action='logout']")?.click();

    expect(client.stopSession).toHaveBeenCalledTimes(1);
    expect(root.querySelector("form[data-session-form='start']")).toBeNull();
    expect(root.querySelector<HTMLButtonElement>("[data-session-action='logout']")?.disabled).toBe(
      true
    );
    expect(root.textContent).toContain("session-001");

    stop.resolve(stopSessionResponse());
    await flushPromises();
    expect(root.querySelector("form[data-session-form='start']")).not.toBeNull();
    expect(root.textContent).not.toContain("session-001");
  });

  it("keeps a stable live region and focuses auth errors for recovery", async () => {
    const client = mockClient({
      startSession: vi.fn().mockRejectedValue(
        new RuntimeApiError({
          code: "auth_rejected",
          message: "Authentication rejected.",
          retryable: false,
          details: {}
        })
      )
    });
    const root = document.createElement("div");
    document.body.appendChild(root);

    try {
      mountOperatorApp(root, config, client, null);
      const liveRegion = root.querySelector(".session-live");
      const form = root.querySelector<HTMLFormElement>("form[data-session-form='start']");

      form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      await flushPromises();

      expect(root.querySelector(".session-live")).toBe(liveRegion);
      expect(liveRegion?.textContent).toBe("authentication rejected");
      expect(document.activeElement).toBe(root.querySelector("[data-session-alert]"));
    } finally {
      root.remove();
    }
  });

  it("subscribes active sessions to runtime events and renders live run updates", async () => {
    const eventClient = mockEventClient();
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse(18))
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, eventClient.client);
    await flushPromises();

    expect(eventClient.client.eventSocket).toHaveBeenCalledTimes(1);
    eventClient.emit({
      schema_version: 1,
      type: "run_updated",
      session_id: "session-001",
      client_seq: null,
      source_id: "server",
      server_seq: 1,
      payload: {
        state: "paused",
        current_frame: 18,
        preview_stale: false,
        active_capture_job_id: null
      }
    });
    await flushPromises();

    expect(root.textContent).toContain("paused");
    expect(root.textContent).toContain("#18");
    expect(root.textContent).toContain("fresh");
  });

  it("renders current frame preview metadata for active sessions", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();

    const image = root.querySelector<HTMLImageElement>("[data-preview-image]");
    expect(client.currentFrame).toHaveBeenCalledTimes(1);
    expect(root.textContent).toContain("Preview frame21");
    expect(root.textContent).toContain("fresh");
    expect(image?.getAttribute("src")).toBe("/api/frame/current/image?frame=21");
    expect(image?.dataset.previewHash).toBe(frameCurrentResponse().preview_hash);
  });

  it("rechecks session state when the protected preview image fails", async () => {
    const client = mockClient({
      sessionStatus: vi
        .fn()
        .mockResolvedValueOnce(activeSessionResponse())
        .mockResolvedValueOnce({ schema_version: 1, active: false, state: "idle" }),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root
      .querySelector<HTMLImageElement>("[data-preview-image]")
      ?.dispatchEvent(new Event("error"));
    await flushPromises();

    expect(client.sessionStatus).toHaveBeenCalledTimes(2);
    expect(root.querySelector("form[data-session-form='start']")).not.toBeNull();
    expect(root.textContent).not.toContain("session-001");
  });
});

type MockRuntimeClient = RuntimeSessionClient &
  Partial<Pick<RuntimeApiClient, "health"> & RuntimePreviewClient & RuntimeRunClient>;

function mockClient(overrides: Partial<MockRuntimeClient> = {}): MockRuntimeClient {
  return {
    startSession: vi.fn().mockResolvedValue(startSessionResponse()),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn().mockResolvedValue(stopSessionResponse()),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
    pauseRun: vi.fn(),
    resumeRun: vi.fn(),
    triggerCapture: vi.fn(),
    captureJob: vi.fn(),
    ...overrides
  } as MockRuntimeClient;
}

function mockEventClient(): {
  client: RuntimeEventClient;
  close: ReturnType<typeof vi.fn>;
  emit: (message: RuntimeWsMessage) => void;
} {
  let onMessage: ((message: RuntimeWsMessage) => void) | undefined;
  const close = vi.fn();
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onMessage = handlers.onMessage;
        return { close } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      })
    },
    close,
    emit: (message) => onMessage?.(message)
  };
}

function frameCurrentResponse(frame = 21) {
  return {
    schema_version: 1,
    frame,
    captured_at: "1970-01-01T00:00:00Z",
    stale: false,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: `/api/frame/current/image?frame=${frame}`,
    preview_hash: "sha256:0123456789abcdef"
  };
}

function deferred<T>(defaultValue: T): {
  promise: Promise<T>;
  resolve: (value?: T) => void;
} {
  let resolvePromise: (value: T) => void = () => undefined;
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve;
  });
  return {
    promise,
    resolve: (value = defaultValue) => resolvePromise(value)
  };
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 4; index += 1) {
    await Promise.resolve();
  }
}

function activeSessionResponse(sessionId = "session-001") {
  return {
    schema_version: 1,
    active: true,
    session_id: sessionId,
    run_id: "run-001",
    state: "running",
    current_frame: 12,
    backend_mode: "synthetic"
  };
}

function healthResponse(backendMode: "synthetic" | "real" = "synthetic") {
  return {
    schema_version: 1,
    ok: true,
    service_version: "test",
    backend_mode: backendMode,
    runtime_api: 1
  };
}

function startSessionResponse(sessionId = "session-001") {
  return {
    schema_version: 1,
    session_id: sessionId,
    run_id: "run-001",
    state: "running",
    current_frame: 12,
    pad_layout: { layout_id: "console16-12btn-v1", layout_version: 1 },
    capabilities
  };
}

function runStatusResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    backend_mode: "synthetic",
    current_frame: 12,
    last_applied_input_frame: 10,
    last_preview_frame: 11,
    preview_stale: true,
    active_capture_job_id: null,
    capabilities
  };
}

function stopSessionResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    state: "stopped",
    final_frame: 20
  };
}
