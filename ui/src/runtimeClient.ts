import {
  BACKEND_MODES,
  CAPABILITY_NAMES,
  CAPTURE_STATUSES,
  ERROR_CODES,
  INPUT_SOURCES,
  LABEL_ROLES,
  PAD_BUTTONS,
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  RUNTIME_API_SCHEMA_VERSION,
  SESSION_STATES,
  type BackendMode,
  type SessionState
} from "./runtimeContract";
import type { RuntimeConfig } from "./runtimeConfig";

type JsonRecord = Record<string, unknown>;
type Fetcher = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

const STOP_REASONS = ["operator_stop", "fault_cleanup", "session_replaced"] as const;
const PRIVATE_ERROR_PATTERN =
  /credential|password|secret|token|private|\/home\/|\/run\/|\.env|[A-Za-z]:\\/i;
const ID_PATTERN = /^[A-Za-z0-9._:-]{1,128}$/;
const UUID_PATTERN =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;
const RFC3339_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;
const FRAME_IMAGE_PATTERN = /^\/api\/frame\/current\/image(?:\?frame=\d+)?$/;
const CAPTURE_PREVIEW_PATTERN =
  /^\/api\/capture\/[A-Za-z0-9._:-]+\/preview(?:\?frame=\d+)?$/;
const PAD_WORD_MAX = 0x0fff;
const WS_OPEN = 1;
const INBOUND_WS_MESSAGE_TYPES = [
  "input_ack",
  "input_reject",
  "session_updated",
  "run_updated",
  "capture_updated",
  "label_updated",
  "validation_updated"
] as const;
const VALIDATION_STATUSES = ["not_run", "running", "passed", "failed"] as const;

export type CapabilityName = (typeof CAPABILITY_NAMES)[number];
export type StopReason = (typeof STOP_REASONS)[number];
export type CaptureStatus = (typeof CAPTURE_STATUSES)[number];
export type LabelRole = (typeof LABEL_ROLES)[number];
export type InputSource = (typeof INPUT_SOURCES)[number];
export type PadButton = (typeof PAD_BUTTONS)[number];
export type RuntimeErrorCode = (typeof ERROR_CODES)[number];

export type RuntimeCapabilities = Record<CapabilityName, boolean>;

export type HealthResponse = {
  schema_version: 1;
  ok: boolean;
  service_version: string;
  backend_mode: BackendMode;
  runtime_api: 1;
};

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

  health(): Promise<HealthResponse> {
    return this.requestFrom("", "/health", parseHealthResponse);
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

  recentCaptures(input: { cursor?: string; limit?: number } = {}): Promise<CaptureRecentResponse> {
    const params = new URLSearchParams();
    if (input.cursor) {
      params.set("cursor", input.cursor);
    }
    if (input.limit !== undefined) {
      params.set("limit", String(input.limit));
    }
    const query = params.toString();
    const suffix = query ? `?${query}` : "";
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

  labelsSnapshot(): Promise<LabelsSnapshotResponse> {
    return this.request("/labels", parseLabelsSnapshotResponse);
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
    return this.requestFrom(this.config.api_base_path, path, parser, init);
  }

  private async requestFrom<T>(
    basePath: string,
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
      response = await this.fetcher(joinPath(basePath, path), requestInit);
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
        throw new RuntimeApiError(error.display, response.status);
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

export type SessionUpdatedPayload = {
  state: SessionState;
  backend_mode: BackendMode;
  current_frame: number;
  capabilities: RuntimeCapabilities;
};

export type RunUpdatedPayload = {
  state: SessionState;
  current_frame: number;
  preview_stale: boolean;
  active_capture_job_id: string | null;
};

export type CaptureUpdatedPayload = {
  job_id: string;
  status: CaptureStatus;
  capture_id: string | null;
};

export type LabelUpdatedPayload = {
  label_revision: number;
  applied: boolean;
};

export type ValidationUpdatedPayload = {
  status: "not_run" | "running" | "passed" | "failed";
  summary: string;
};

type RuntimeEventBase<TType extends string> = {
  schema_version: 1;
  type: TType;
  session_id: string;
  client_seq: null;
  source_id: "server";
  server_seq: number;
};

export type RuntimeEventMessage =
  | (RuntimeEventBase<"session_updated"> & {
      payload: SessionUpdatedPayload;
    })
  | (RuntimeEventBase<"run_updated"> & { payload: RunUpdatedPayload })
  | (RuntimeEventBase<"capture_updated"> & {
      payload: CaptureUpdatedPayload;
    })
  | (RuntimeEventBase<"label_updated"> & { payload: LabelUpdatedPayload })
  | (RuntimeEventBase<"validation_updated"> & {
      payload: ValidationUpdatedPayload;
    });

export type RuntimeWsMessage = InputAckMessage | InputRejectMessage | RuntimeEventMessage;

type WebSocketLike = {
  readonly readyState: number;
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
  private pendingSends: string[] = [];

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
    const data = JSON.stringify(value);
    if (this.socket?.readyState === WS_OPEN) {
      this.socket.send(data);
      return;
    }
    this.pendingSends.push(data);
  }

  protected acceptMessage(message: RuntimeWsMessage): RuntimeWsMessage | null {
    return message;
  }

  protected clearPendingSends(): void {
    this.pendingSends = [];
  }

  protected afterReconnect(): void {}

  private open(wasReconnect = false): void {
    const socket = new this.socketConstructor(this.url);
    this.socket = socket;
    socket.onopen = () => {
      this.reconnectAttempts = 0;
      if (wasReconnect) {
        this.afterReconnect();
      }
      for (const data of this.pendingSends.splice(0)) {
        socket.send(data);
      }
    };
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
          return;
        }
        this.lastServerSeq = message.server_seq;
      }
      const accepted = this.acceptMessage(message);
      if (accepted !== null) {
        this.handlers.onMessage?.(accepted);
      }
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
      this.open(true);
    }, this.reconnect.delayMs);
  }
}

