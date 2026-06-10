export type FilePreview =
  | { kind: "text"; text: string }
  | { kind: "binary"; text: string };

export type FileLoadState =
  | { phase: "loading" }
  | { phase: "ready"; byteLength: number; preview: FilePreview }
  | { phase: "error"; message: string };
