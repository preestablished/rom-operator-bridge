import { beforeAll, describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { build } from "vite";

const UI_ROOT = new URL("..", import.meta.url).pathname;
const SOURCE_ROOT = join(UI_ROOT, "src");
const PUBLIC_ROOT = join(UI_ROOT, "public");
const DIST_ROOT = join(UI_ROOT, "dist");
const PERSISTENCE_API_PATTERN =
  /serviceWorker|caches|localStorage|sessionStorage|indexedDB|CacheStorage/i;

describe("browser privacy boundaries", () => {
  beforeAll(async () => {
    await build({ root: UI_ROOT, logLevel: "silent" });
  });

  it("does not register service workers or browser persistence APIs in static surfaces", () => {
    const staticSurfaceText = [
      readFileSync(join(UI_ROOT, "index.html"), "utf8"),
      ...readTextFiles(SOURCE_ROOT),
      ...readTextFiles(PUBLIC_ROOT),
      ...readTextFiles(DIST_ROOT)
    ].join("\n");
    expect(staticSurfaceText).not.toMatch(PERSISTENCE_API_PATTERN);
  });

  it("keeps runtime config free of secret-shaped keys", () => {
    const configText = readFileSync(join(PUBLIC_ROOT, "runtime-config.json"), "utf8");
    expect(configText).not.toMatch(/credential|password|secret|token|private/i);
  });
});

function readTextFiles(root: string): string[] {
  return readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      return readTextFiles(path);
    }
    if (/\.(css|html|js|json|ts)$/.test(entry)) {
      return [readFileSync(path, "utf8")];
    }
    return [];
  });
}
