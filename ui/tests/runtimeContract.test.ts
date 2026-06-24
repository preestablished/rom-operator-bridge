import { describe, expect, it } from "vitest";
import {
  BACKEND_MODES,
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  RUNTIME_API_SCHEMA_VERSION,
  SESSION_STATES
} from "../src/runtimeContract";

describe("runtime contract constants", () => {
  it("comes from the shared schema", () => {
    expect(RUNTIME_API_SCHEMA_VERSION).toBe(1);
    expect(PAD_LAYOUT_ID).toBe("console16-12btn-v1");
    expect(PAD_LAYOUT_VERSION).toBe(1);
    expect(BACKEND_MODES).toEqual(["synthetic", "real"]);
    expect(SESSION_STATES).toEqual([
      "idle",
      "starting",
      "running",
      "paused",
      "capture_pending",
      "stopping",
      "stopped",
      "faulted"
    ]);
  });
});
