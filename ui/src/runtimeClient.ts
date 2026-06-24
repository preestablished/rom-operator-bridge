import {
  BACKEND_MODES,
  RUNTIME_API_SCHEMA_VERSION,
  SESSION_STATES,
  type BackendMode,
  type SessionState
} from "./runtimeContract";
import type { RuntimeConfig } from "./runtimeConfig";

type JsonRecord = Record<string, unknown>;
type Fetcher = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

const CAPABILITY_NAMES = [
  "input",
  "preview",
  "capture",
  "labels",
  "privileged_features",
  "validation_runner"
] as const;
const STOP_REASONS = ["operator_stop", "fault_cleanup", "session_replaced"] as const;
const CAPTURE_STATUSES = ["requested", "capturing", "completed", "failed", "not_labelable"] as const;
const LABEL_ROLES = [
  "first_boss",
  "goal_positive",
  "goal_negative",
  "needs_review",
  "rejected"
] as const;
const INPUT_SOURCES = ["keyboard", "gamepad", "combined"] as const;
const PAD_BUTTONS = ["A", "B", "X", "Y", "L", "R", "Up", "Down", "Left", "Right", "Start", "Select"] as const;
const WS_MESSAGE_TYPES = [
  "input_ack",
  "input_reject",
  "session_updated",
  "run_updated",
  "capture_updated",
  "label_updated",
  "validation_updated"
] as const;
const ERROR_CODES = [
  "auth_rejected",
  "origin_rejected",
  "session_inactive",
  "session_active_elsewhere",
  "backend_unavailable",
  "frame_stale",
  "capture_in_progress",
  "capture_failed",
  "label_conflict",
  "validation_failed",
  "bad_request"
] as const;
const PRIVATE_ERROR_PATTERN =
  /credential|password|secret|token|private|\/home\/|\/run\/|\.env|[A-Za-z]:\\/i;

export type CapabilityName = (typeof CAPABILITY_NAMES)[number];
export type StopReason = (typeof STOP_REASONS)[number];
export type CaptureStatus = (typeof CAPTURE_STATUSES)[number];
export type LabelRole = (typeof LABEL_ROLES)[number];
export type InputSource = (typeof INPUT_SOURCES)[number];
export type PadButton = (typeof PAD_BUTTONS)[number];
export type RuntimeErrorCode = (typeof ERROR_CODES)[number];

export type RuntimeCapabilities = Record<CapabilityName, boolean>;

export type RuntimeErrorDisplay = {
  code: RuntimeErrorCode;
  message: string;
  retryable: boolean;
  details: Record<string, never>;
};

export class RuntimeApiError extends Error {
  readonly status: number | null;
  readonly display: RuntimeErrorDisplay;

  constructor(display: RuntimeErrorDisplay, status: number | null = null) {
    super(display.message);
    this.name = "RuntimeApiError";
    this.status = status;
    this.display = display;
  }
}

export type StartSessionResponse = {
  schema_version: 1;
  session_id: string;
  run_id: string;
  state: SessionState;
  current_frame: number;
  pad_layout: {
    layout_id: "console16-12btn-v1";
    layout_version: 1;
  };
  capabilities: RuntimeCapabilities;
};

export type SessionResponse =
  | {
      schema_version: 1;
      active: true;
      session_id: string;
      run_id: string;
      state: SessionState;
      current_frame: number;
      backend_mode: BackendMode;
    }
  | {
      schema_version: 1;
      active: false;
      state: "idle";
    };

export type StopSessionResponse = {
  schema_version: 1;
  session_id: string;
  state: "stopped";
  final_frame: number;
};

export type RunStatusResponse = {
  schema_version: 1;
  session_id: string;
  run_id: string;
  state: SessionState;
  backend_mode: BackendMode;
  current_frame: number;
  last_applied_input_frame: number;
  last_preview_frame: number;
  preview_stale: boolean;
  active_capture_job_id: string | null;
  capabilities: RuntimeCapabilities;
};

