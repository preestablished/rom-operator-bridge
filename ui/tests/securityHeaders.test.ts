import { describe, expect, it } from "vitest";
import { build, preview } from "vite";
import { fileURLToPath } from "node:url";
import { SPA_RESPONSE_HEADERS } from "../vite.config";

const UI_ROOT = fileURLToPath(new URL("..", import.meta.url));

describe("SPA security headers", () => {
  it("configures no-store and browser isolation headers for preview responses", async () => {
    expect(SPA_RESPONSE_HEADERS).toEqual({
      "Cache-Control": "no-store",
      Pragma: "no-cache",
      "Content-Security-Policy":
        "default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'",
      "Referrer-Policy": "no-referrer",
      "X-Frame-Options": "DENY",
      "X-Content-Type-Options": "nosniff"
    });

    await build({ root: UI_ROOT, logLevel: "silent" });
    const server = await preview({
      root: UI_ROOT,
      logLevel: "silent",
      preview: {
        host: "127.0.0.1",
        port: 0,
        strictPort: false
      }
    });

    try {
      const address = server.httpServer.address();
      if (typeof address !== "object" || address === null) {
        throw new Error("preview server must bind a TCP address");
      }
      const baseUrl = `http://127.0.0.1:${address.port}`;

      for (const path of ["/", "/index.html", "/runtime-config.json"]) {
        const response = await fetch(`${baseUrl}${path}`);
        assertSecurityHeaders(response.headers);
      }
    } finally {
      await server.close();
    }
  });
});

function assertSecurityHeaders(headers: Headers) {
  expect(headers.get("cache-control")).toBe("no-store");
  expect(headers.get("pragma")).toBe("no-cache");
  expect(headers.get("content-security-policy")).toBe(
    "default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
  );
  expect(headers.get("referrer-policy")).toBe("no-referrer");
  expect(headers.get("x-frame-options")).toBe("DENY");
  expect(headers.get("x-content-type-options")).toBe("nosniff");
}
