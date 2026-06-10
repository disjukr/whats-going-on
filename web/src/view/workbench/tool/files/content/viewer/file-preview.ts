import { formatSize } from "../../../../../../state/explorer.ts";
import type { FilePreview } from "./types.ts";

export function decodeFilePreview(bytes: Uint8Array): FilePreview {
  if (looksBinary(bytes)) {
    return { kind: "binary", text: hexPreview(bytes) };
  }

  try {
    return {
      kind: "text",
      text: new TextDecoder("utf-8", { fatal: true }).decode(bytes),
    };
  } catch {
    return { kind: "binary", text: hexPreview(bytes) };
  }
}

function looksBinary(bytes: Uint8Array): boolean {
  const sample = bytes.subarray(0, Math.min(bytes.length, 4096));
  if (sample.includes(0)) return true;
  let controls = 0;
  for (const byte of sample) {
    const isTextControl = byte === 9 || byte === 10 || byte === 13;
    if (byte < 32 && !isTextControl) controls++;
  }
  return sample.length > 0 && controls / sample.length > 0.08;
}

function hexPreview(bytes: Uint8Array): string {
  const previewLength = Math.min(bytes.length, 4096);
  const lines: string[] = [];
  for (let offset = 0; offset < previewLength; offset += 16) {
    const chunk = bytes.subarray(offset, Math.min(offset + 16, previewLength));
    const hex = Array.from(chunk)
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(47, " ");
    const ascii = Array.from(chunk)
      .map((byte) =>
        byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : "."
      )
      .join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  if (bytes.length > previewLength) {
    lines.push(`... ${formatSize(bytes.length - previewLength)} more`);
  }
  return lines.join("\n");
}