export type RunStateResponse = {
  schema_version: 1;
  state: SessionState;
  current_frame: number;
};

export type FrameCurrentResponse = {
  schema_version: 1;
  frame: number;
  captured_at: string;
  stale: boolean;
  width: 256;
  height: 224;
  format: "image/png";
  image_url: string;
  preview_hash: string;
};

export type CaptureTriggerResponse = {
  schema_version: 1;
  job_id: string;
  status: CaptureStatus;
  requested_frame: number;
  scheduled_frame: number;
};

export type CaptureJobResponse = CaptureTriggerResponse & {
  captured_frame: number | null;
  capture_id: string | null;
  labelable: boolean;
  has_preview: boolean;
  error: RuntimeErrorDisplay | null;
};

export type CaptureSummary = {
  capture_id: string;
  frame: number;
  status: CaptureStatus;
  labelable: boolean;
  has_preview: boolean;
  labels: LabelRole[];
  created_at: string;
};

export type CaptureRecentResponse = {
  schema_version: 1;
  captures: CaptureSummary[];
  next_cursor: string | null;
};

export type CaptureDetailResponse = {
  schema_version: 1;
  capture_id: string;
  frame: number;
  status: CaptureStatus;
  labelable: boolean;
  preview_image_url: string;
  privileged_features_available: boolean;
  labels: LabelRole[];
  sanitized_provenance: {
    capture_source: "synthetic" | "hypervisor";
    layout_hash: string;
    capture_spec_hash: string;
    map_hash: string;
  };
};

export type ChangedOffsetRange = {
  start: number;
  len: number;
};

export type LabelUpdate = {
  op: "upsert" | "delete";
  capture_id: string;
  role: LabelRole;
  confidence?: "candidate" | "confirmed";
  note?: string;
};

export type LabelsResponse = {
  schema_version: 1;
  applied: boolean;
  label_revision: number;
  conflicts: RuntimeErrorDisplay[];
};

export type LabelsSnapshotResponse = {
  schema_version: 1;
  label_revision: number;
  target_labels: {
    first_boss: string | null;
    goal_positive: string | null;
    goal_negative: string | null;
  };
  status_labels: Array<{ capture_id: string; status: "needs_review" | "rejected" }>;
  dedup_groups: Array<{
    group_id: string;
    expected_relation: "same_canonical_state" | "distinct_stable_state";
    capture_ids: string[];
    changed_features?: string[];
    changed_offset_ranges?: ChangedOffsetRange[];
    status?: "candidate" | "confirmed" | "conflict";
  }>;
};

export type RuntimeSessionModel = {
  active: boolean;
  session_id: string | null;
  run_id: string | null;
  state: SessionState;
  backend_mode: BackendMode;
  current_frame: number;
  last_applied_input_frame: number;
  last_preview_frame: number;
  preview_stale: boolean;
  capabilities: RuntimeCapabilities | null;
};

export function initialRuntimeSessionModel(): RuntimeSessionModel {
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
    capabilities: null
  };
}

export function modelFromStartSession(response: StartSessionResponse): RuntimeSessionModel {
  return {
    active: true,
    session_id: response.session_id,
    run_id: response.run_id,
    state: response.state,
    backend_mode: "synthetic",
    current_frame: response.current_frame,
    last_applied_input_frame: 0,
    last_preview_frame: 0,
    preview_stale: true,
    capabilities: response.capabilities
  };
}

export function applyRunStatus(
  model: RuntimeSessionModel,
  status: RunStatusResponse
): RuntimeSessionModel {
  return {
    ...model,
    active: true,
    session_id: status.session_id,
    run_id: status.run_id,
    state: status.state,
    backend_mode: status.backend_mode,
    current_frame: status.current_frame,
    last_applied_input_frame: status.last_applied_input_frame,
    last_preview_frame: status.last_preview_frame,
    preview_stale: status.preview_stale,
    capabilities: status.capabilities
  };
}

