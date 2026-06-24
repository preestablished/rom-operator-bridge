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

  const candidate: RuntimeConfig = {
    schema_version:
      input.schema_version === RUNTIME_API_SCHEMA_VERSION
        ? RUNTIME_API_SCHEMA_VERSION
        : DEFAULT_RUNTIME_CONFIG.schema_version,
    api_base_path: sameOriginPath(input.api_base_path, "/api"),
    ws_base_path: sameOriginPath(input.ws_base_path, "/ws"),
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
    config.api_base_path.startsWith("/") &&
    config.ws_base_path.startsWith("/")
  );
}

function sameOriginPath(value: unknown, fallback: string): string {
  if (typeof value !== "string" || value.length === 0) {
    return fallback;
  }
  if (!value.startsWith("/") || value.startsWith("//")) {
    return fallback;
  }
  if (value.includes("..") || value.includes("\\") || value.includes("?")) {
    return fallback;
  }
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
