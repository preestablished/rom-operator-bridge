import { describe, expect, it } from "vitest";
import {
  DEFAULT_RUNTIME_CONFIG,
  isRuntimeConfigSafe,
  normalizeRuntimeConfig
} from "../src/runtimeConfig";

describe("runtime config", () => {
  it("defaults to same-origin API and websocket bases", () => {
    expect(DEFAULT_RUNTIME_CONFIG).toEqual({
      schema_version: 1,
      api_base_path: "/api",
      ws_base_path: "/ws",
      allow_persistence: false
    });
    expect(isRuntimeConfigSafe(DEFAULT_RUNTIME_CONFIG)).toBe(true);
  });

  it("rejects absolute, cross-origin, and secret-shaped config values", () => {
    expect(
      normalizeRuntimeConfig({
        schema_version: 1,
        api_base_path: "https://example.invalid/api",
        ws_base_path: "//example.invalid/ws",
        allow_persistence: true
      })
    ).toEqual(DEFAULT_RUNTIME_CONFIG);

    expect(
      normalizeRuntimeConfig({
        schema_version: 1,
        api_base_path: "/api/private-token",
        ws_base_path: "/ws",
        allow_persistence: false
      })
    ).toEqual(DEFAULT_RUNTIME_CONFIG);
  });

  it("keeps safe same-origin overrides", () => {
    expect(
      normalizeRuntimeConfig({
        schema_version: 1,
        api_base_path: "/api",
        ws_base_path: "/ws/events",
        allow_persistence: false
      })
    ).toEqual({
      schema_version: 1,
      api_base_path: "/api",
      ws_base_path: "/ws/events",
      allow_persistence: false
    });
  });
});