export class RuntimeApiClient {
  private readonly fetcher: Fetcher;

  constructor(
    private readonly config: RuntimeConfig,
    options: { fetcher?: Fetcher } = {}
  ) {
    this.fetcher = options.fetcher ?? fetch;
  }

  startSession(input: {
    operatorCredential: string;
    backendMode?: BackendMode;
    requestedCapabilities?: CapabilityName[];
  }): Promise<StartSessionResponse> {
    return this.request("/session/start", parseStartSessionResponse, {
      method: "POST",
      body: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        operator_credential: input.operatorCredential,
        backend_mode: input.backendMode ?? "synthetic",
        requested_capabilities: input.requestedCapabilities ?? ["input"]
      }
    });
  }

  sessionStatus(): Promise<SessionResponse> {
    return this.request("/session", parseSessionResponse);
  }

  stopSession(sessionId: string, reason: StopReason = "operator_stop"): Promise<StopSessionResponse> {
    return this.request("/session/stop", parseStopSessionResponse, {
      method: "POST",
      body: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: sessionId,
        reason
      }
    });
  }

  runStatus(): Promise<RunStatusResponse> {
    return this.request("/run/status", parseRunStatusResponse);
  }

  pauseRun(sessionId: string): Promise<RunStateResponse> {
    return this.sessionTransition("/run/pause", sessionId);
  }

  resumeRun(sessionId: string): Promise<RunStateResponse> {
    return this.sessionTransition("/run/resume", sessionId);
  }

  currentFrame(): Promise<FrameCurrentResponse> {
    return this.request("/frame/current", parseFrameCurrentResponse);
  }

  triggerCapture(input: {
    sessionId: string;
    idempotencyKey: string;
    observedPreviewFrame: number;
  }): Promise<CaptureTriggerResponse> {
    return this.request("/capture/trigger", parseCaptureTriggerResponse, {
      method: "POST",
      body: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: input.sessionId,
        idempotency_key: input.idempotencyKey,
        observed_preview_frame: input.observedPreviewFrame,
        reason: "operator_mark"
      }
    });
  }

  captureJob(jobId: string): Promise<CaptureJobResponse> {
    return this.request(`/capture/jobs/${encodeURIComponent(jobId)}`, parseCaptureJobResponse);
  }

  recentCaptures(cursor?: string): Promise<CaptureRecentResponse> {
    const suffix = cursor ? `?cursor=${encodeURIComponent(cursor)}` : "";
    return this.request(`/capture/recent${suffix}`, parseCaptureRecentResponse);
  }

  captureDetail(captureId: string): Promise<CaptureDetailResponse> {
    return this.request(`/capture/${encodeURIComponent(captureId)}`, parseCaptureDetailResponse);
  }

  updateLabels(input: {
    sessionId: string;
    idempotencyKey: string;
    updates: LabelUpdate[];
  }): Promise<LabelsResponse> {
    return this.request("/labels", parseLabelsResponse, {
      method: "POST",
      body: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: input.sessionId,
        idempotency_key: input.idempotencyKey,
        updates: input.updates
      }
    });
  }

  labelsSnapshot(sessionId: string): Promise<LabelsSnapshotResponse> {
    return this.request(
      `/labels?session_id=${encodeURIComponent(sessionId)}`,
      parseLabelsSnapshotResponse
    );
  }

  private sessionTransition(path: string, sessionId: string): Promise<RunStateResponse> {
    return this.request(path, parseRunStateResponse, {
      method: "POST",
      body: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: sessionId
      }
    });
  }

  private async request<T>(
    path: string,
    parser: (value: unknown) => T,
    init: { method?: string; body?: unknown } = {}
  ): Promise<T> {
    const requestInit: RequestInit = {
      method: init.method ?? "GET",
      cache: "no-store",
      credentials: "same-origin",
      headers: {
        accept: "application/json"
      }
    };
    if (init.body !== undefined) {
      requestInit.body = JSON.stringify(init.body);
      requestInit.headers = {
        ...requestInit.headers,
        "content-type": "application/json"
      };
    }

    let response: Response;
    try {
      response = await this.fetcher(joinPath(this.config.api_base_path, path), requestInit);
    } catch {
      throw runtimeError("backend_unavailable", "Runtime unavailable.", true, null);
    }

    const payload = await response.json().catch(() => null);
    if (!response.ok) {
      throw errorFromPayload(payload, response.status);
    }

    try {
      return parser(payload);
    } catch (error) {
      if (error instanceof RuntimeApiError) {
        throw error;
      }
      throw runtimeError("bad_request", "Runtime schema mismatch.", false, response.status);
    }
  }
}

