import { beforeEach, describe, expect, it } from "vitest";
import {
  RuntimeApiClient,
  RuntimeApiError,
  RuntimeWebSocketClient,
  initialRuntimeSessionModel,
  modelFromStartSession,
  applyRunStatus,
  type RuntimeWsMessage
} from "../src/runtimeClient";
import type { RuntimeConfig } from "../src/runtimeConfig";

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

describe("runtime API client", () => {
  it("covers typed session, run, frame, capture, and labels requests without URL credentials", async () => {
    const fetcher = queuedFetch([
      startSessionResponse(),
      runStatusResponse(),
      sessionResponse(),
      stopSessionResponse(),
      runStateResponse("paused"),
      runStateResponse("running"),
      frameCurrentResponse(),
      captureTriggerResponse(),
      captureJobResponse(),
      captureRecentResponse(),
      captureDetailResponse(),
      labelsResponse(),
      labelsSnapshotResponse()
    ]);
    const client = new RuntimeApiClient(config, { fetcher });

    const started = await client.startSession({
      operatorCredential: "operator-secret",
      requestedCapabilities: ["input", "preview"]
    });
    const model = applyRunStatus(modelFromStartSession(started), await client.runStatus());

    await client.sessionStatus();
    await client.stopSession("session-001");
    await client.pauseRun("session-001");
    await client.resumeRun("session-001");
    await client.currentFrame();
    await client.triggerCapture({
      sessionId: "session-001",
      idempotencyKey: "00000000-0000-0000-0000-000000000001",
      observedPreviewFrame: 42
    });
    await client.captureJob("job-001");
    await client.recentCaptures("cursor-001");
    await client.captureDetail("capture-001");
    await client.updateLabels({
      sessionId: "session-001",
      idempotencyKey: "00000000-0000-0000-0000-000000000002",
      updates: [{ op: "upsert", capture_id: "capture-001", role: "first_boss" }]
    });
    const labelSnapshot = await client.labelsSnapshot("session-001");

    expect(model).toMatchObject({
      active: true,
      session_id: "session-001",
      run_id: "run-001",
      state: "running",
      last_applied_input_frame: 12
    });
    expect(initialRuntimeSessionModel()).toMatchObject({ active: false, state: "idle" });
    expect(labelSnapshot.dedup_groups).toEqual([
      {
        group_id: "dedup-001",
        expected_relation: "same_canonical_state",
        capture_ids: ["capture-001", "capture-002"],
        changed_features: undefined,
        changed_offset_ranges: [{ start: 4, len: 2 }],
        status: "candidate"
      }
    ]);
    expect(fetcher.calls.map((call) => call.url)).toEqual([
      "/api/session/start",
      "/api/run/status",
      "/api/session",
      "/api/session/stop",
      "/api/run/pause",
      "/api/run/resume",
      "/api/frame/current",
      "/api/capture/trigger",
      "/api/capture/jobs/job-001",
      "/api/capture/recent?cursor=cursor-001",
      "/api/capture/capture-001",
      "/api/labels",
      "/api/labels?session_id=session-001"
    ]);
    expect(fetcher.calls[0]?.url).not.toContain("operator-secret");
    expect(JSON.parse(String(fetcher.calls[0]?.init.body))).toMatchObject({
      schema_version: 1,
      operator_credential: "operator-secret"
    });
    for (const call of fetcher.calls) {
      expect(call.init.credentials).toBe("same-origin");
      expect(call.init.cache).toBe("no-store");
    }
  });

  it("rejects schema mismatches instead of coercing responses", async () => {
    const fetcher = queuedFetch([{ ...startSessionResponse(), schema_version: 2 }]);
    const client = new RuntimeApiClient(config, { fetcher });

    await expect(
      client.startSession({ operatorCredential: "operator-secret" })
    ).rejects.toMatchObject({
      display: {
        code: "bad_request",
        message: "Runtime schema mismatch.",
        retryable: false
      }
    });
  });

  it("turns auth and origin error envelopes into sanitized display data", async () => {
    const fetcher = queuedFetch(
      [
        {
          schema_version: 1,
          error: {
            code: "auth_rejected",
            message: "secret leaked from /home/operator/private.env",
            retryable: false,
            details: { path: "/home/operator/private.env" }
          }
        }
      ],
      403
    );
    const client = new RuntimeApiClient(config, { fetcher });

    await expect(client.sessionStatus()).rejects.toSatisfy((error: unknown) => {
      expect(error).toBeInstanceOf(RuntimeApiError);
      const apiError = error as RuntimeApiError;
      expect(apiError.status).toBe(403);
      expect(apiError.display).toEqual({
        code: "auth_rejected",
        message: "Request failed.",
        retryable: false,
        details: {}
      });
      return true;
    });

    const originFetcher = queuedFetch(
      [
        {
          schema_version: 1,
          error: {
            code: "origin_rejected",
            message: "Origin rejected.",
            retryable: false,
            details: { origin: "https://operator.example" }
          }
        }
      ],
      403
    );
    const originClient = new RuntimeApiClient(config, { fetcher: originFetcher });

    await expect(originClient.runStatus()).rejects.toMatchObject({
      display: {
        code: "origin_rejected",
        message: "Origin rejected.",
        retryable: false,
        details: {}
      }
    });
  });
});

