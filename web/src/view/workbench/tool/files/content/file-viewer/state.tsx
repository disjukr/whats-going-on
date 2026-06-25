import { createContext } from "react";
import { bunja } from "bunja";
import { createScopeFromContext } from "bunja/react";
import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { JotaiStoreScope } from "unsaturated/store";
import { FsEntry, FsEntryKind } from "../../../../../../protocol/rpc.ts";
import { fileViewerSelectedImplStorageKey } from "../../../../../../state/file-viewer.ts";
import { machineBunja } from "../../../../../../state/machine-store.ts";
import { rpcSessionBunja } from "../../../../../../state/rpc-session.ts";
import { WorkbenchTabIdScope } from "../../../../../../state/workbench.ts";
import { type FileViewerImplId, isFileViewerImpl } from "./impl/index.ts";
import { detectFileViewerImpl } from "./impl-detector/index.ts";

type FileViewerState =
  | { phase: "detecting" }
  | { phase: "ready"; initialBytes: Uint8Array; impl: FileViewerImplId };

export const FsEntryContext = createContext<FsEntry | undefined>(undefined);
export const FsEntryPathContext = createContext<string | undefined>(undefined);
const FsEntryPathScope = createScopeFromContext(FsEntryPathContext);

export const fileViewerBunja = bunja(() => {
  const machine = bunja.use(machineBunja);
  const rpcSession = bunja.use(rpcSessionBunja);
  const tabId = requireWorkbenchTabId(bunja.use(WorkbenchTabIdScope));
  const fsEntryPath = requireFsEntryPath(bunja.use(FsEntryPathScope));
  const fsEntry = fsEntryFromPath(fsEntryPath);
  const store = bunja.use(JotaiStoreScope);

  const stateAtom = atom<FileViewerState>({ phase: "detecting" });
  const persistedSelectedImplAtom = atomWithStorage<string | undefined>(
    fileViewerSelectedImplStorageKey(machine.machineId, tabId, fsEntry.path),
    undefined,
    undefined,
    { getOnInit: true },
  );
  const selectedImplAtom = atom(
    (get) => {
      const selectedImpl = get(persistedSelectedImplAtom);
      return selectedImpl !== undefined && isFileViewerImpl(selectedImpl)
        ? selectedImpl
        : undefined;
    },
    (_get, set, impl: FileViewerImplId) => {
      set(persistedSelectedImplAtom, impl);
    },
  );
  const implAtom = atom(
    (get) => {
      const selectedImpl = get(selectedImplAtom);
      if (selectedImpl) return selectedImpl;

      const state = get(stateAtom);
      return state.phase === "ready" ? state.impl : undefined;
    },
    (_get, set, impl: FileViewerImplId) => {
      set(selectedImplAtom, impl);
    },
  );

  bunja.effect(() => {
    const currentMachine = store.get(machine.machineAtom);

    let cancelled = false;
    store.set(stateAtom, { phase: "detecting" });
    void (async () => {
      const transport = await rpcSession.webTransport();
      if (cancelled) return;
      const result = await detectFileViewerImpl(
        currentMachine,
        fsEntry,
        transport,
      );
      if (cancelled) return;
      store.set(stateAtom, {
        phase: "ready",
        initialBytes: result.initialBytes,
        impl: result.impl,
      });
    })();

    return () => {
      cancelled = true;
    };
  });

  return {
    fsEntry,
    implAtom,
    machineAtom: machine.machineAtom,
    stateAtom,
    tabId,
    webTransport: rpcSession.webTransport,
  };
});

export function requireFsEntry(fsEntry: FsEntry | undefined): FsEntry {
  if (!fsEntry) throw new Error("FsEntry context is not provided.");
  return fsEntry;
}

function requireFsEntryPath(path: string | undefined): string {
  if (!path) throw new Error("FsEntry path context is not provided.");
  return path;
}

function requireWorkbenchTabId(tabId: string | undefined): string {
  if (!tabId) throw new Error("Workbench tab id context is not provided.");
  return tabId;
}

function fsEntryFromPath(path: string): FsEntry {
  return {
    kind: FsEntryKind.File,
    name: fileBasename(path),
    path,
    readonly: false,
  };
}

function fileBasename(path: string): string {
  const slashIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return path.slice(slashIndex + 1) || path;
}
