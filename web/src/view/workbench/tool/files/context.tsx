import { createContext } from "react";
import type React from "react";
import type { Atom } from "jotai";
import type { FsEntry } from "../../../../protocol/generated/rpc.ts";
import type {
  ExplorerLocation,
  ExplorerSpecialLocation,
} from "../../../../state/explorer.ts";

export interface FilesExplorerState {
  currentPathAtom: Atom<string | undefined>;
  displayPathAtom: Atom<string | undefined>;
  historyAtom: Atom<ExplorerLocation[]>;
  openedFileAtom: Atom<FsEntry | undefined>;
  selectedEntryAtom: Atom<FsEntry | undefined>;
  selectedPathAtom: Atom<string | undefined>;
  specialLocationAtom: Atom<ExplorerSpecialLocation | undefined>;
  visibleRowsAtom: Atom<FsEntry[]>;
  goBack: () => void;
  goUp: () => void;
  navigate: (path?: string) => void;
  navigateTrash: () => void;
  openEntry: (entry: FsEntry) => void;
  openFile: (entry: FsEntry) => void;
  replaceWithPath: (path?: string) => void;
  replaceWithTrash: () => void;
  refresh: () => void;
  selectEntry: (entry: FsEntry) => void;
}

export interface FilesActions {
  goBack: () => void;
  goUp: () => void;
  navigate: (path?: string) => void;
  openEntry: (entry: FsEntry) => void;
  openEntryMenu: (
    entry: FsEntry,
    event: React.MouseEvent<HTMLDivElement>,
  ) => void;
  openFolderMenu: (event: React.MouseEvent<HTMLDivElement>) => void;
  selectEntry: (entry: FsEntry) => void;
}

export interface FilesRenameState {
  draftName: string;
  entryPath?: string;
  error?: string;
  isRenaming: boolean;
  cancelRename: () => void;
  commitRename: (entry: FsEntry) => void;
  updateDraftName: (value: string) => void;
}

export interface FilesCreateFileState {
  draftName: string;
  error?: string;
  isCreating: boolean;
  isEditing: boolean;
  cancelCreate: () => void;
  commitCreate: () => void;
  updateDraftName: (value: string) => void;
}

export const FilesExplorerContext = createContext<
  FilesExplorerState | undefined
>(
  undefined,
);
export const FilesActionsContext = createContext<FilesActions | undefined>(
  undefined,
);
export const FilesRenameContext = createContext<FilesRenameState | undefined>(
  undefined,
);
export const FilesCreateFileContext = createContext<
  FilesCreateFileState | undefined
>(
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

export function requireFilesRenameState(
  state: FilesRenameState | undefined,
): FilesRenameState {
  if (!state) throw new Error("Files rename context is not provided.");
  return state;
}

export function requireFilesCreateFileState(
  state: FilesCreateFileState | undefined,
): FilesCreateFileState {
  if (!state) throw new Error("Files create-file context is not provided.");
  return state;
}
