// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { mountOperatorApp } from "../../src/app";
import type {
  RuntimeEventClient,
  RuntimePreviewClient,
  RuntimeRunClient
} from "../../src/app";
import type { RuntimeSessionClient } from "../../src/authSession";
import type {
  CaptureDetailResponse,
  CaptureRecentResponse,
  LabelsSnapshotResponse,
  RuntimeWsMessage
} from "../../src/runtimeClient";
import { RuntimeApiClient, RuntimeApiError } from "../../src/runtimeClient";
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
  privileged_features: true,
  validation_runner: false
};

describe("capture review and label drawer", () => {
  it("renders capture job and recent review states with sanitized provenance only", async () => {
    const socketClient = mockSocketClient();
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse({ active_capture_job_id: "job-009" })),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
      captureJob: vi.fn().mockResolvedValue(captureJobResponse({ status: "requested", job_id: "job-009" })),
      recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
      captureDetail: vi.fn().mockResolvedValue(captureDetailResponse()),
      labelsSnapshot: vi.fn().mockResolvedValue(labelsSnapshotResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, socketClient.client);
    await flushPromises();
    socketClient.emitEvent(captureUpdated(2, { status: "capturing", job_id: "job-009" }));
    await flushPromises();

    expect(root.textContent).toContain("requested");
    expect(root.textContent).toContain("capturing");
    expect(root.textContent).toContain("completed");
    expect(root.textContent).toContain("failed");
    expect(root.textContent).toContain("not labelable");
    expect(root.textContent).toContain("sha256:layout-public");
    expect(root.textContent).toContain("host available");
    expect(root.textContent).toContain("withheld");
    expect(root.textContent ?? "").not.toMatch(
      /\/home\/|private\.env|feature bytes|decoded_features|raw capture|screenshot/i
    );
    expect(root.querySelector("[data-capture-preview]")?.getAttribute("src")).toBe(
      "/api/capture/capture-001/preview"
    );
  });

  it("writes and deletes labels from the drawer without browser persistence", async () => {
    const storageSpy = vi.spyOn(Storage.prototype, "setItem");
    const updateLabels = vi.fn().mockResolvedValue({
      schema_version: 1,
      applied: true,
      label_revision: 2,
      conflicts: []
    });
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
      recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
      captureDetail: vi
        .fn()
        .mockResolvedValueOnce(captureDetailResponse())
        .mockResolvedValue(captureDetailResponse({ labels: ["goal_positive", "needs_review"] })),
      labelsSnapshot: vi
        .fn()
        .mockResolvedValueOnce(labelsSnapshotResponse())
        .mockResolvedValue(labelsSnapshotResponse({ label_revision: 2 })),
      updateLabels
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    const firstBoss = root.querySelector<HTMLInputElement>("input[name='label_first_boss']");
    const goalPositive = root.querySelector<HTMLInputElement>("input[name='label_goal_positive']");
    const note = root.querySelector<HTMLTextAreaElement>("[data-private-note]");
    const compareCapture = root.querySelector<HTMLInputElement>("input[name='dedup_capture_id']");
    const changedFeature = root.querySelector<HTMLInputElement>("input[name='dedup_changed_feature']");
    firstBoss!.checked = false;
    goalPositive!.checked = true;
    note!.value = "Private operator note for verifier context";
    compareCapture!.value = "capture-004";
    changedFeature!.value = "stable door flag";
    root
      .querySelector<HTMLFormElement>("[data-label-drawer-form='capture']")
      ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();

    expect(updateLabels).toHaveBeenCalledWith({
      sessionId: "session-001",
      idempotencyKey: expect.stringMatching(/^[0-9a-f-]{36}$/i),
      updates: [
        { op: "delete", capture_id: "capture-001", role: "first_boss" },
        {
          op: "upsert",
          capture_id: "capture-001",
          role: "goal_positive",
          confidence: "candidate",
          note: "Private operator note for verifier context"
        }
      ],
      dedupUpdates: [
        {
          op: "upsert",
          group_id: "dedup-capture-001-capture-004",
          expected_relation: "same_canonical_state",
          capture_ids: ["capture-001", "capture-004"],
          changed_features: ["stable door flag"],
          status: "candidate"
        }
      ]
    });
    expect(storageSpy).not.toHaveBeenCalled();
    expect(root.querySelector<HTMLTextAreaElement>("[data-private-note]")?.value).toBe("");
    expect(root.textContent).toContain("r2");
  });

  it("shows sanitized role conflicts and keeps private paths out of the drawer", async () => {
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
      recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
      captureDetail: vi.fn().mockResolvedValue(captureDetailResponse()),
      labelsSnapshot: vi.fn().mockResolvedValue(
        labelsSnapshotResponse({
          target_labels: {
            first_boss: "capture-other",
            goal_positive: null,
            goal_negative: null
          }
        })
      ),
      updateLabels: vi.fn().mockResolvedValue({
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
      })
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    root.querySelector<HTMLInputElement>("input[name='label_rejected']")!.checked = true;
    root
      .querySelector<HTMLFormElement>("[data-label-drawer-form='capture']")
      ?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await flushPromises();

    const conflicts = root.querySelector("[data-label-conflicts]");
    expect(conflicts?.textContent).toContain("first boss is assigned to another capture");
    expect(conflicts?.textContent).toContain("Resolve the conflicting label role");
    expect(root.textContent ?? "").not.toMatch(/\/home\/|private\/captures|index\.jsonl/i);
  });

  it("keeps not-labelable drawers locked and exposes a retry affordance for failed captures", async () => {
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
    const client = mockClient({
      sessionStatus: vi.fn().mockResolvedValue(activeSessionResponse()),
      runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
      currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
      recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
      captureDetail: vi.fn().mockResolvedValue(
        captureDetailResponse({
          capture_id: "capture-003",
          status: "not_labelable",
          labelable: false,
          labels: []
        })
      ),
      labelsSnapshot: vi.fn().mockResolvedValue(labelsSnapshotResponse()),
      triggerCapture,
      captureJob: vi.fn().mockResolvedValue(captureJobResponse())
    });
    const root = document.createElement("div");

    mountOperatorApp(root, config, client, null);
    await flushPromises();
    expect(root.querySelector<HTMLButtonElement>("[data-label-drawer-form='capture'] button[type='submit']")?.disabled).toBe(
      true
    );

    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();
    expect(root.textContent).toContain("Capture failed.");
    expect(root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.textContent).toContain(
      "Retry"
    );
    root.querySelector<HTMLButtonElement>("[data-run-action='capture']")?.click();
    await flushPromises();

    expect(triggerCapture).toHaveBeenCalledTimes(2);
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
    startSession: vi.fn(),
    sessionStatus: vi.fn().mockResolvedValue({ schema_version: 1, active: false, state: "idle" }),
    stopSession: vi.fn(),
    runStatus: vi.fn().mockResolvedValue(runStatusResponse()),
    pauseRun: vi.fn(),
    resumeRun: vi.fn(),
    currentFrame: vi.fn().mockResolvedValue(frameCurrentResponse()),
    triggerCapture: vi.fn(),
    captureJob: vi.fn(),
    recentCaptures: vi.fn().mockResolvedValue(captureRecentResponse()),
    captureDetail: vi.fn().mockResolvedValue(captureDetailResponse()),
    labelsSnapshot: vi.fn().mockResolvedValue(labelsSnapshotResponse()),
    updateLabels: vi.fn(),
    ...overrides
  } as MockRuntimeClient;
}

function mockSocketClient(): {
  client: RuntimeEventClient;
  emitEvent: (message: RuntimeWsMessage) => void;
} {
  let onEventMessage: ((message: RuntimeWsMessage) => void) | undefined;
  return {
    client: {
      eventSocket: vi.fn((handlers = {}) => {
        onEventMessage = handlers.onMessage;
        return { close: vi.fn() } as unknown as ReturnType<RuntimeEventClient["eventSocket"]>;
      })
    },
    emitEvent: (message) => onEventMessage?.(message)
  };
}

function activeSessionResponse() {
  return {
    schema_version: 1,
    active: true,
    session_id: "session-001",
    run_id: "run-001",
    state: "running",
    current_frame: 42,
    backend_mode: "synthetic"
  };
}

function runStatusResponse(overrides: Record<string, unknown> = {}) {
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

function frameCurrentResponse() {
  return {
    schema_version: 1,
    frame: 42,
    captured_at: "1970-01-01T00:00:00Z",
    stale: false,
    width: 256,
    height: 224,
    format: "image/png",
    image_url: "/api/frame/current/image?frame=42",
    preview_hash: "sha256:frame-public"
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

function captureJobResponse(overrides: Record<string, unknown> = {}) {
  return {
    ...captureTriggerResponse(),
    status: "completed",
    captured_frame: 43,
    capture_id: "capture-001",
    labelable: true,
    has_preview: true,
    error: null,
    ...overrides
  };
}

function captureRecentResponse(): CaptureRecentResponse {
  return {
    schema_version: 1,
    captures: [
      {
        capture_id: "capture-001",
        frame: 43,
        status: "completed",
        labelable: true,
        has_preview: true,
        labels: ["first_boss", "needs_review"],
        created_at: "2026-06-24T09:00:00Z"
      },
      {
        capture_id: "capture-002",
        frame: 44,
        status: "failed",
        labelable: false,
        has_preview: false,
        labels: [],
        created_at: "2026-06-24T09:01:00Z"
      },
      {
        capture_id: "capture-003",
        frame: 45,
        status: "not_labelable",
        labelable: false,
        has_preview: true,
        labels: [],
        created_at: "2026-06-24T09:02:00Z"
      }
    ],
    next_cursor: null
  };
}

function captureDetailResponse(overrides: Partial<CaptureDetailResponse> = {}): CaptureDetailResponse {
  return {
    schema_version: 1,
    capture_id: "capture-001",
    frame: 43,
    status: "completed",
    labelable: true,
    preview_image_url: "/api/capture/capture-001/preview",
    privileged_features_available: true,
    labels: ["first_boss", "needs_review"],
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
      first_boss: "capture-001",
      goal_positive: null,
      goal_negative: null
    },
    status_labels: [{ capture_id: "capture-001", status: "needs_review" }],
    dedup_groups: [
      {
        group_id: "dedup-001",
        expected_relation: "same_canonical_state",
        capture_ids: ["capture-001", "capture-004"],
        changed_offset_ranges: [{ start: 4, len: 2 }],
        status: "candidate"
      }
    ],
    ...overrides
  };
}

function captureUpdated(
  serverSeq: number,
  overrides: Partial<Extract<RuntimeWsMessage, { type: "capture_updated" }>["payload"]> = {}
): RuntimeWsMessage {
  return {
    schema_version: 1,
    type: "capture_updated",
    session_id: "session-001",
    client_seq: null,
    source_id: "server",
    server_seq: serverSeq,
    payload: {
      job_id: "job-001",
      status: "completed",
      capture_id: "capture-001",
      ...overrides
    }
  };
}

async function flushPromises(): Promise<void> {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve();
  }
}
