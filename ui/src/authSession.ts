import {
  RuntimeApiError,
  initialRuntimeSessionModel,
  modelFromStartSession,
  type RuntimeApiClient,
  type RuntimeErrorDisplay,
  type RuntimeSessionModel,
  type SessionResponse
} from "./runtimeClient";

export type AuthSessionStatus =
  | "locked"
  | "starting"
  | "active"
  | "expired"
  | "auth_rejected"
  | "origin_rejected"
  | "session_active_elsewhere"
  | "stopping"
  | "faulted";

export type RuntimeSessionClient = Pick<
  RuntimeApiClient,
  "startSession" | "sessionStatus" | "stopSession"
>;

export type AuthSessionState = {
  status: AuthSessionStatus;
  session: RuntimeSessionModel;
  error: RuntimeErrorDisplay | null;
};

export function initialAuthSessionState(): AuthSessionState {
  return {
    status: "locked",
    session: initialRuntimeSessionModel(),
    error: null
  };
}

export async function submitCredential(
  state: AuthSessionState,
  client: RuntimeSessionClient,
  operatorCredential: string
): Promise<AuthSessionState> {
  if (operatorCredential.length === 0) {
    return {
      ...state,
      status: "auth_rejected",
      error: {
        code: "auth_rejected",
        message: "Authentication rejected.",
        retryable: false,
        details: {}
      }
    };
  }

  try {
    const response = await client.startSession({
      operatorCredential,
      requestedCapabilities: ["input", "preview", "capture", "labels"]
    });
    return {
      status: "active",
      session: modelFromStartSession(response),
      error: null
    };
  } catch (error) {
    return stateFromRuntimeError(state, error);
  }
}

export async function refreshSession(
  state: AuthSessionState,
  client: RuntimeSessionClient
): Promise<AuthSessionState> {
  try {
    return stateFromSessionResponse(await client.sessionStatus());
  } catch (error) {
    return stateFromRuntimeError(state, error);
  }
}

export async function logoutSession(
  state: AuthSessionState,
  client: RuntimeSessionClient
): Promise<AuthSessionState> {
  if (!state.session.session_id) {
    return initialAuthSessionState();
  }

  try {
    await client.stopSession(state.session.session_id);
    return initialAuthSessionState();
  } catch (error) {
    return stateFromRuntimeError({ ...state, status: "stopping" }, error);
  }
}

function stateFromSessionResponse(response: SessionResponse): AuthSessionState {
  if (!response.active) {
    return initialAuthSessionState();
  }

  return {
    status: "active",
    session: {
      ...initialRuntimeSessionModel(),
      active: true,
      session_id: response.session_id,
      run_id: response.run_id,
      state: response.state,
      backend_mode: response.backend_mode,
      current_frame: response.current_frame,
      preview_stale: true
    },
    error: null
  };
}

function stateFromRuntimeError(state: AuthSessionState, error: unknown): AuthSessionState {
  if (!(error instanceof RuntimeApiError)) {
    return {
      ...state,
      status: "faulted",
      error: {
        code: "backend_unavailable",
        message: "Runtime unavailable.",
        retryable: true,
        details: {}
      }
    };
  }

  const status = statusFromErrorCode(error.display.code);
  const clearSession =
    status === "auth_rejected" ||
    status === "expired" ||
    status === "origin_rejected" ||
    status === "session_active_elsewhere";

  return {
    ...state,
    status,
    session: clearSession ? initialRuntimeSessionModel() : state.session,
    error: error.display
  };
}

function statusFromErrorCode(code: RuntimeErrorDisplay["code"]): AuthSessionStatus {
  switch (code) {
    case "auth_rejected":
      return "auth_rejected";
    case "session_active_elsewhere":
      return "session_active_elsewhere";
    case "session_inactive":
      return "expired";
    case "origin_rejected":
      return "origin_rejected";
    default:
      return "faulted";
  }
}
