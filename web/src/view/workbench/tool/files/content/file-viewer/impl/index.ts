import type { ComponentType } from "react";
import { hexFileViewerImpl } from "./hex/index.tsx";
import { textFileViewerImpl } from "./text/index.tsx";

export interface FileViewerImpl {
  id: string;
  label: string;
  viewerName: string;
  Component: ComponentType;
}

export const fileViewerImpls = [
  textFileViewerImpl,
  hexFileViewerImpl,
] as const satisfies readonly FileViewerImpl[];

export type FileViewerImplId = (typeof fileViewerImpls)[number]["id"];

export const textFileViewerImplId = fileViewerImpls[0].id;
export const hexFileViewerImplId = fileViewerImpls[1].id;

const fileViewerImplById = new Map<string, FileViewerImpl>(
  fileViewerImpls.map((impl) => [impl.id, impl]),
);

export function getFileViewerImpl(
  impl: FileViewerImplId,
): FileViewerImpl {
  const definition = fileViewerImplById.get(impl);
  if (!definition) throw new Error(`Unknown file viewer impl: ${impl}`);
  return definition;
}

export function isFileViewerImpl(value: string): value is FileViewerImplId {
  return fileViewerImplById.has(value);
}
