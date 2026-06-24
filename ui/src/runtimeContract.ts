import runtimeApiSchema from "../../contracts/runtime-api.schema.json";

type JsonObject = Record<string, unknown>;

const defs = (runtimeApiSchema as JsonObject)["$defs"] as JsonObject;
const schemaVersion = defs["schemaVersion"] as JsonObject;
const backendMode = defs["backendMode"] as JsonObject;
const sessionState = defs["sessionState"] as JsonObject;
const padLayout = defs["padLayout"] as JsonObject;
const padLayoutProperties = padLayout["properties"] as JsonObject;
const padLayoutId = padLayoutProperties["layout_id"] as JsonObject;
const padLayoutVersion = padLayoutProperties["layout_version"] as JsonObject;

export const RUNTIME_API_SCHEMA_VERSION = schemaVersion["const"] as 1;
export const PAD_LAYOUT_ID = padLayoutId["const"] as "console16-12btn-v1";
export const PAD_LAYOUT_VERSION = padLayoutVersion["const"] as 1;
export const BACKEND_MODES = enumValues(backendMode, "backendMode");
export const SESSION_STATES = enumValues(sessionState, "sessionState");

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