export type InputStateMessage = {
  schema_version: 1;
  type: "input_state";
  session_id: string;
  client_seq: number;
  source_id: string;
  server_seq: null;
  payload: {
    client_event_id: string;
    client_time_ms: number;
    source: InputSource;
    buttons: PadButton[];
  };
};

export type InputAckMessage = {
  schema_version: 1;
  type: "input_ack";
  session_id: string;
  client_seq: number;
  source_id: string;
  server_seq: number | null;
  payload: {
    client_event_id: string;
    assigned_frame: number;
    pad_word: number;
    status: "applied" | "queued" | "dropped";
  };
};

export type InputRejectMessage = {
  schema_version: 1;
  type: "input_reject";
  session_id: string;
  client_seq: number;
  source_id: string;
  server_seq: number | null;
  payload: {
    schema_version: 1;
    error: RuntimeErrorDisplay;
  };
};

export type RuntimeEventMessage = {
  schema_version: 1;
  type: Exclude<(typeof WS_MESSAGE_TYPES)[number], "input_ack" | "input_reject">;
  session_id: string;
  client_seq: null;
  source_id: "server";
  server_seq: number;
  payload: JsonRecord;
};

export type RuntimeWsMessage = InputAckMessage | InputRejectMessage | RuntimeEventMessage;

