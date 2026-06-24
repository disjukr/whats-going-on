import type { FileViewerImplId } from "../impl/index.ts";
import { type FsEntry, readFile } from "../../../../../../../protocol/rpc.ts";
import type { Machine } from "../../../../../../../state/machines.ts";

const sampleByteCount = 4096;
const binaryControlRatioThreshold = 0.08;

interface DetectFileViewerImplResult {
  initialBytes: Uint8Array;
  impl: FileViewerImplId;
}

const binaryExtensions = new Set([
  "7z",
  "avif",
  "bmp",
  "class",
  "dll",
  "dmg",
  "doc",
  "docx",
  "exe",
  "gif",
  "gz",
  "ico",
  "jar",
  "jpeg",
  "jpg",
  "mov",
  "mp3",
  "mp4",
  "o",
  "obj",
  "png",
  "ppt",
  "pptx",
  "rar",
  "wasm",
  "webp",
  "xls",
  "xlsx",
  "zip",
]);

const textExtensions = new Set([
  "bat",
  "c",
  "cmd",
  "cpp",
  "cs",
  "css",
  "csv",
  "env",
  "go",
  "h",
  "hpp",
  "html",
  "java",
  "js",
  "json",
  "jsx",
  "kt",
  "lock",
  "log",
  "mjs",
  "ps1",
  "py",
  "rs",
  "sh",
  "sql",
  "svg",
  "toml",
  "ts",
  "tsx",
  "txt",
  "xml",
  "yaml",
  "yml",
]);

const markdownExtensions = new Set([
  "markdown",
  "md",
  "mdown",
  "mkd",
  "mkdn",
]);

const pdfExtensions = new Set([
  "pdf",
]);

const markdownFilenames = new Set([
  "readme",
]);

const textFilenames = new Set([
  ".editorconfig",
  ".env",
  ".gitattributes",
  ".gitignore",
  "dockerfile",
  "license",
  "makefile",
]);

export async function detectFileViewerImpl(
  machine: Machine | undefined,
  fsEntry: FsEntry,
  transport: WebTransport,
): Promise<DetectFileViewerImplResult> {
  const nameHint = detectFileViewerImplFromName(fsEntry);
  if (!machine) {
    return {
      initialBytes: new Uint8Array(),
      impl: nameHint ?? "text",
    };
  }

  try {
    const initialBytes = await readFile(transport, fsEntry.path, {
      offset: 0,
      length: sampleByteCount,
    });
    return {
      initialBytes,
      impl: detectFileViewerImplFromBytes(initialBytes, nameHint),
    };
  } catch {
    return {
      initialBytes: new Uint8Array(),
      impl: nameHint ?? "text",
    };
  }
}

function detectFileViewerImplFromBytes(
  bytes: Uint8Array,
  nameHint: FileViewerImplId | undefined,
): FileViewerImplId {
  if (nameHint === "pdf") return "pdf";

  const sample = bytes.subarray(0, Math.min(bytes.length, sampleByteCount));
  if (sample.includes(0)) return "hex";

  let controls = 0;
  for (const byte of sample) {
    const isTextControl = byte === 9 || byte === 10 || byte === 13;
    if (byte < 32 && !isTextControl) controls++;
  }
  return sample.length > 0 &&
      controls / sample.length > binaryControlRatioThreshold
    ? "hex"
    : nameHint ?? "text";
}

function detectFileViewerImplFromName(
  fsEntry: FsEntry,
): FileViewerImplId | undefined {
  const basename = fileBasename(fsEntry.name || fsEntry.path).toLowerCase();
  if (markdownFilenames.has(basename)) return "markdown";
  if (textFilenames.has(basename)) return "text";

  const extension = fileExtension(basename);
  if (!extension) return undefined;
  if (markdownExtensions.has(extension)) return "markdown";
  if (pdfExtensions.has(extension)) return "pdf";
  if (binaryExtensions.has(extension)) return "hex";
  if (textExtensions.has(extension)) return "text";
  return undefined;
}

function fileBasename(name: string): string {
  const normalized = name.trim().replace(/[\\/]+$/g, "");
  const slashIndex = Math.max(
    normalized.lastIndexOf("/"),
    normalized.lastIndexOf("\\"),
  );
  return normalized.slice(slashIndex + 1);
}

function fileExtension(basename: string): string | undefined {
  const dotIndex = basename.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === basename.length - 1) return undefined;
  return basename.slice(dotIndex + 1).toLowerCase();
}