export class RuntimeInputSocket extends RuntimeSocket {
  private clientSeq = 0;
  private lastSource: InputSource;

  constructor(
    url: string,
    socketConstructor: WebSocketConstructor,
    private readonly sessionId: string,
    private readonly sourceId: string,
    handlers: ConstructorParameters<typeof RuntimeSocket>[2],
    reconnect: ConstructorParameters<typeof RuntimeSocket>[3],
    private readonly createClientEventId: () => string = createRuntimeClientEventId,
    private readonly nowMs: () => number = () => Date.now()
  ) {
    const initialSource = enumValueOrDefault(sourceId, INPUT_SOURCES, "combined");
    super(url, socketConstructor, handlers, reconnect);
    this.lastSource = initialSource;
  }

  protected override acceptMessage(message: RuntimeWsMessage): RuntimeWsMessage | null {
    if (message.type !== "input_ack" && message.type !== "input_reject") {
      throw runtimeError("bad_request", "Runtime schema mismatch.", false, null);
    }
    if (message.session_id !== this.sessionId || message.source_id !== this.sourceId) {
      throw runtimeError("bad_request", "Runtime schema mismatch.", false, null);
    }
    return message;
  }

  protected override afterReconnect(): void {
    this.clearPendingSends();
    this.sendInput({
      clientEventId: this.createClientEventId(),
      clientTimeMs: this.nowMs(),
      source: this.lastSource,
      buttons: []
    });
  }

  sendInput(input: {
    clientEventId: string;
    clientTimeMs: number;
    source: InputSource;
    buttons: PadButton[];
  }): number {
    this.clientSeq += 1;
    this.lastSource = input.source;
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
      createClientEventId?: () => string;
      nowMs?: () => number;
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
      this.reconnectOptions(),
      this.options.createClientEventId,
      this.options.nowMs
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

function parseHealthResponse(value: unknown): HealthResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "ok",
    "service_version",
    "backend_mode",
    "runtime_api"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    ok: booleanField(record, "ok"),
    service_version: boundedStringField(record, "service_version", 1, 240),
    backend_mode: enumField(record, "backend_mode", BACKEND_MODES),
    runtime_api: literalField(record, "runtime_api", RUNTIME_API_SCHEMA_VERSION)
  };
}

function parseStartSessionResponse(value: unknown): StartSessionResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "session_id",
    "run_id",
    "state",
    "current_frame",
    "pad_layout",
    "capabilities"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: idField(record, "session_id"),
    run_id: idField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame"),
    pad_layout: parsePadLayout(record["pad_layout"]),
    capabilities: parseCapabilities(record["capabilities"])
  };
}

