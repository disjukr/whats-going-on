import React, { useEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { HardDrive, Info, KeyRound } from "lucide-react";
import { FsEntry, FsEntryKind } from "../../../../protocol/rpc.ts";
import { connectionBunja } from "../../../../state/connection.ts";
import {
  explorerBunja,
  ExplorerPaneScope,
} from "../../../../state/explorer.ts";
import { machineModalBunja } from "../../../../state/machine-modal.ts";
import { machineStoreBunja } from "../../../../state/machine-store.ts";
import { workbenchTabBunja } from "../../../../state/workbench.ts";
import {
  type FilesActions,
  FilesActionsContext,
  FilesExplorerContext,
} from "./context.tsx";
import { FilesContent } from "./content/index.tsx";
import { EntryPropertiesModal } from "./content/directory/index.tsx";
import { FilesNavbar } from "./navbar/index.tsx";

interface EntryMenuState {
  entry: FsEntry;
  x: number;
  y: number;
}

export function FilesTool() {
  const connectionState = useBunja(connectionBunja);
  const machineModal = useBunja(machineModalBunja);
  const machineStore = useBunja(machineStoreBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const isPaired = useAtomValue(machineStore.selectedIsPairedAtom);
  const connectionEpoch = useAtomValue(connectionState.connectionEpochAtom);
  const tabState = useBunja(workbenchTabBunja);
  const explorer = useBunja(explorerBunja, [
    ExplorerPaneScope.bind(tabState.tabId),
  ]);
  const openedFile = useAtomValue(explorer.openedFileAtom);
  const lastConnectionEpochRef = useRef(connectionEpoch);
  const [entryMenu, setEntryMenu] = useState<EntryMenuState>();
  const [propertiesEntry, setPropertiesEntry] = useState<FsEntry>();

  useEffect(() => {
    if (lastConnectionEpochRef.current === connectionEpoch) return;
    lastConnectionEpochRef.current = connectionEpoch;
    explorer.refresh();
  }, [connectionEpoch, explorer]);

  useEffect(() => {
    if (!entryMenu) return;

    function closeMenu() {
      setEntryMenu(undefined);
    }

    function closeMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }

    globalThis.addEventListener("mousedown", closeMenu);
    globalThis.addEventListener("keydown", closeMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeMenu);
      globalThis.removeEventListener("keydown", closeMenuOnEscape);
    };
  }, [entryMenu]);

  useEffect(() => {
    if (!propertiesEntry) return;

    function closeModalOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setPropertiesEntry(undefined);
    }

    globalThis.addEventListener("keydown", closeModalOnEscape);
    return () => globalThis.removeEventListener("keydown", closeModalOnEscape);
  }, [propertiesEntry]);

  const {
    goBack,
    goUp,
    navigate,
    openEntry,
    openFile,
    selectEntry,
  } = explorer;

  function goBackFromToolbar() {
    goBack();
  }

  function goUpFromToolbar() {
    goUp();
  }

  function navigateFromToolbar(path?: string) {
    if (openedFile && path === openedFile.path) return;
    navigate(path);
  }

  function openTableEntry(entry: FsEntry) {
    if (entry.kind === FsEntryKind.Directory) {
      openEntry(entry);
      return;
    }
    if (entry.kind !== FsEntryKind.File) {
      selectEntry(entry);
      return;
    }

    selectEntry(entry);
    openFile(entry);
  }

  function openEntryMenu(
    entry: FsEntry,
    event: React.MouseEvent<HTMLButtonElement>,
  ) {
    event.preventDefault();
    event.stopPropagation();
    selectEntry(entry);
    setEntryMenu({ entry, x: event.clientX, y: event.clientY });
  }

  function openEntryProperties(entry: FsEntry) {
    setEntryMenu(undefined);
    setPropertiesEntry(entry);
  }

  const actions: FilesActions = {
    goBack: goBackFromToolbar,
    goUp: goUpFromToolbar,
    navigate: navigateFromToolbar,
    openEntry: openTableEntry,
    openEntryMenu,
    selectEntry,
  };

  if (!machine) {
    return (
      <section className="empty-workspace">
        <HardDrive size={28} />
        <h2>No machine selected</h2>
      </section>
    );
  }

  if (!isPaired) {
    return (
      <section className="empty-workspace">
        <KeyRound size={28} />
        <h2>Pairing required</h2>
        <button
          type="button"
          onClick={() => machineModal.openPairMachineModal(machine.id)}
        >
          <KeyRound size={16} />
          Pair
        </button>
      </section>
    );
  }

  return (
    <section className="explorer">
      <FilesExplorerContext value={explorer}>
        <FilesActionsContext value={actions}>
          <FilesNavbar />

          <FilesContent />
        </FilesActionsContext>
      </FilesExplorerContext>

      {entryMenu
        ? (
          <div
            className="entry-context-menu"
            style={{ left: entryMenu.x, top: entryMenu.y }}
            role="menu"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              onClick={() => openEntryProperties(entryMenu.entry)}
            >
              <Info size={15} />
              Properties
            </button>
          </div>
        )
        : null}

      {propertiesEntry
        ? (
          <EntryPropertiesModal
            entry={propertiesEntry}
            onClose={() => setPropertiesEntry(undefined)}
          />
        )
        : null}
    </section>
  );
}
