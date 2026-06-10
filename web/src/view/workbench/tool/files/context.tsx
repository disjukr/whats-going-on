import { createContext } from "react";
import type React from "react";
import type { Atom, PrimitiveAtom } from "jotai";
import type { FsEntry } from "../../../../protocol/rpc.ts";

export interface FilesExplorerHistoryEntry {
  path?: string;
  openedFile?: FsEntry;
}

export interface FilesExplorerState {
  currentPathAtom: Atom<string | undefined>;
  displayPathAtom: Atom<string | undefined>;
  fileOpenPromptAtom: PrimitiveAtom<FsEntry | undefined>;
  historyAtom: Atom<FilesExplorerHistoryEntry[]>;
  openedFileAtom: Atom<FsEntry | undefined>;
  selectedEntryAtom: Atom<FsEntry | undefined>;
  selectedPathAtom: Atom<string | undefined>;
  visibleRowsAtom: Atom<FsEntry[]>;
  goBack: () => void;
  goUp: () => void;
  navigate: (path?: string) => void;
  openEntry: (entry: FsEntry) => void;
  openFile: (entry: FsEntry) => void;
  refresh: () => void;
  selectEntry: (entry: FsEntry) => void;
}

export interface FilesActions {
  cancelFileOpen: () => void;
  confirmFileOpen: () => void;
  goBack: () => void;
  goUp: () => void;
  navigate: (path?: string) => void;
  openEntry: (entry: FsEntry) => void;
  openEntryMenu: (
    entry: FsEntry,
    event: React.MouseEvent<HTMLButtonElement>,
  ) => void;
  selectEntry: (entry: FsEntry) => void;
}

export const FilesExplorerContext = createContext<
  FilesExplorerState | undefined
>(
  undefined,
);
export const FilesActionsContext = createContext<FilesActions | undefined>(
  undefined,
);

export function requireFilesExplorer(
  explorer: FilesExplorerState | undefined,
): FilesExplorerState {
  if (!explorer) throw new Error("Files explorer context is not provided.");
  return explorer;
}

export function requireFilesActions(
  actions: FilesActions | undefined,
): FilesActions {
  if (!actions) throw new Error("Files actions context is not provided.");
  return actions;
}