function parseSessionResponse(value: unknown): SessionResponse {
  const record = schemaRecord(value);
  if (record["active"] === false) {
    assertOnlyFields(record, ["schema_version", "active", "state"]);
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      active: false,
      state: enumField(record, "state", ["idle"] as const)
    };
  }
  assertOnlyFields(record, [
    "schema_version",
    "active",
    "session_id",
    "run_id",
    "state",
    "current_frame",
    "backend_mode"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    active: true,
    session_id: idField(record, "session_id"),
    run_id: idField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame"),
    backend_mode: enumField(record, "backend_mode", BACKEND_MODES)
  };
}

function parseStopSessionResponse(value: unknown): StopSessionResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, ["schema_version", "session_id", "state", "final_frame"]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: idField(record, "session_id"),
    state: enumField(record, "state", ["stopped"] as const),
    final_frame: u64Field(record, "final_frame")
  };
}

function parseRunStatusResponse(value: unknown): RunStatusResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "session_id",
    "run_id",
    "state",
    "backend_mode",
    "current_frame",
    "last_applied_input_frame",
    "last_preview_frame",
    "preview_stale",
    "active_capture_job_id",
    "capabilities"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    session_id: idField(record, "session_id"),
    run_id: idField(record, "run_id"),
    state: enumField(record, "state", SESSION_STATES),
    backend_mode: enumField(record, "backend_mode", BACKEND_MODES),
    current_frame: u64Field(record, "current_frame"),
    last_applied_input_frame: u64Field(record, "last_applied_input_frame"),
    last_preview_frame: u64Field(record, "last_preview_frame"),
    preview_stale: booleanField(record, "preview_stale"),
    active_capture_job_id: nullableIdField(record, "active_capture_job_id"),
    capabilities: parseCapabilities(record["capabilities"])
  };
}

function parseRunStateResponse(value: unknown): RunStateResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, ["schema_version", "state", "current_frame"]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    state: enumField(record, "state", SESSION_STATES),
    current_frame: u64Field(record, "current_frame")
  };
}

function parseFrameCurrentResponse(value: unknown): FrameCurrentResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "frame",
    "captured_at",
    "stale",
    "width",
    "height",
    "format",
    "image_url",
    "preview_hash"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    frame: u64Field(record, "frame"),
    captured_at: rfc3339Field(record, "captured_at"),
    stale: booleanField(record, "stale"),
    width: literalField(record, "width", 256),
    height: literalField(record, "height", 224),
    format: literalField(record, "format", "image/png"),
    image_url: patternStringField(record, "image_url", FRAME_IMAGE_PATTERN),
    preview_hash: hashField(record, "preview_hash")
  };
}

function parseCaptureTriggerResponse(value: unknown): CaptureTriggerResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "job_id",
    "status",
    "requested_frame",
    "scheduled_frame"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    job_id: idField(record, "job_id"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    requested_frame: u64Field(record, "requested_frame"),
    scheduled_frame: u64Field(record, "scheduled_frame")
  };
}

function parseCaptureJobResponse(value: unknown): CaptureJobResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, [
    "schema_version",
    "job_id",
    "status",
    "requested_frame",
    "scheduled_frame",
    "captured_frame",
    "capture_id",
    "labelable",
    "has_preview",
    "error"
  ]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    job_id: idField(record, "job_id"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    requested_frame: u64Field(record, "requested_frame"),
    scheduled_frame: u64Field(record, "scheduled_frame"),
    captured_frame: nullableU64Field(record, "captured_frame"),
    capture_id: nullableIdField(record, "capture_id"),
    labelable: booleanField(record, "labelable"),
    has_preview: booleanField(record, "has_preview"),
    error: record["error"] === null ? null : parseErrorObject(record["error"])
  };
}

function parseCaptureRecentResponse(value: unknown): CaptureRecentResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, ["schema_version", "captures", "next_cursor"]);
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
  assertOnlyFields(record, [
    "schema_version",
    "capture_id",
    "frame",
    "status",
    "labelable",
    "preview_image_url",
    "privileged_features_available",
    "labels",
    "sanitized_provenance"
  ]);
  assertOnlyFields(provenance, ["capture_source", "layout_hash", "capture_spec_hash", "map_hash"]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    capture_id: idField(record, "capture_id"),
    frame: u64Field(record, "frame"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    labelable: booleanField(record, "labelable"),
    preview_image_url: patternStringField(record, "preview_image_url", CAPTURE_PREVIEW_PATTERN),
    privileged_features_available: booleanField(record, "privileged_features_available"),
    labels: uniqueArray(arrayField(record, "labels").map((label) => enumValue(label, LABEL_ROLES))),
    sanitized_provenance: {
      capture_source: enumField(provenance, "capture_source", ["synthetic", "hypervisor"] as const),
      layout_hash: hashField(provenance, "layout_hash"),
      capture_spec_hash: hashField(provenance, "capture_spec_hash"),
      map_hash: hashField(provenance, "map_hash")
    }
  };
}

