// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { mountOperatorApp } from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
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

    mountOperatorApp(root, config, client);
    await flushPromises();

    expect(client.sessionStatus).toHaveBeenCalledTimes(1);
    expect(root.textContent).toContain("Logout");
    expect(root.textContent).toContain("session-001");
  });

  it("clears submitted form credentials and ignores stale refresh results", async () => {
    const refresh = deferred(activeSessionResponse("stale-session"));
    const start = deferred(startSessionResponse("session-002"));
    const client = mockClient({
      sessionStatus: vi.fn().mockReturnValue(refresh.promise),
      startSession: vi.fn().mockReturnValue(start.promise)
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client);
    const input = root.querySelector<HTMLInputElement>("input[name='operator_credential']");
    const form = root.querySelector<HTMLFormElement>("form[data-session-form='start']");
    expect(input).not.toBeNull();
    expect(form).not.toBeNull();

    input!.value = "operator-secret";
    form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));

    expect(client.startSession).toHaveBeenCalledTimes(1);
    expect(root.querySelector<HTMLInputElement>("input[name='operator_credential']")?.value).toBe("");
    expect(root.querySelector<HTMLButtonElement>("button[type='submit']")?.disabled).toBe(true);

    refresh.resolve(activeSessionResponse("stale-session"));
    await flushPromises();
    expect(root.textContent).not.toContain("stale-session");

    start.resolve(startSessionResponse("session-002"));
    await flushPromises();
    expect(root.textContent).toContain("session-002");
    expect(root.textContent).not.toContain("operator-secret");
  });

  it("keeps logout pending in the active layout and prevents stale starts", async () => {
    const stop = deferred(stopSessionResponse());
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      stopSession: vi.fn().mockReturnValue(stop.promise)
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client);
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
});

function mockClient(overrides: Partial<RuntimeSessionClient> = {}): RuntimeSessionClient {
  return {
    startSession: vi.fn().mockResolvedValue(startSessionResponse()),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn().mockResolvedValue(stopSessionResponse()),
    ...overrides
  } as RuntimeSessionClient;
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
  await Promise.resolve();
  await Promise.resolve();
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

function stopSessionResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    state: "stopped",
    final_frame: 20
  };
}
