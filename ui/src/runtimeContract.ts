import runtimeApiSchema from "../../contracts/runtime-api.schema.json";

type JsonObject = Record<string, unknown>;

const defs = (runtimeApiSchema as JsonObject)["$defs"] as JsonObject;
const schemaVersion = defs["schemaVersion"] as JsonObject;
const backendMode = defs["backendMode"] as JsonObject;
const sessionState = defs["sessionState"] as JsonObject;
const capabilityName = defs["capabilityName"] as JsonObject;
const captureStatus = defs["captureStatus"] as JsonObject;
const labelRole = defs["labelRole"] as JsonObject;
const inputSource = defs["inputSource"] as JsonObject;
const padButton = defs["padButton"] as JsonObject;
const wsMessageType = defs["wsMessageType"] as JsonObject;
const errorCode = defs["errorCode"] as JsonObject;
const padLayout = defs["padLayout"] as JsonObject;
const padLayoutProperties = padLayout["properties"] as JsonObject;
const padLayoutId = padLayoutProperties["layout_id"] as JsonObject;
const padLayoutVersion = padLayoutProperties["layout_version"] as JsonObject;

export const RUNTIME_API_SCHEMA_VERSION = schemaVersion["const"] as 1;
export const PAD_LAYOUT_ID = padLayoutId["const"] as "console16-12btn-v1";
export const PAD_LAYOUT_VERSION = padLayoutVersion["const"] as 1;
export const BACKEND_MODES = enumValues(backendMode, "backendMode");
export const SESSION_STATES = enumValues(sessionState, "sessionState");
export const CAPABILITY_NAMES = enumValues(capabilityName, "capabilityName") as readonly [
  "input",
  "preview",
  "capture",
  "labels",
  "privileged_features",
  "validation_runner"
];
export const CAPTURE_STATUSES = enumValues(captureStatus, "captureStatus") as readonly [
  "requested",
  "capturing",
  "completed",
  "failed",
  "not_labelable"
];
export const LABEL_ROLES = enumValues(labelRole, "labelRole") as readonly [
  "first_boss",
  "goal_positive",
  "goal_negative",
  "needs_review",
  "rejected"
];
export const INPUT_SOURCES = enumValues(inputSource, "inputSource") as readonly [
  "keyboard",
  "gamepad",
  "combined"
];
export const PAD_BUTTONS = enumValues(padButton, "padButton") as readonly [
  "A",
  "B",
  "X",
  "Y",
  "L",
  "R",
  "Up",
  "Down",
  "Left",
  "Right",
  "Start",
  "Select"
];
export const WS_MESSAGE_TYPES = enumValues(wsMessageType, "wsMessageType") as readonly [
  "input_state",
  "input_ack",
  "input_reject",
  "session_updated",
  "run_updated",
  "capture_updated",
  "label_updated",
  "validation_updated"
];
export const ERROR_CODES = enumValues(errorCode, "errorCode") as readonly [
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
];

export type BackendMode = (typeof BACKEND_MODES)[number];
export type SessionState = (typeof SESSION_STATES)[number];

function enumValues(schema: JsonObject, name: string): readonly [string, ...string[]] {
  const values = schema["enum"];
  if (
    !Array.isArray(values) ||
    values.length === 0 ||
    !values.every((value) => typeof value === "string")
  ) {
    throw new Error(`runtime schema ${name} enum is missing or invalid`);
  }
  return values as [string, ...string[]];
}