type WebSocketLike = {
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onclose: ((event: CloseEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  send(data: string): void;
  close(): void;
};
type WebSocketConstructor = new (url: string) => WebSocketLike;
type TimerHandle = ReturnType<typeof setTimeout>;

export class RuntimeSocket {
  private socket: WebSocketLike | null = null;
  private closed = false;
  private reconnectAttempts = 0;
  private lastServerSeq = 0;
  private reconnectTimer: TimerHandle | null = null;

  constructor(
    private readonly url: string,
    private readonly socketConstructor: WebSocketConstructor,
    private readonly handlers: {
      onMessage?: (message: RuntimeWsMessage) => void;
      onError?: (error: RuntimeApiError) => void;
      onReconnect?: (attempt: number) => void;
    } = {},
    private readonly reconnect: {
      maxAttempts: number;
      delayMs: number;
      setTimer: (callback: () => void, delayMs: number) => TimerHandle;
      clearTimer: (handle: TimerHandle) => void;
    } = {
      maxAttempts: 2,
      delayMs: 250,
      setTimer: setTimeout,
      clearTimer: clearTimeout
    }
  ) {
    this.open();
  }

  close(): void {
    this.closed = true;
    if (this.reconnectTimer !== null) {
      this.reconnect.clearTimer(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.socket?.close();
  }

  protected sendJson(value: unknown): void {
    this.socket?.send(JSON.stringify(value));
  }

  private open(): void {
    const socket = new this.socketConstructor(this.url);
    this.socket = socket;
    socket.onmessage = (event) => this.handleMessage(event);
    socket.onerror = () => {
      this.handlers.onError?.(runtimeError("backend_unavailable", "Runtime unavailable.", true, null));
    };
    socket.onclose = () => this.handleClose();
  }

  private handleMessage(event: MessageEvent): void {
    try {
      const message = parseWsMessage(JSON.parse(String(event.data)));
      if (message.server_seq !== null) {
        if (message.server_seq <= this.lastServerSeq) {
          throw runtimeError("bad_request", "Runtime event ordering error.", true, null);
        }
        this.lastServerSeq = message.server_seq;
      }
      this.handlers.onMessage?.(message);
    } catch (error) {
      this.handlers.onError?.(
        error instanceof RuntimeApiError
          ? error
          : runtimeError("bad_request", "Runtime schema mismatch.", false, null)
      );
    }
  }

  private handleClose(): void {
    if (this.closed || this.reconnectAttempts >= this.reconnect.maxAttempts) {
      return;
    }
    this.reconnectAttempts += 1;
    this.handlers.onReconnect?.(this.reconnectAttempts);
    this.reconnectTimer = this.reconnect.setTimer(() => {
      this.reconnectTimer = null;
      this.open();
    }, this.reconnect.delayMs);
  }
}

export class RuntimeInputSocket extends RuntimeSocket {
  private clientSeq = 0;

  constructor(
    url: string,
    socketConstructor: WebSocketConstructor,
    private readonly sessionId: string,
    private readonly sourceId: string,
    handlers: ConstructorParameters<typeof RuntimeSocket>[2],
    reconnect: ConstructorParameters<typeof RuntimeSocket>[3]
  ) {
    super(url, socketConstructor, handlers, reconnect);
  }

  sendInput(input: {
    clientEventId: string;
    clientTimeMs: number;
    source: InputSource;
    buttons: PadButton[];
  }): number {
    this.clientSeq += 1;
    const message: InputStateMessage = {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      type: "input_state",
      session_id: this.sessionId,
      client_seq: this.clientSeq,
      source_id: this.sourceId,
      server_seq: null,
      payload: {
        client_event_id: input.clientEventId,
        client_time_ms: input.clientTimeMs,
        source: input.source,
        buttons: input.buttons
      }
    };
    this.sendJson(message);
    return this.clientSeq;
  }
}

export class RuntimeWebSocketClient {
  constructor(
    private readonly config: RuntimeConfig,
    private readonly options: {
      socketConstructor?: WebSocketConstructor;
      location?: Pick<Location, "protocol" | "host">;
      reconnectDelayMs?: number;
      maxReconnects?: number;
      setTimer?: (callback: () => void, delayMs: number) => TimerHandle;
      clearTimer?: (handle: TimerHandle) => void;
    } = {}
  ) {}

  inputSocket(
    sessionId: string,
    sourceId: string,
    handlers: ConstructorParameters<typeof RuntimeSocket>[2] = {}
  ): RuntimeInputSocket {
    return new RuntimeInputSocket(
      this.url("/input"),
      this.socketConstructor(),
      sessionId,
      sourceId,
      handlers,
      this.reconnectOptions()
    );
  }

  eventSocket(handlers: ConstructorParameters<typeof RuntimeSocket>[2] = {}): RuntimeSocket {
    return new RuntimeSocket(
      this.url("/events"),
      this.socketConstructor(),
      handlers,
      this.reconnectOptions()
    );
  }

  private url(path: string): string {
    const joined = joinPath(this.config.ws_base_path, path);
    const location = this.options.location ?? globalThis.location;
    if (!location) {
      return joined;
    }
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    return `${protocol}//${location.host}${joined}`;
  }

  private socketConstructor(): WebSocketConstructor {
    const constructor = this.options.socketConstructor ?? globalThis.WebSocket;
    if (!constructor) {
      throw runtimeError("backend_unavailable", "Runtime unavailable.", true, null);
    }
    return constructor as WebSocketConstructor;
  }

  private reconnectOptions(): ConstructorParameters<typeof RuntimeSocket>[3] {
    return {
      maxAttempts: this.options.maxReconnects ?? 2,
      delayMs: this.options.reconnectDelayMs ?? 250,
      setTimer: this.options.setTimer ?? setTimeout,
      clearTimer: this.options.clearTimer ?? clearTimeout
    };
  }
}

function joinPath(base: string, path: string): string {
  return `${base.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`;
}

function parseStartSessionResponse(value: unknown): StartSessionResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: stringField(record, "session_id"),
    run_id: stringField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame"),
    pad_layout: parsePadLayout(record["pad_layout"]),
    capabilities: parseCapabilities(record["capabilities"])
  };
}

function parseSessionResponse(value: unknown): SessionResponse {
  const record = schemaRecord(value);
  if (record["active"] === false) {
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      active: false,
      state: enumField(record, "state", ["idle"] as const)
    };
  }
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    active: true,
    session_id: stringField(record, "session_id"),
    run_id: stringField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame"),
    backend_mode: enumField(record, "backend_mode", BACKEND_MODES)
  };
}