function parseLabelsResponse(value: unknown): LabelsResponse {
  const record = schemaRecord(value);
  assertOnlyFields(record, ["schema_version", "applied", "label_revision", "conflicts"]);
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
  assertOnlyFields(record, [
    "schema_version",
    "label_revision",
    "target_labels",
    "status_labels",
    "dedup_groups"
  ]);
  assertOnlyFields(targets, ["first_boss", "goal_positive", "goal_negative"]);
  return {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    label_revision: u64Field(record, "label_revision"),
    target_labels: {
      first_boss: nullableIdField(targets, "first_boss"),
      goal_positive: nullableIdField(targets, "goal_positive"),
      goal_negative: nullableIdField(targets, "goal_negative")
    },
    status_labels: arrayField(record, "status_labels").map((entry) => {
      const status = recordValue(entry);
      assertOnlyFields(status, ["capture_id", "status"]);
      return {
        capture_id: idField(status, "capture_id"),
        status: enumField(status, "status", ["needs_review", "rejected"] as const)
      };
    }),
    dedup_groups: arrayField(record, "dedup_groups").map((entry) => {
      const group = recordValue(entry);
      assertOnlyFields(group, [
        "group_id",
        "expected_relation",
        "capture_ids",
        "changed_features",
        "changed_offset_ranges",
        "status"
      ]);
      const changed_features =
        group["changed_features"] === undefined
          ? undefined
          : uniqueArray(arrayField(group, "changed_features").map((feature) => boundedStringValue(feature, 1, 128)));
      const changed_offset_ranges =
        group["changed_offset_ranges"] === undefined
          ? undefined
          : arrayField(group, "changed_offset_ranges").map((range) => {
              const offsetRange = recordValue(range);
              assertOnlyFields(offsetRange, ["start", "len"]);
              return {
                start: u64Field(offsetRange, "start"),
                len: u64Field(offsetRange, "len")
              };
            });
      if (changed_features === undefined && changed_offset_ranges === undefined) {
        throw new Error("expected dedup change details");
      }
      return {
        group_id: idField(group, "group_id"),
        expected_relation: enumField(group, "expected_relation", [
          "same_canonical_state",
          "distinct_stable_state"
        ] as const),
        capture_ids: minLengthArray(uniqueArray(arrayField(group, "capture_ids").map(idValue)), 2),
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
  assertOnlyFields(record, [
    "capture_id",
    "frame",
    "status",
    "labelable",
    "has_preview",
    "labels",
    "created_at"
  ]);
  return {
    capture_id: idField(record, "capture_id"),
    frame: u64Field(record, "frame"),
    status: enumField(record, "status", CAPTURE_STATUSES),
    labelable: booleanField(record, "labelable"),
    has_preview: booleanField(record, "has_preview"),
    labels: uniqueArray(arrayField(record, "labels").map((label) => enumValue(label, LABEL_ROLES))),
    created_at: rfc3339Field(record, "created_at")
  };
}

function parseWsMessage(value: unknown): RuntimeWsMessage {
  const record = schemaRecord(value);
  const type = enumField(record, "type", INBOUND_WS_MESSAGE_TYPES);
  if (type === "input_ack") {
    const payload = recordField(record, "payload");
    assertOnlyFields(record, [
      "schema_version",
      "type",
      "session_id",
      "client_seq",
      "source_id",
      "server_seq",
      "payload"
    ]);
    assertOnlyFields(payload, ["client_event_id", "assigned_frame", "pad_word", "status"]);
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      type,
      session_id: idField(record, "session_id"),
      client_seq: u64Field(record, "client_seq"),
      source_id: sourceIdField(record, "source_id"),
      server_seq: nullableU64Field(record, "server_seq"),
      payload: {
        client_event_id: uuidField(payload, "client_event_id"),
        assigned_frame: u64Field(payload, "assigned_frame"),
        pad_word: boundedIntegerField(payload, "pad_word", 0, PAD_WORD_MAX),
        status: enumField(payload, "status", ["applied", "queued", "dropped"] as const)
      }
    };
  }
  if (type === "input_reject") {
    const payload = schemaRecord(record["payload"]);
    assertOnlyFields(record, [
      "schema_version",
      "type",
      "session_id",
      "client_seq",
      "source_id",
      "server_seq",
      "payload"
    ]);
    return {
      schema_version: RUNTIME_API_SCHEMA_VERSION,
      type,
      session_id: idField(record, "session_id"),
      client_seq: u64Field(record, "client_seq"),
      source_id: sourceIdField(record, "source_id"),
      server_seq: nullableU64Field(record, "server_seq"),
      payload: {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        error: parseErrorObject(payload["error"])
      }
    };
  }
  assertOnlyFields(record, [
    "schema_version",
    "type",
    "session_id",
    "client_seq",
    "source_id",
    "server_seq",
    "payload"
  ]);
  const eventBase = {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    type,
    session_id: idField(record, "session_id"),
    client_seq: null,
    source_id: enumField(record, "source_id", ["server"] as const),
    server_seq: u64Field(record, "server_seq")
  };
  if (record["client_seq"] !== null) {
    throw new Error("expected null client_seq");
  }
  switch (type) {
    case "session_updated":
      return { ...eventBase, type, payload: parseSessionUpdatedPayload(record["payload"]) };
    case "run_updated":
      return { ...eventBase, type, payload: parseRunUpdatedPayload(record["payload"]) };
    case "capture_updated":
      return { ...eventBase, type, payload: parseCaptureUpdatedPayload(record["payload"]) };
    case "label_updated":
      return { ...eventBase, type, payload: parseLabelUpdatedPayload(record["payload"]) };
    case "validation_updated":
      return { ...eventBase, type, payload: parseValidationUpdatedPayload(record["payload"]) };
  }
}

function parseSessionUpdatedPayload(value: unknown): SessionUpdatedPayload {
  const payload = recordValue(value);
  assertOnlyFields(payload, ["state", "backend_mode", "current_frame", "capabilities"]);
  return {
    state: enumField(payload, "state", SESSION_STATES),
    backend_mode: enumField(payload, "backend_mode", BACKEND_MODES),
    current_frame: u64Field(payload, "current_frame"),
    capabilities: parseCapabilities(payload["capabilities"])
  };
}

function parseRunUpdatedPayload(value: unknown): RunUpdatedPayload {
  const payload = recordValue(value);
  assertOnlyFields(payload, ["state", "current_frame", "preview_stale", "active_capture_job_id"]);
  return {
    state: enumField(payload, "state", SESSION_STATES),
    current_frame: u64Field(payload, "current_frame"),
    preview_stale: booleanField(payload, "preview_stale"),
    active_capture_job_id: nullableIdField(payload, "active_capture_job_id")
  };
}

function parseCaptureUpdatedPayload(value: unknown): CaptureUpdatedPayload {
  const payload = recordValue(value);
  assertOnlyFields(payload, ["job_id", "status", "capture_id"]);
  return {
    job_id: idField(payload, "job_id"),
    status: enumField(payload, "status", CAPTURE_STATUSES),
    capture_id: nullableIdField(payload, "capture_id")
  };
}

function parseLabelUpdatedPayload(value: unknown): LabelUpdatedPayload {
  const payload = recordValue(value);
  assertOnlyFields(payload, ["label_revision", "applied"]);
  return {
    label_revision: u64Field(payload, "label_revision"),
    applied: booleanField(payload, "applied")
  };
}

function parseValidationUpdatedPayload(value: unknown): ValidationUpdatedPayload {
  const payload = recordValue(value);
  assertOnlyFields(payload, ["status", "summary"]);
  return {
    status: enumField(payload, "status", VALIDATION_STATUSES),
    summary: boundedStringField(payload, "summary", 0, 240)
  };
}

function parsePadLayout(value: unknown): StartSessionResponse["pad_layout"] {
  const record = recordValue(value);
  assertOnlyFields(record, ["layout_id", "layout_version"]);
  return {
    layout_id: literalField(record, "layout_id", PAD_LAYOUT_ID),
    layout_version: literalField(record, "layout_version", PAD_LAYOUT_VERSION)
  };
}

function parseCapabilities(value: unknown): RuntimeCapabilities {
  const record = recordValue(value);
  assertOnlyFields(record, CAPABILITY_NAMES);
  return Object.fromEntries(
    CAPABILITY_NAMES.map((capability) => [capability, booleanField(record, capability)])
  ) as RuntimeCapabilities;
}

function parseErrorObject(value: unknown): RuntimeErrorDisplay {
  const record = recordValue(value);
  assertOnlyFields(record, ["code", "message", "retryable", "details"]);
  recordField(record, "details");
  return sanitizeDisplayError({
    code: enumField(record, "code", ERROR_CODES),
    message: boundedStringField(record, "message", 1, 240),
    retryable: booleanField(record, "retryable"),
    details: {}
  });
}

function errorFromPayload(payload: unknown, status: number): RuntimeApiError {
  if (!isRecord(payload) || payload["schema_version"] !== RUNTIME_API_SCHEMA_VERSION) {
    return runtimeError("bad_request", "Runtime schema mismatch.", false, status);
  }
  try {
    assertOnlyFields(payload, ["schema_version", "error"]);
    return new RuntimeApiError(parseErrorObject(payload["error"]), status);
  } catch {
    return runtimeError("bad_request", "Runtime schema mismatch.", false, status);
  }
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

function idField(record: JsonRecord, key: string): string {
  return idValue(record[key]);
}

function nullableIdField(record: JsonRecord, key: string): string | null {
  return record[key] === null ? null : idField(record, key);
}

function idValue(value: unknown): string {
  const candidate = stringValue(value);
  if (!ID_PATTERN.test(candidate)) {
    throw new Error("expected runtime id");
  }
  return candidate;
}

function sourceIdField(record: JsonRecord, key: string): string {
  return boundedStringField(record, key, 1, 64);
}

function uuidField(record: JsonRecord, key: string): string {
  const candidate = stringField(record, key);
  if (!UUID_PATTERN.test(candidate)) {
    throw new Error("expected uuid");
  }
  return candidate;
}

function hashField(record: JsonRecord, key: string): string {
  return boundedStringField(record, key, 1, 256);
}

function rfc3339Field(record: JsonRecord, key: string): string {
  const candidate = stringField(record, key);
  if (!RFC3339_PATTERN.test(candidate) || Number.isNaN(Date.parse(candidate))) {
    throw new Error("expected rfc3339 timestamp");
  }
  return candidate;
}

function patternStringField(record: JsonRecord, key: string, pattern: RegExp): string {
  const candidate = stringField(record, key);
  if (!pattern.test(candidate)) {
    throw new Error("expected patterned string");
  }
  return candidate;
}

function boundedStringField(
  record: JsonRecord,
  key: string,
  minLength: number,
  maxLength: number
): string {
  return boundedStringValue(record[key], minLength, maxLength);
}

function boundedStringValue(value: unknown, minLength: number, maxLength: number): string {
  if (typeof value !== "string" || value.length < minLength || value.length > maxLength) {
    throw new Error("expected bounded string");
  }
  return value;
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

function boundedIntegerField(record: JsonRecord, key: string, min: number, max: number): number {
  const value = u64Field(record, key);
  if (value < min || value > max) {
    throw new Error("integer out of range");
  }
  return value;
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

function uniqueArray<T extends string>(values: T[]): T[] {
  if (new Set(values).size !== values.length) {
    throw new Error("expected unique array");
  }
  return values;
}

function minLengthArray<T>(values: T[], minLength: number): T[] {
  if (values.length < minLength) {
    throw new Error("array too short");
  }
  return values;
}

function assertOnlyFields(record: JsonRecord, allowed: readonly string[]): void {
  for (const key of Object.keys(record)) {
    if (!allowed.includes(key)) {
      throw new Error("unexpected field");
    }
  }
}

function enumValueOrDefault<const T extends readonly string[]>(
  value: string,
  values: T,
  fallback: T[number]
): T[number] {
  return values.includes(value) ? (value as T[number]) : fallback;
}

function createRuntimeClientEventId(): string {
  const randomUuid = globalThis.crypto?.randomUUID?.();
  if (randomUuid) {
    return randomUuid;
  }
  return "00000000-0000-4000-8000-000000000000";
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
