import { beforeAll, describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { build } from "vite";

const UI_ROOT = new URL("..", import.meta.url).pathname;
const SOURCE_ROOT = join(UI_ROOT, "src");
const PUBLIC_ROOT = join(UI_ROOT, "public");
const DIST_ROOT = join(UI_ROOT, "dist");
const PERSISTENCE_AND_DOWNLOAD_API_PATTERN =
  /\b(serviceWorker|caches|CacheStorage|localStorage|sessionStorage|indexedDB|navigator\.storage|document\.cookie|cookieStore|openDatabase|requestFileSystem|webkitRequestFileSystem|showSaveFilePicker|showDirectoryPicker|createObjectURL|revokeObjectURL|FileReader)\b|\.download\b|\sdownload\s*=/i;
const SOURCE_TEXT_EXTENSIONS = /\.(css|html|js|json|ts)$/;
const DEPLOYABLE_TEXT_EXTENSIONS = /\.(css|html|js|mjs|cjs|json|txt|svg|xml|webmanifest)$/i;
const FORBIDDEN_STATIC_PAYLOADS = [
  {
    label: "private absolute path",
    pattern:
      /(?:^|[\s:=("'`])(?:\/(?:home|Users|opt|srv|mnt|Volumes|tmp)\/|\/var\/(?:lib|log|tmp|run|secrets?)\/|\/run\/(?:dh|private|secret|rom|operator)\/|[A-Za-z]:\\(?:Users|rombridge|private)\\)[^\s"'`<>)]+/i
  },
  {
    label: "operator credential or token value",
    pattern:
      /operator-secret|(?:operator_credential|credential|password|secret|token)\s*[:=]\s*["']?(?![A-Za-z_$]{1,3}\.)[A-Za-z0-9._:/+=-]{6,}|Bearer\s+[A-Za-z0-9._-]{8,}/i
  },
  {
    label: "real-run capture id",
    pattern: /\b(?:real|phase4|hypervisor)-capture-[A-Za-z0-9._:-]+\b|\bcapture-[0-9a-f]{8,}\b/i
  },
  {
    label: "preview image payload",
    pattern: /data:image\/(?:png|jpeg|webp);base64,|iVBORw0KGgo|\/9j\/4AAQSkZJRgABAQ/i
  },
  {
    label: "decoded feature values",
    pattern: /(?:feature_bytes|feature_values|decoded_features|feature_vector)\s*[:=]|\b[0-9a-f]{2}(?:\s+[0-9a-f]{2}){15,}\b/i
  },
  {
    label: "validation report excerpt",
    pattern:
      /validation report excerpt:\s*\S|verifier (?:stdout|stderr):\s*\S|(?:stack trace|traceback):\s*\S|panic at [^\s"'`<>)]+/i
  },
  {
    label: "source map artifact",
    pattern: /sourceMappingURL=|sourcesContent|webpack:\/\/|vite:\/\/|\.map(?:["')\s]|$)/i
  }
] as const;

type TextFile = {
  path: string;
  text: string;
};

describe("browser privacy boundaries", () => {
  beforeAll(async () => {
    await build({ root: UI_ROOT, logLevel: "silent" });
  });

  it("does not register service workers, browser persistence, downloads, or preview-cache APIs", () => {
    const apiSurfaces = [
      { path: join(UI_ROOT, "index.html"), text: readFileSync(join(UI_ROOT, "index.html"), "utf8") },
      ...readTextFiles(SOURCE_ROOT, SOURCE_TEXT_EXTENSIONS),
      ...readDeployableTextFiles(PUBLIC_ROOT),
      ...readDeployableTextFiles(DIST_ROOT)
    ];
    expect(matchesFor(apiSurfaces, PERSISTENCE_AND_DOWNLOAD_API_PATTERN)).toEqual([]);
  });

  it("keeps deployable static output free of private runtime payloads", () => {
    const deployableFiles = deployableStaticFiles();

    for (const payload of FORBIDDEN_STATIC_PAYLOADS) {
      expect(matchesFor(deployableFiles, payload.pattern), payload.label).toEqual([]);
    }
  });

  it("rejects deployable binary or blob-like static assets", () => {
    const deployablePaths = [...readAllFiles(PUBLIC_ROOT), ...readAllFiles(DIST_ROOT)];
    expect(
      deployablePaths
        .filter((path) => !DEPLOYABLE_TEXT_EXTENSIONS.test(path))
        .map((path) => path.replace(UI_ROOT, "ui/"))
    ).toEqual([]);
  });

  it("keeps source maps out of deployable output", () => {
    const distPaths = readAllFiles(DIST_ROOT);
    expect(distPaths.filter((path) => path.endsWith(".map"))).toEqual([]);
    expect(readFileSync(join(UI_ROOT, "vite.config.ts"), "utf8")).toMatch(/sourcemap:\s*false/);
  });

  it("keeps runtime config free of secret-shaped keys", () => {
    const configText = readFileSync(join(PUBLIC_ROOT, "runtime-config.json"), "utf8");
    expect(configText).not.toMatch(/credential|password|secret|token|private/i);
    expect(JSON.parse(configText)).toMatchObject({ allow_persistence: false });
  });

  it("recognizes forbidden private payload samples", () => {
    const sampleFiles = [
      textFile("private-path.txt", "opened /home/operator/private.env"),
      textFile("private-path-opt.txt", "path=/opt/rombridge/private/run.json"),
      textFile("private-path-srv.txt", "root: /srv/corpus/private"),
      textFile("private-path-mnt.txt", "bundle=/mnt/private/capture.bin"),
      textFile("private-path-volume.txt", "report: /Volumes/private/report.json"),
      textFile("credential.txt", '"operator_credential": "operator-secret"'),
      textFile("token.txt", "token: abcdef123456"),
      textFile("capture-id.txt", "real-capture-9f86d081884c7d65"),
      textFile("preview.txt", "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA"),
      textFile("features.txt", "feature_values: [0.12, 0.34]"),
      textFile("validation.txt", "validation report excerpt: verifier stderr leaked"),
      textFile("source-map.txt", "sourceMappingURL=assets/index.js.map\nsourcesContent")
    ];

    for (const payload of FORBIDDEN_STATIC_PAYLOADS) {
      expect(matchesFor(sampleFiles, payload.pattern), payload.label).not.toEqual([]);
    }
    expect(PERSISTENCE_AND_DOWNLOAD_API_PATTERN.test("document.cookie = 'token=abc'")).toBe(true);
    expect(PERSISTENCE_AND_DOWNLOAD_API_PATTERN.test("cookieStore.set('token', 'abc')")).toBe(true);
    expect(PERSISTENCE_AND_DOWNLOAD_API_PATTERN.test("openDatabase('bridge')")).toBe(true);
    expect(PERSISTENCE_AND_DOWNLOAD_API_PATTERN.test("webkitRequestFileSystem")).toBe(true);
    expect(DEPLOYABLE_TEXT_EXTENSIONS.test("capture-preview.png")).toBe(false);
    expect(DEPLOYABLE_TEXT_EXTENSIONS.test("private-report.pdf")).toBe(false);
    expect(DEPLOYABLE_TEXT_EXTENSIONS.test("font-cache.woff2")).toBe(false);
  });
});

function deployableStaticFiles(): TextFile[] {
  return [
    { path: join(UI_ROOT, "index.html"), text: readFileSync(join(UI_ROOT, "index.html"), "utf8") },
    ...readDeployableTextFiles(PUBLIC_ROOT),
    ...readDeployableTextFiles(DIST_ROOT)
  ];
}

function readTextFiles(root: string, textExtensions: RegExp): TextFile[] {
  return readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      return readTextFiles(path, textExtensions);
    }
    if (textExtensions.test(entry)) {
      return [{ path, text: readFileSync(path, "utf8") }];
    }
    return [];
  });
}

function readDeployableTextFiles(root: string): TextFile[] {
  return readAllFiles(root)
    .filter((path) => DEPLOYABLE_TEXT_EXTENSIONS.test(path))
    .flatMap((path) => {
      const text = readFileSync(path, "utf8");
      return text.includes("\u0000") ? [] : [{ path, text }];
    });
}

function readAllFiles(root: string): string[] {
  return readdirSync(root).flatMap((entry) => {
    const path = join(root, entry);
    return statSync(path).isDirectory() ? readAllFiles(path) : [path];
  });
}

function matchesFor(files: TextFile[], pattern: RegExp): string[] {
  return files
    .filter((file) => pattern.test(file.text))
    .map((file) => file.path.replace(UI_ROOT, "ui/"));
}

function textFile(path: string, text: string): TextFile {
  return { path: join(UI_ROOT, path), text };
}