function parseStopSessionResponse(value: unknown): StopSessionResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: stringField(record, "session_id"),
    state: enumField(record, "state", ["stopped"] as const),
    final_frame: u64Field(record, "final_frame")
  };
}

function parseRunStatusResponse(value: unknown): RunStatusResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: stringField(record, "session_id"),
    run_id: stringField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    backend_mode: enumField(record, "backend_mode", BACKEND_MODES),
    current_frame: u64Field(record, "current_frame"),
    last_applied_input_frame: u64Field(record, "last_applied_input_frame"),
    last_preview_frame: u64Field(record, "last_preview_frame"),
    preview_stale: booleanField(record, "preview_stale"),
    active_capture_job_id: nullableStringField(record, "active_capture_job_id"),
    capabilities: parseCapabilities(record["capabilities"])
  };
}

function parseRunStateResponse(value: unknown): RunStateResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame")
  };
}

function parseFrameCurrentResponse(value: unknown): FrameCurrentResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    frame: u64Field(record, "frame"),
    captured_at: stringField(record, "captured_at"),
    stale: booleanField(record, "stale"),
    width: literalField(record, "width", 256),
    height: literalField(record, "height", 224),
    format: literalField(record, "format", "image/png"),
    image_url: stringField(record, "image_url"),
    preview_hash: stringField(record, "preview_hash")
  };
}

function parseCaptureTriggerResponse(value: unknown): CaptureTriggerResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    job_id: stringField(record, "job_id"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    requested_frame: u64Field(record, "requested_frame"),
    scheduled_frame: u64Field(record, "scheduled_frame")
  };
}

function parseCaptureJobResponse(value: unknown): CaptureJobResponse {
  const record = schemaRecord(value);
  return {
    ...parseCaptureTriggerResponse(value),
    captured_frame: nullableU64Field(record, "captured_frame"),
    capture_id: nullableStringField(record, "capture_id"),
    labelable: booleanField(record, "labelable"),
    has_preview: booleanField(record, "has_preview"),
    error: record["error"] === null ? null : parseErrorObject(record["error"])
  };
}

function parseCaptureRecentResponse(value: unknown): CaptureRecentResponse {
  const record = schemaRecord(value);
  const captures = arrayField(record, "captures").map(parseCaptureSummary);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    captures,
    next_cursor: nullableStringField(record, "next_cursor")
  };
}

function parseCaptureDetailResponse(value: unknown): CaptureDetailResponse {
  const record = schemaRecord(value);
  const provenance = recordField(record, "sanitized_provenance");
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    capture_id: stringField(record, "capture_id"),
    frame: u64Field(record, "frame"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    labelable: booleanField(record, "labelable"),
    preview_image_url: stringField(record, "preview_image_url"),
    privileged_features_available: booleanField(record, "privileged_features_available"),
    labels: arrayField(record, "labels").map((label) => enumValue(label, LABEL_ROLES)),
    sanitized_provenance: {
      capture_source: enumField(provenance, "capture_source", ["synthetic", "hypervisor"] as const),
      layout_hash: stringField(provenance, "layout_hash"),
      capture_spec_hash: stringField(provenance, "capture_spec_hash"),
      map_hash: stringField(provenance, "map_hash")
    }
  };
}

