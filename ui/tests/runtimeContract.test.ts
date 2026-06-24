import { describe, expect, it } from "vitest";
import {
  PAD_LAYOUT_ID,
  PAD_LAYOUT_VERSION,
  RUNTIME_API_SCHEMA_VERSION
} from "../src/runtimeContract";

describe("runtime contract constants", () => {
  it("comes from the shared schema", () => {
    expect(RUNTIME_API_SCHEMA_VERSION).toBe(1);
    expect(PAD_LAYOUT_ID).toBe("console16-12btn-v1");
    expect(PAD_LAYOUT_VERSION).toBe(1);
  });
});
