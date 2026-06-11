import {
  type FileViewerImplId,
  hexFileViewerImplId,
  textFileViewerImplId,
} from "../impl/index.ts";
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
  "pdf",
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
  "md",
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

const textFilenames = new Set([
  ".editorconfig",
  ".env",
  ".gitattributes",
  ".gitignore",
  "dockerfile",
  "license",
  "makefile",
  "readme",
]);

export async function detectFileViewerImpl(
  machine: Machine | undefined,
  fsEntry: FsEntry,
): Promise<DetectFileViewerImplResult> {
  const nameHint = detectFileViewerImplFromName(fsEntry);
  if (!machine) {
    return {
      initialBytes: new Uint8Array(),
      impl: nameHint ?? textFileViewerImplId,
    };
  }

  try {
    const initialBytes = await readFile(machine, fsEntry.path, {
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
      impl: nameHint ?? textFileViewerImplId,
    };
  }
}

function detectFileViewerImplFromBytes(
  bytes: Uint8Array,
  nameHint: FileViewerImplId | undefined,
): FileViewerImplId {
  const sample = bytes.subarray(0, Math.min(bytes.length, sampleByteCount));
  if (sample.includes(0)) return hexFileViewerImplId;

  let controls = 0;
  for (const byte of sample) {
    const isTextControl = byte === 9 || byte === 10 || byte === 13;
    if (byte < 32 && !isTextControl) controls++;
  }
  return sample.length > 0 &&
      controls / sample.length > binaryControlRatioThreshold
    ? hexFileViewerImplId
    : nameHint ?? textFileViewerImplId;
}

function detectFileViewerImplFromName(
  fsEntry: FsEntry,
): FileViewerImplId | undefined {
  const basename = fileBasename(fsEntry.name || fsEntry.path).toLowerCase();
  if (textFilenames.has(basename)) return textFileViewerImplId;

  const extension = fileExtension(basename);
  if (!extension) return undefined;
  if (binaryExtensions.has(extension)) return hexFileViewerImplId;
  if (textExtensions.has(extension)) return textFileViewerImplId;
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
