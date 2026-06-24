import { describe, expect, it } from "vitest";
import { renderOperatorApp } from "../src/app";
import type { RuntimeConfig } from "../src/runtimeConfig";

describe("operator app rendering", () => {
  it("escapes config-derived endpoint text", () => {
    const html = renderOperatorApp({
      schema_version: 1,
      api_base_path: "/api/<img src=x>",
      ws_base_path: "/ws/\"events\"",
      allow_persistence: false
    } as RuntimeConfig);

    expect(html).not.toContain("<img src=x>");
    expect(html).not.toContain('/ws/"events"');
    expect(html).toContain("/api/&lt;img src=x&gt;");
    expect(html).toContain("/ws/&quot;events&quot;");
  });
});