describe("runtime WebSocket clients", () => {
  beforeEach(() => {
    FakeWebSocket.instances = [];
  });

  it("sends typed input envelopes, parses acks, and reconnects without URL credentials", () => {
    const messages: RuntimeWsMessage[] = [];
    const reconnects: number[] = [];
    const timers: Array<() => void> = [];
    const client = new RuntimeWebSocketClient(config, {
      socketConstructor: FakeWebSocket,
      location: { protocol: "https:", host: "rombridge.test" },
      maxReconnects: 1,
      reconnectDelayMs: 5,
      setTimer: (callback) => {
        timers.push(callback);
        return timers.length as unknown as ReturnType<typeof setTimeout>;
      },
      clearTimer: () => undefined
    });

    const input = client.inputSocket("session-001", "keyboard", {
      onMessage: (message) => messages.push(message),
      onReconnect: (attempt) => reconnects.push(attempt)
    });
    const firstSocket = FakeWebSocket.instances[0]!;
    const seq = input.sendInput({
      clientEventId: "00000000-0000-0000-0000-000000000001",
      clientTimeMs: 10,
      source: "keyboard",
      buttons: ["A", "Start"]
    });

    expect(firstSocket.url).toBe("wss://rombridge.test/ws/input");
    expect(firstSocket.url).not.toContain("credential");
    expect(seq).toBe(1);
    expect(JSON.parse(firstSocket.sent[0]!)).toEqual({
      schema_version: 1,
      type: "input_state",
      session_id: "session-001",
      client_seq: 1,
      source_id: "keyboard",
      server_seq: null,
      payload: {
        client_event_id: "00000000-0000-0000-0000-000000000001",
        client_time_ms: 10,
        source: "keyboard",
        buttons: ["A", "Start"]
      }
    });

    firstSocket.emitMessage({
      schema_version: 1,
      type: "input_ack",
      session_id: "session-001",
      client_seq: 1,
      source_id: "keyboard",
      server_seq: 1,
      payload: {
        client_event_id: "00000000-0000-0000-0000-000000000001",
        assigned_frame: 12,
        pad_word: 1025,
        status: "applied"
      }
    });
    expect(messages[0]?.type).toBe("input_ack");

    firstSocket.emitClose();
    expect(reconnects).toEqual([1]);
    timers[0]?.();
    expect(FakeWebSocket.instances).toHaveLength(2);
    input.close();
  });

  it("checks server event ordering and reports stale event streams", () => {
    const errors: RuntimeApiError[] = [];
    const client = new RuntimeWebSocketClient(config, {
      socketConstructor: FakeWebSocket,
      location: { protocol: "http:", host: "localhost:5173" },
      maxReconnects: 0
    });

    client.eventSocket({ onError: (error) => errors.push(error) });
    const socket = FakeWebSocket.instances[0]!;
    socket.emitMessage(serverEvent(2));
    socket.emitMessage(serverEvent(2));

    expect(socket.url).toBe("ws://localhost:5173/ws/events");
    expect(errors[0]?.display).toMatchObject({
      code: "bad_request",
      message: "Runtime event ordering error."
    });
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

function sessionResponse() {
  return {
    schema_version: 1,
    active: true,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 4,
    backend_mode: "synthetic"
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

function runStatusResponse() {
  return {
    schema_version: 1,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    backend_mode: "synthetic",
    current_frame: 12,
    last_applied_input_frame: 12,
    last_preview_frame: 10,
    preview_stale: true,
    active_capture_job_id: null,
    capabilities
  };
}

function runStateResponse(state: "paused" | "running") {
  return { schema_version: 1, state, current_frame: 12 };
}

function frameCurrentResponse() {
  return {
    schema_version: 1,
    frame: 12,
    captured_at: "2026-06-24T00:00:00Z",
    stale: false,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: "/api/frame/current/image",
    preview_hash: "hash-001"
  };
}

function captureTriggerResponse() {
  return {
    schema_version: 1,
    job_id: "job-001",
    status: "requested",
    requested_frame: 12,
    scheduled_frame: 13
  };
}

function captureJobResponse() {
  return {
    ...captureTriggerResponse(),
    status: "completed",
    captured_frame: 13,
    capture_id: "capture-001",
    labelable: true,
    has_preview: true,
    error: null
  };
}

function captureSummary() {
  return {
    capture_id: "capture-001",
    frame: 13,
    status: "completed",
    labelable: true,
    has_preview: true,
    labels: ["first_boss"],
    created_at: "2026-06-24T00:00:00Z"
  };
}

function captureRecentResponse() {
  return { schema_version: 1, captures: [captureSummary()], next_cursor: null };
}

function captureDetailResponse() {
  return {
    schema_version: 1,
    capture_id: "capture-001",
    frame: 13,
    status: "completed",
    labelable: true,
    preview_image_url: "/api/capture/capture-001/preview",
    privileged_features_available: false,
    labels: ["first_boss"],
    sanitized_provenance: {
      capture_source: "synthetic",
      layout_hash: "layout-hash",
      capture_spec_hash: "capture-spec-hash",
      map_hash: "map-hash"
    }
  };
}

function labelsResponse() {
  return { schema_version: 1, applied: true, label_revision: 1, conflicts: [] };
}

function labelsSnapshotResponse() {
  return {
    schema_version: 1,
    label_revision: 1,
    target_labels: {
      first_boss: "capture-001",
      goal_positive: null,
      goal_negative: null
    },
    status_labels: [],
    dedup_groups: [
      {
        group_id: "dedup-001",
        expected_relation: "same_canonical_state",
        capture_ids: ["capture-001", "capture-002"],
        changed_offset_ranges: [{ start: 4, len: 2 }],
        status: "candidate"
      }
    ]
  };
}

function serverEvent(serverSeq: number) {
  return {
    schema_version: 1,
    type: "run_updated",
    session_id: "session-001",
    client_seq: null,
    source_id: "server",
    server_seq: serverSeq,
    payload: {
      state: "running",
      current_frame: 2,
      preview_stale: false,
      active_capture_job_id: null
    }
  };
}

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  readonly sent: string[] = [];

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.onclose?.({} as CloseEvent);
  }

  emitMessage(payload: unknown): void {
    this.onmessage?.({ data: JSON.stringify(payload) } as MessageEvent);
  }

  emitClose(): void {
    this.onclose?.({} as CloseEvent);
  }
}
