import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { renderOperatorApp } from "../../src/app";
import {
  initialAuthSessionState,
  logoutSession,
  refreshSession,
  startOperatorSession,
  type AuthSessionState
} from "../../src/authSession";
import { RuntimeApiClient } from "../../src/runtimeClient";
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

describe("UI auth and session flow", () => {
  it("starts a session without credential material in URLs, bodies, or rendered HTML", async () => {
    const fetcher = queuedFetch([startSessionResponse()]);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await startOperatorSession(initialAuthSessionState(), client);
    const html = renderOperatorApp(config, state);

    expect(state.status).toBe("active");
    expect(state.session).toMatchObject({
      active: true,
      session_id: "session-001",
      run_id: "run-001",
      state: "running"
    });
    expect(fetcher.calls[0]?.url).toBe("/api/session/start");
    expect(bodyAt(fetcher, 0)).toEqual({
      schema_version: 1,
      backend_mode: "synthetic",
      requested_capabilities: ["input", "preview", "capture", "labels", "privileged_features"]
    });
    expect(html).toContain("Stop");
    expect(Object.hasOwn(bodyAt(fetcher, 0) as object, ["operator", "credential"].join("_"))).toBe(false);
  });

  it("starts real backend sessions when the mount supplies a real backend mode", async () => {
    const fetcher = queuedFetch([startSessionResponse()]);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await startOperatorSession(initialAuthSessionState(), client, "real");

    expect(state.status).toBe("active");
    expect(state.session.backend_mode).toBe("real");
    expect(bodyAt(fetcher, 0)).toMatchObject({
      schema_version: 1,
      backend_mode: "real"
    });
  });

  it("renders auth_rejected as a sanitized locked-screen alert", async () => {
    const fetcher = queuedFetch([errorEnvelope("auth_rejected", "Authentication rejected.")], 401);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await startOperatorSession(initialAuthSessionState(), client);
    const html = renderOperatorApp(config, state);

    expect(state.status).toBe("auth_rejected");
    expect(state.error).toMatchObject({
      code: "auth_rejected",
      message: "Authentication rejected.",
      details: {}
    });
    expect(html).toContain('role="alert"');
    expect(html).toContain("Authentication rejected.");
  });

  it("maps expired sessions to the expired screen state", async () => {
    const fetcher = queuedFetch([errorEnvelope("session_inactive", "Session inactive.")], 401);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await refreshSession(activeState(), client);

    expect(state.status).toBe("expired");
    expect(renderOperatorApp(config, state)).toContain("Session inactive.");
  });

  it("keeps a fresh visit locked when the session endpoint reports inactive", async () => {
    const fetcher = queuedFetch([errorEnvelope("session_inactive", "Session inactive.")], 401);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await refreshSession(initialAuthSessionState(), client);

    expect(state.status).toBe("locked");
    expect(state.error).toBeNull();
    expect(renderOperatorApp(config, state)).not.toContain("Session inactive.");
  });

  it("logs out with the active session id and returns to the locked state", async () => {
    const fetcher = queuedFetch([stopSessionResponse()]);
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await logoutSession(activeState(), client);

    expect(state.status).toBe("locked");
    expect(state.session.active).toBe(false);
    expect(fetcher.calls[0]?.url).toBe("/api/session/stop");
    expect(bodyAt(fetcher, 0)).toEqual({
      schema_version: 1,
      session_id: "session-001",
      reason: "operator_stop"
    });
  });

  it("surfaces session_active_elsewhere without persisting credentials", async () => {
    const fetcher = queuedFetch(
      [errorEnvelope("session_active_elsewhere", "Session active elsewhere.")],
      409
    );
    const client = new RuntimeApiClient(config, { fetcher });

    const state = await startOperatorSession(initialAuthSessionState(), client);
    const html = renderOperatorApp(config, state);

    expect(state.status).toBe("session_active_elsewhere");
    expect(html).toContain("Session active elsewhere.");
  });

  it("does not use browser persistence APIs in auth/session source", () => {
    const sourceRoot = new URL("../../src", import.meta.url).pathname;
    const authSource = [
      readFileSync(join(sourceRoot, "app.ts"), "utf8"),
      readFileSync(join(sourceRoot, "authSession.ts"), "utf8"),
      readFileSync(join(sourceRoot, "main.ts"), "utf8")
    ].join("\n");

    expect(authSource).not.toMatch(/localStorage|sessionStorage|indexedDB|serviceWorker|caches/i);
  });
});

type FetchCall = { url: string; init: RequestInit };

function queuedFetch(payloads: unknown[], status = 200): ((input: RequestInfo | URL, init?: RequestInit) => Promise<Response>) & {
  calls: FetchCall[];
} {
  const calls: FetchCall[] = [];
  const fetcher = async (input: RequestInfo | URL, init: RequestInit = {}) => {
    calls.push({ url: String(input), init });
    const payload = payloads.shift();
    return new Response(JSON.stringify(payload), {
      status,
      headers: { "content-type": "application/json" }
    });
  };
  return Object.assign(fetcher, { calls });
}

function bodyAt(fetcher: { calls: FetchCall[] }, index: number): unknown {
  return JSON.parse(String(fetcher.calls[index]?.init.body));
}

function startSessionResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 1,
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

function errorEnvelope(code: string, message: string) {
  return {
    schema_version: 1,
    error: {
      code,
      message,
      retryable: false,
      details: {}
    }
  };
}

function activeState(): AuthSessionState {
  return {
    status: "active",
    error: null,
    session: {
      active: true,
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
    }
  };
}
