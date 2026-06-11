import { createContext } from "react";
import { bunja } from "bunja";
import { createScopeFromContext } from "bunja/react";
import { atom } from "jotai";
import { FsEntry } from "../../../../../../protocol/rpc.ts";
import { JotaiStoreScope } from "../../../../../../state/jotai-store.ts";
import { machineBunja } from "../../../../../../state/machine-store.ts";
import type { FileViewerImplId } from "./impl/index.ts";
import { detectFileViewerImpl } from "./impl-detector/index.ts";

type FileViewerState =
  | { phase: "detecting" }
  | { phase: "ready"; initialBytes: Uint8Array; impl: FileViewerImplId };

export const FsEntryContext = createContext<FsEntry | undefined>(undefined);
const FsEntryScope = createScopeFromContext(FsEntryContext);

export const fileViewerBunja = bunja(() => {
  const machine = bunja.use(machineBunja);
  const fsEntry = requireFsEntry(bunja.use(FsEntryScope));
  const store = bunja.use(JotaiStoreScope);

  const stateAtom = atom<FileViewerState>({ phase: "detecting" });
  const selectedImplAtom = atom<FileViewerImplId | undefined>(undefined);
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
      const result = await detectFileViewerImpl(currentMachine, fsEntry);
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
  };
});

function requireFsEntry(fsEntry: FsEntry | undefined): FsEntry {
  if (!fsEntry) throw new Error("FsEntry context is not provided.");
  return fsEntry;
}
