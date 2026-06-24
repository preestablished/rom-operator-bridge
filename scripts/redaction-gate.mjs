#!/usr/bin/env node
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";

const TEXT_EXTENSIONS = new Set([
  ".css",
  ".html",
  ".js",
  ".json",
  ".md",
  ".mjs",
  ".svg",
  ".toml",
  ".ts",
  ".txt",
  ".webmanifest",
  ".xml",
  ".yaml",
  ".yml"
]);

const SCAN_TARGETS = [
  "README.md",
  "contracts",
  "docs",
  "ui/README.md",
  "ui/index.html",
  "ui/public",
  "ui/dist"
];

const STATIC_ASSET_ROOTS = ["ui/public", "ui/dist"];

const PATTERNS = [
  {
    kind: "rom_path",
    pattern:
      /(?:^|[\s:=("'`])(?:\/[^\s"'`<>)]+|[A-Za-z]:\\[^\s"'`<>)]+)?(?:rom|private|corpus)[^\s"'`<>)]+\.(?:sfc|smc|fig|swc|nes|gb|gbc|gba|zip)\b/i
  },
  {
    kind: "private_corpus_root",
    pattern:
      /(?:^|[\s:=("'`])(?:\/(?:srv|mnt|Volumes)\/(?:corpus|private)\b|\/run\/(?:private|secret|rom|operator)\b|[A-Za-z]:\\(?:Users\\rombridge|private)\b)/i
  },
  {
    kind: "operator_credential",
    pattern:
      /operator-secret|Bearer\s+[A-Za-z0-9._-]{8,}|(?:operator_credential|credential|password|secret|token)\s*[:=]\s*["'][^"'<]{6,}["']/i
  },
  {
    kind: "real_capture_id",
    pattern:
      /\b(?:real|phase4|hypervisor)-capture-[A-Za-z0-9._:-]+\b|\bcapture-[0-9a-f]{8,}\b/i
  },
  {
    kind: "screenshot_or_preview_cache",
    pattern:
      /data:image\/(?:png|jpeg|webp);base64,|iVBORw0KGgo|\/9j\/4AAQ|(?:screenshot|framebuffer|preview[-_]?cache)[^\s"'`<>)]+\.(?:png|jpe?g|webp|raw|bin|lz4)\b/i
  },
  {
    kind: "source_map_private_path",
    pattern:
      /sourceMappingURL=|sourcesContent|webpack:\/\/|vite:\/\/|(?:\/(?:home|Users|srv|mnt|Volumes)\/|[A-Za-z]:\\)[^\s"'`<>)]+\.map\b/i
  },
  {
    kind: "validation_excerpt",
    pattern:
      /validation report excerpt:\s*\S|verifier (?:stdout|stderr):\s*\S|(?:stack trace|traceback):\s*\S|panic at [^\s"'`<>)]+/i
  },
  {
    kind: "private_network_literal",
    pattern:
      /\b(?:10(?:\.\d{1,3}){3}|192\.168(?:\.\d{1,3}){2}|172\.(?:1[6-9]|2\d|3[01])(?:\.\d{1,3}){2})\b|(?:^|[^0-9a-f])(?:fd[0-9a-f]{2}|fe80):/i
  }
];

function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === "scan") {
    scanCommand(args);
    return;
  }
  if (command === "self-test") {
    selfTest();
    return;
  }

  throw new Error("usage: redaction-gate.mjs <scan|self-test> [options]");
}

function scanCommand(args) {
  const options = parseOptions(args);
  const root = resolve(options.root ?? process.cwd());
  const aggregatePath = requiredOption(options.aggregate, "--aggregate");
  const summaryPath = requiredOption(options.summary, "--summary");
  const files = collectScanFiles(root);
  const findings = [];
  const aggregateChunks = [];

  for (const file of files) {
    const relPath = relative(root, file).replaceAll("\\", "/");
    const bytes = readFileSync(file);
    if (bytes.includes(0)) {
      if (isStaticAsset(relPath)) {
        findings.push(finding("binary_or_blob_static_asset", relPath, 1, 1));
      }
      continue;
    }
    if (!TEXT_EXTENSIONS.has(extname(file)) && relPath !== "README.md") {
      if (isStaticAsset(relPath)) {
        findings.push(finding("binary_or_blob_static_asset", relPath, 1, 1));
      }
      continue;
    }

    const text = bytes.toString("utf8");
    aggregateChunks.push(`\n--- BEGIN ${relPath} ---\n${text}\n--- END ${relPath} ---\n`);
    findings.push(...scanText(relPath, text));
  }

  mkdirSync(dirname(aggregatePath), { recursive: true });
  mkdirSync(dirname(summaryPath), { recursive: true });
  writeFileSync(aggregatePath, aggregateChunks.join(""), "utf8");
  writeFileSync(
    summaryPath,
    `${JSON.stringify(
      {
        schema_version: 1,
        status: findings.length === 0 ? "pass" : "fail",
        scanned_file_count: files.length,
        finding_count: findings.length,
        counts_by_kind: countsByKind(findings),
        findings
      },
      null,
      2
    )}\n`,
    "utf8"
  );

  if (findings.length > 0) {
    process.exitCode = 1;
  }
}

function selfTest() {
  const samples = [
    ["rom_path", "loaded /srv/corpus/private/operator-rom.sfc"],
    ["private_corpus_root", "private root: /mnt/private/corpus"],
    ["operator_credential", 'operator_credential: "operator-secret"'],
    ["real_capture_id", "real-capture-9f86d081884c7d65"],
    ["screenshot_or_preview_cache", "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA"],
    ["source_map_private_path", "sourceMappingURL=/home/operator/build/index.js.map"],
    ["validation_excerpt", "validation report excerpt: verifier stderr leaked"],
    ["private_network_literal", "connect to 10.0.0.106:7410"]
  ];

  for (const [kind, text] of samples) {
    const findings = scanText(`${kind}.txt`, text);
    if (!findings.some((entry) => entry.kind === kind)) {
      throw new Error(`redaction self-test missed ${kind}`);
    }
  }

  const cleanFindings = scanText(
    "clean.md",
    "Status: pass\nPrivate values are referenced only as <redacted> placeholders.\n"
  );
  if (cleanFindings.length !== 0) {
    throw new Error("redaction self-test flagged clean placeholder text");
  }
}

function parseOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index];
    const value = args[index + 1];
    if (!key?.startsWith("--") || value === undefined) {
      throw new Error(`invalid option sequence near ${key ?? "<end>"}`);
    }
    options[key.slice(2)] = value;
  }
  return options;
}

function requiredOption(value, name) {
  if (!value) {
    throw new Error(`${name} is required`);
  }
  return value;
}

function collectScanFiles(root) {
  const files = [];
  for (const target of SCAN_TARGETS) {
    const path = join(root, target);
    collect(path, files);
  }
  return files.sort();
}

function collect(path, files) {
  let stat;
  try {
    stat = statSync(path);
  } catch (error) {
    if (error?.code === "ENOENT") {
      return;
    }
    throw error;
  }

  if (stat.isDirectory()) {
    for (const entry of readdirSync(path)) {
      collect(join(path, entry), files);
    }
    return;
  }

  if (stat.isFile()) {
    files.push(path);
  }
}

function isStaticAsset(relPath) {
  return STATIC_ASSET_ROOTS.some((root) => relPath === root || relPath.startsWith(`${root}/`));
}

function scanText(path, text) {
  const findings = [];
  const lines = text.split(/\r?\n/);
  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const line = lines[lineIndex];
    for (const { kind, pattern } of PATTERNS) {
      const match = pattern.exec(line);
      if (match) {
        findings.push(finding(kind, path, lineIndex + 1, match.index + 1));
      }
    }
  }
  return findings;
}

function finding(kind, path, line, column) {
  return { kind, path, line, column };
}

function countsByKind(findings) {
  return findings.reduce((counts, item) => {
    counts[item.kind] = (counts[item.kind] ?? 0) + 1;
    return counts;
  }, {});
}

main();
