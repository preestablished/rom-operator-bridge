import { RUNTIME_API_SCHEMA_VERSION } from "./runtimeContract";

export type RuntimeConfig = {
  schema_version: 1;
  api_base_path: string;
  ws_base_path: string;
  allow_persistence: false;
};

export const DEFAULT_RUNTIME_CONFIG: RuntimeConfig = {
  schema_version: RUNTIME_API_SCHEMA_VERSION,
  api_base_path: "/api",
  ws_base_path: "/ws",
  allow_persistence: false
};

const FORBIDDEN_CONFIG_KEYS = [
  "credential",
  "password",
  "private",
  "secret",
  "token"
];

export function normalizeRuntimeConfig(input: unknown): RuntimeConfig {
  if (!isRecord(input)) {
    return DEFAULT_RUNTIME_CONFIG;
  }
  if (
    containsForbiddenMaterial(input) ||
    input.schema_version !== RUNTIME_API_SCHEMA_VERSION ||
    input.allow_persistence !== false
  ) {
    return DEFAULT_RUNTIME_CONFIG;
  }

  const apiBasePath = sameOriginPath(input.api_base_path);
  const wsBasePath = sameOriginPath(input.ws_base_path);
  if (!apiBasePath || !wsBasePath) {
    return DEFAULT_RUNTIME_CONFIG;
  }

  const candidate: RuntimeConfig = {
    schema_version: RUNTIME_API_SCHEMA_VERSION,
    api_base_path: apiBasePath,
    ws_base_path: wsBasePath,
    allow_persistence: false
  };

  return isRuntimeConfigSafe(candidate) ? candidate : DEFAULT_RUNTIME_CONFIG;
}

export async function loadRuntimeConfig(fetcher: typeof fetch = fetch): Promise<RuntimeConfig> {
  try {
    const response = await fetcher("/runtime-config.json", {
      cache: "no-store",
      credentials: "same-origin"
    });
    if (!response.ok) {
      return DEFAULT_RUNTIME_CONFIG;
    }
    return normalizeRuntimeConfig(await response.json());
  } catch {
    return DEFAULT_RUNTIME_CONFIG;
  }
}

export function isRuntimeConfigSafe(config: RuntimeConfig): boolean {
  const serialized = JSON.stringify(config).toLowerCase();
  return (
    !FORBIDDEN_CONFIG_KEYS.some((key) => serialized.includes(key)) &&
    config.allow_persistence === false &&
    config.schema_version === RUNTIME_API_SCHEMA_VERSION &&
    sameOriginPath(config.api_base_path) !== null &&
    sameOriginPath(config.ws_base_path) !== null
  );
}

function sameOriginPath(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0) {
    return null;
  }
  if (!value.startsWith("/") || value.startsWith("//")) {
    return null;
  }
  if (
    value.includes("..") ||
    value.includes("\\") ||
    !/^\/[A-Za-z0-9/_:.-]*$/.test(value)
  ) {
    return null;
  }
  return value;
}

function containsForbiddenMaterial(value: unknown): boolean {
  if (typeof value === "string") {
    return includesForbidden(value);
  }
  if (Array.isArray(value)) {
    return value.some(containsForbiddenMaterial);
  }
  if (isRecord(value)) {
    return Object.entries(value).some(
      ([key, entry]) => includesForbidden(key) || containsForbiddenMaterial(entry)
    );
  }
  return false;
}

function includesForbidden(value: string): boolean {
  const normalized = value.toLowerCase();
  return FORBIDDEN_CONFIG_KEYS.some((key) => normalized.includes(key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
