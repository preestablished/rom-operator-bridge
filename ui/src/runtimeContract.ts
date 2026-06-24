import runtimeApiSchema from "../../contracts/runtime-api.schema.json";

type JsonObject = Record<string, unknown>;

const defs = (runtimeApiSchema as JsonObject)["$defs"] as JsonObject;
const schemaVersion = defs["schemaVersion"] as JsonObject;
const padLayout = defs["padLayout"] as JsonObject;
const padLayoutProperties = padLayout["properties"] as JsonObject;
const padLayoutId = padLayoutProperties["layout_id"] as JsonObject;
const padLayoutVersion = padLayoutProperties["layout_version"] as JsonObject;

export const RUNTIME_API_SCHEMA_VERSION = schemaVersion["const"] as 1;
export const PAD_LAYOUT_ID = padLayoutId["const"] as "console16-12btn-v1";
export const PAD_LAYOUT_VERSION = padLayoutVersion["const"] as 1;

export type SessionState =
  | "idle"
  | "starting"
  | "running"
  | "paused"
  | "capture_pending"
  | "stopping"
  | "stopped"
  | "faulted";

export type BackendMode = "synthetic" | "real";