function parseLabelsResponse(value: unknown): LabelsResponse {
  const record = schemaRecord(value);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    applied: booleanField(record, "applied"),
    label_revision: u64Field(record, "label_revision"),
    conflicts: arrayField(record, "conflicts").map(parseErrorObject)
  };
}

function parseLabelsSnapshotResponse(value: unknown): LabelsSnapshotResponse {
  const record = schemaRecord(value);
  const targets = recordField(record, "target_labels");
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    label_revision: u64Field(record, "label_revision"),
    target_labels: {
      first_boss: nullableStringField(targets, "first_boss"),
      goal_positive: nullableStringField(targets, "goal_positive"),
      goal_negative: nullableStringField(targets, "goal_negative")
    },
    status_labels: arrayField(record, "status_labels").map((entry) => {
      const status = recordValue(entry);
      return {
        capture_id: stringField(status, "capture_id"),
        status: enumField(status, "status", ["needs_review", "rejected"] as const)
      };
    }),
    dedup_groups: arrayField(record, "dedup_groups").map((entry) => {
      const group = recordValue(entry);
      const changed_features =
        group["changed_features"] === undefined
          ? undefined
          : arrayField(group, "changed_features").map(stringValue);
      const changed_offset_ranges =
        group["changed_offset_ranges"] === undefined
          ? undefined
          : arrayField(group, "changed_offset_ranges").map((range) => {
              const offsetRange = recordValue(range);
              return {
                start: u64Field(offsetRange, "start"),
                len: u64Field(offsetRange, "len")
              };
            });
      if (changed_features === undefined && changed_offset_ranges === undefined) {
        throw new Error("expected dedup change details");
      }
      return {
        group_id: stringField(group, "group_id"),
        expected_relation: enumField(group, "expected_relation", [
          "same_canonical_state",
          "distinct_stable_state"
        ] as const),
        capture_ids: arrayField(group, "capture_ids").map(stringValue),
        changed_features,
        changed_offset_ranges,
        status:
          group["status"] === undefined
            ? undefined
            : enumField(group, "status", ["candidate", "confirmed", "conflict"] as const)
      };
    })
  };
}

function parseCaptureSummary(value: unknown): CaptureSummary {
  const record = recordValue(value);
  return {
    capture_id: stringField(record, "capture_id"),
    frame: u64Field(record, "frame"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    labelable: booleanField(record, "labelable"),
    has_preview: booleanField(record, "has_preview"),
    labels: arrayField(record, "labels").map((label) => enumValue(label, LABEL_ROLES)),
    created_at: stringField(record, "created_at")
  };
}

function parseWsMessage(value: unknown): RuntimeWsMessage {
  const record = schemaRecord(value);
  const type = enumField(record, "type", WS_MESSAGE_TYPES);
  if (type === "input_ack") {
    const payload = recordField(record, "payload");
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      type,
      session_id: stringField(record, "session_id"),
      client_seq: u64Field(record, "client_seq"),
      source_id: stringField(record, "source_id"),
      server_seq: nullableU64Field(record, "server_seq"),
      payload: {
        client_event_id: stringField(payload, "client_event_id"),
        assigned_frame: u64Field(payload, "assigned_frame"),
        pad_word: u64Field(payload, "pad_word"),
        status: enumField(payload, "status", ["applied", "queued", "dropped"] as const)
      }
    };
  }
  if (type === "input_reject") {
    const payload = schemaRecord(record["payload"]);
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      type,
      session_id: stringField(record, "session_id"),
      client_seq: u64Field(record, "client_seq"),
      source_id: stringField(record, "source_id"),
      server_seq: nullableU64Field(record, "server_seq"),
      payload: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        error: parseErrorObject(payload["error"])
      }
    };
  }
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    type,
    session_id: stringField(record, "session_id"),
    client_seq: null,
    source_id: enumField(record, "source_id", ["server"] as const),
    server_seq: u64Field(record, "server_seq"),
    payload: recordField(record, "payload")
  };
}

