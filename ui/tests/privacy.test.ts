import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const UI_ROOT = new URL("..", import.meta.url).pathname;
const SOURCE_ROOT = join(UI_ROOT, "src");
const PUBLIC_ROOT = join(UI_ROOT, "public");

describe("browser privacy boundaries", () => {
  it("does not register service workers or browser persistence APIs", () => {
    const sourceText = readTextFiles(SOURCE_ROOT).join("\n");
    expect(sourceText).not.toMatch(/serviceWorker|caches|localStorage|indexedDB/i);
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
    if (entry.endsWith(".ts") || entry.endsWith(".css")) {
      return [readFileSync(path, "utf8")];
    }
    return [];
  });
}