function parsePadLayout(value: unknown): StartSessionResponse["pad_layout"] {
  const record = recordValue(value);
  return {
    layout_id: literalField(record, "layout_id", "console16-12btn-v1"),
    layout_version: literalField(record, "layout_version", 1)
  };
}

function parseCapabilities(value: unknown): RuntimeCapabilities {
  const record = recordValue(value);
  return Object.fromEntries(
    CAPABILITY_NAMES.map((capability) => [capability, booleanField(record, capability)])
  ) as RuntimeCapabilities;
}

function parseErrorObject(value: unknown): RuntimeErrorDisplay {
  const record = recordValue(value);
  return sanitizeDisplayError({
    code: enumField(record, "code", ERROR_CODES),
    message: stringField(record, "message"),
    retryable: booleanField(record, "retryable"),
    details: {}
  });
}

function errorFromPayload(payload: unknown, status: number): RuntimeApiError {
  if (isRecord(payload) && payload["schema_version"] === RUNTIME_API_SCHEMA_VERSION) {
    const error = payload["error"];
    if (isRecord(error)) {
      return new RuntimeApiError(parseErrorObject(error), status);
    }
  }
  return runtimeError("backend_unavailable", "Runtime unavailable.", true, status);
}

function runtimeError(
  code: RuntimeErrorCode,
  message: string,
  retryable: boolean,
  status: number | null
): RuntimeApiError {
  return new RuntimeApiError(sanitizeDisplayError({ code, message, retryable, details: {} }), status);
}

function sanitizeDisplayError(error: RuntimeErrorDisplay): RuntimeErrorDisplay {
  return {
    code: error.code,
    message: PRIVATE_ERROR_PATTERN.test(error.message) ? "Request failed." : error.message,
    retryable: error.retryable,
    details: {}
  };
}

function schemaRecord(value: unknown): JsonRecord {
  const record = recordValue(value);
  if (record["schema_version"] !== RUNTIME_API_SCHEMA_VERSION) {
    throw runtimeError("bad_request", "Runtime schema mismatch.", false, null);
  }
  return record;
}

function recordField(record: JsonRecord, key: string): JsonRecord {
  return recordValue(record[key]);
}

function recordValue(value: unknown): JsonRecord {
  if (!isRecord(value)) {
    throw new Error("expected object");
  }
  return value;
}

function stringField(record: JsonRecord, key: string): string {
  return stringValue(record[key]);
}

function nullableStringField(record: JsonRecord, key: string): string | null {
  return record[key] === null ? null : stringField(record, key);
}

function stringValue(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("expected string");
  }
  return value;
}

function booleanField(record: JsonRecord, key: string): boolean {
  if (typeof record[key] !== "boolean") {
    throw new Error("expected boolean");
  }
  return record[key];
}

function u64Field(record: JsonRecord, key: string): number {
  const value = record[key];
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error("expected u64");
  }
  return value;
}

function nullableU64Field(record: JsonRecord, key: string): number | null {
  return record[key] === null ? null : u64Field(record, key);
}

function literalField<T extends string | number | boolean>(
  record: JsonRecord,
  key: string,
  expected: T
): T {
  if (record[key] !== expected) {
    throw new Error("unexpected literal");
  }
  return expected;
}

function enumField<const T extends readonly string[]>(
  record: JsonRecord,
  key: string,
  values: T
): T[number] {
  return enumValue(record[key], values);
}

function enumValue<const T extends readonly string[]>(value: unknown, values: T): T[number] {
  if (typeof value !== "string" || !values.includes(value)) {
    throw new Error("unexpected enum");
  }
  return value as T[number];
}

function arrayField(record: JsonRecord, key: string): unknown[] {
  const value = record[key];
  if (!Array.isArray(value)) {
    throw new Error("expected array");
  }
  return value;
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
