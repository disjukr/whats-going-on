import React, { useEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import {
  HardDrive,
  Info,
  KeyRound,
  Loader2,
  SquareTerminal,
  Trash2,
  X,
} from "lucide-react";
import {
  type AvailableShellInfo,
  type AvailableShellsTableEvent,
  DeleteMode,
  deletePaths,
  FsEntry,
  FsEntryKind,
  subscribeAvailableShells,
} from "../../../../protocol/rpc.ts";
import { connectionBunja } from "../../../../state/connection.ts";
import {
  displayName,
  explorerBunja,
  ExplorerPaneScope,
} from "../../../../state/explorer.ts";
import { machineModalBunja } from "../../../../state/machine-modal.ts";
import { machineStoreBunja } from "../../../../state/machine-store.ts";
import {
  workbenchBunja,
  workbenchTabBunja,
} from "../../../../state/workbench.ts";
import {
  type FilesActions,
  FilesActionsContext,
  FilesExplorerContext,
} from "./context.tsx";
import { Button } from "../../../ui/button.tsx";
import { FilesContent } from "./content/index.tsx";
import { EntryPropertiesModal } from "./content/directory/index.tsx";
import { FilesNavbar } from "./navbar/index.tsx";
import {
  clampFloatingMenuPosition,
  FloatingMenu,
  FloatingMenuItem,
} from "../../../ui/floating-menu.tsx";

const emptyWorkspaceClassName = [
  "grid content-center justify-items-center w-full h-full gap-[10px]",
  "min-h-0 text-[#667085]",
  "[&_h2]:m-0 [&_h2]:text-[#303642] [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");
const explorerClassName = [
  "grid [grid-template-rows:auto_minmax(0,1fr)]",
  "w-full h-full min-h-0 overflow-hidden",
].join(" ");
const entryContextMenuWidth = 176;
const modalBackdropClassName =
  "fixed inset-0 z-[20] grid place-items-center bg-[rgb(32_36_45_/_42%)] p-[24px]";
const modalClassName = [
  "w-[min(460px,100%)] overflow-hidden border border-[#d8dde7]",
  "rounded-[8px] bg-white [box-shadow:0_24px_72px_rgb(32_36_45_/_28%)]",
].join(" ");
const modalHeadClassName = [
  "flex items-center justify-between gap-[12px] border-b border-b-[#e4e8ef]",
  "px-[16px] py-[14px]",
  "[&_div]:grid [&_div]:gap-[2px] [&_div]:min-w-0",
  "[&_span]:text-[#667085] [&_span]:text-[12px] [&_span]:font-700",
  "[&_h2]:m-0 [&_h2]:text-[#20242d] [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");
const iconButtonClassName = "!w-[36px] !min-w-[36px] !p-0";
const deleteDialogBodyClassName = [
  "grid gap-[12px] p-[16px]",
  "[&_p]:m-0 [&_p]:text-[#475467] [&_p]:text-[13px] [&_p]:leading-[1.45]",
].join(" ");
const entrySummaryClassName = [
  "grid gap-[2px] min-w-0 border border-[#d8dde7] rounded-[8px]",
  "bg-[#f7f8fb] p-[10px]",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:text-[#20242d] [&_strong]:text-[13px]",
  "[&_span]:min-w-0 [&_span]:[overflow-wrap:anywhere]",
  "[&_span]:text-[#667085] [&_span]:text-[12px]",
].join(" ");
const modalActionsClassName = "flex justify-end gap-[8px]";
const dangerActionClassName = [
  "border-[#f6c2bd] bg-[#fff4f2] text-[#b42318]",
  "hover:border-[#f04438] hover:bg-[#fff2f0] hover:text-[#912018]",
].join(" ");
const fieldErrorClassName = "text-[#b42318] text-[12px]";

interface EntryMenuState {
  entry: FsEntry;
  x: number;
  y: number;
}

interface FolderMenuState {
  x: number;
  y: number;
}

interface DeleteEntryState {
  entry: FsEntry;
  error?: string;
  isDeleting: boolean;
}

interface DeleteEntryModalProps {
  state: DeleteEntryState;
  onClose: () => void;
  onDelete: () => void;
}

export function FilesTool() {
  const connectionState = useBunja(connectionBunja);
  const machineModal = useBunja(machineModalBunja);
  const machineStore = useBunja(machineStoreBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const isPaired = useAtomValue(machineStore.selectedIsPairedAtom);
  const connectionEpoch = useAtomValue(connectionState.connectionEpochAtom);
  const workbench = useBunja(workbenchBunja);
  const tabState = useBunja(workbenchTabBunja);
  const explorer = useBunja(explorerBunja, [
    ExplorerPaneScope.bind(tabState.tabId),
  ]);
  const openedFile = useAtomValue(explorer.openedFileAtom);
  const lastConnectionEpochRef = useRef(connectionEpoch);
  const [entryMenu, setEntryMenu] = useState<EntryMenuState>();
  const [folderMenu, setFolderMenu] = useState<FolderMenuState>();
  const [propertiesEntry, setPropertiesEntry] = useState<FsEntry>();
  const [deleteEntry, setDeleteEntry] = useState<DeleteEntryState>();
  const [terminalShells, setTerminalShells] = useState<AvailableShellInfo[]>(
    [],
  );

  useEffect(() => {
    if (lastConnectionEpochRef.current === connectionEpoch) return;
    lastConnectionEpochRef.current = connectionEpoch;
    explorer.refresh();
  }, [connectionEpoch, explorer]);

  useEffect(() => {
    if (!entryMenu && !folderMenu) return;

    function closeMenu() {
      setEntryMenu(undefined);
      setFolderMenu(undefined);
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
  }, [entryMenu, folderMenu]);

  useEffect(() => {
    if (!machine || !isPaired) {
      setTerminalShells([]);
      return;
    }

    let cancelled = false;
    const iterator = subscribeAvailableShells(
      machine,
      machineStore.rpcCallOptions(),
    );
    void (async () => {
      try {
        for await (const event of iterator) {
          if (cancelled) return;
          if (event.type === "snapshot") {
            setTerminalShells(event.rows);
          } else {
            setTerminalShells((current) =>
              applyAvailableShellPatch(current, event)
            );
          }
        }
      } catch {
        if (!cancelled) setTerminalShells([]);
      }
    })();

    return () => {
      cancelled = true;
      void iterator.return(undefined);
    };
  }, [connectionEpoch, isPaired, machine, machineStore]);

  useEffect(() => {
    if (!propertiesEntry) return;

    function closeModalOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setPropertiesEntry(undefined);
    }

    globalThis.addEventListener("keydown", closeModalOnEscape);
    return () => globalThis.removeEventListener("keydown", closeModalOnEscape);
  }, [propertiesEntry]);

  useEffect(() => {
    if (!deleteEntry || deleteEntry.isDeleting) return;

    function closeModalOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setDeleteEntry(undefined);
    }

    globalThis.addEventListener("keydown", closeModalOnEscape);
    return () => globalThis.removeEventListener("keydown", closeModalOnEscape);
  }, [deleteEntry]);

  const {
    currentPathAtom,
    goBack,
    goUp,
    navigate,
    openEntry,
    openFile,
    selectEntry,
  } = explorer;
  const currentPath = useAtomValue(currentPathAtom);

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
    setFolderMenu(undefined);
    setEntryMenu({
      entry,
      ...entryContextMenuPosition(
        event.clientX,
        event.clientY,
        Boolean(currentPath),
      ),
    });
  }

  function openFolderMenu(event: React.MouseEvent<HTMLDivElement>) {
    event.preventDefault();
    event.stopPropagation();
    setEntryMenu(undefined);
    setFolderMenu(
      folderContextMenuPosition(event.clientX, event.clientY),
    );
  }

  function openEntryProperties(entry: FsEntry) {
    setEntryMenu(undefined);
    setPropertiesEntry(entry);
  }

  function openDeleteEntry(entry: FsEntry) {
    setEntryMenu(undefined);
    setDeleteEntry({ entry, isDeleting: false });
  }

  function openTerminalHere() {
    setFolderMenu(undefined);
    if (!currentPath) return;
    const shell = terminalShells.find((item) => item.isDefault) ??
      terminalShells[0];
    if (!shell) return;
    workbench.openTerminalTab({
      cwd: currentPath,
      launch: {
        command: shell.command,
        args: shell.args,
      },
      title: shell.name,
    });
  }

  async function deleteSelectedEntry() {
    if (!machine || !deleteEntry || deleteEntry.isDeleting) return;
    setDeleteEntry((current) =>
      current ? { ...current, error: undefined, isDeleting: true } : current
    );
    try {
      const result = await deletePaths(
        machine,
        [deleteEntry.entry.path],
        DeleteMode.Trash,
        machineStore.rpcCallOptions(),
      );
      const failure = result.results.find((item) => !item.ok);
      if (failure && !failure.ok) {
        setDeleteEntry((current) =>
          current
            ? {
              ...current,
              error: failure.message || failure.code,
              isDeleting: false,
            }
            : current
        );
        return;
      }
      setDeleteEntry(undefined);
    } catch (err) {
      setDeleteEntry((current) =>
        current
          ? {
            ...current,
            error: err instanceof Error ? err.message : String(err),
            isDeleting: false,
          }
          : current
      );
    }
  }

  const actions: FilesActions = {
    goBack: goBackFromToolbar,
    goUp: goUpFromToolbar,
    navigate: navigateFromToolbar,
    openEntry: openTableEntry,
    openEntryMenu,
    openFolderMenu,
    selectEntry,
  };

  if (!machine) {
    return (
      <section className={emptyWorkspaceClassName}>
        <HardDrive size={28} />
        <h2>No machine selected</h2>
      </section>
    );
  }

  if (!isPaired) {
    return (
      <section className={emptyWorkspaceClassName}>
        <KeyRound size={28} />
        <h2>Pairing required</h2>
        <Button
          onClick={() => machineModal.openPairMachineModal(machine.id)}
        >
          <KeyRound size={16} />
          Pair
        </Button>
      </section>
    );
  }

  return (
    <section className={explorerClassName}>
      <FilesExplorerContext value={explorer}>
        <FilesActionsContext value={actions}>
          <FilesNavbar />

          <FilesContent />
        </FilesActionsContext>
      </FilesExplorerContext>

      {entryMenu
        ? (
          <FloatingMenu
            className="z-[30] w-[176px]"
            position={{ left: entryMenu.x, top: entryMenu.y }}
          >
            <FloatingMenuItem
              onClick={() => openEntryProperties(entryMenu.entry)}
            >
              <Info size={15} />
              Properties
            </FloatingMenuItem>
            {currentPath
              ? (
                <FloatingMenuItem
                  danger
                  onClick={() => openDeleteEntry(entryMenu.entry)}
                >
                  <Trash2 size={15} />
                  Delete...
                </FloatingMenuItem>
              )
              : null}
          </FloatingMenu>
        )
        : null}

      {folderMenu
        ? (
          <FloatingMenu
            className="z-[30] w-[176px]"
            position={{ left: folderMenu.x, top: folderMenu.y }}
          >
            <FloatingMenuItem
              disabled={!currentPath || terminalShells.length === 0}
              onClick={openTerminalHere}
            >
              <SquareTerminal size={15} />
              Open terminal here
            </FloatingMenuItem>
          </FloatingMenu>
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

      {deleteEntry
        ? (
          <DeleteEntryModal
            state={deleteEntry}
            onClose={() => {
              if (!deleteEntry.isDeleting) setDeleteEntry(undefined);
            }}
            onDelete={deleteSelectedEntry}
          />
        )
        : null}
    </section>
  );
}

function entryContextMenuPosition(
  x: number,
  y: number,
  hasDeleteItem: boolean,
): { x: number; y: number } {
  const position = clampFloatingMenuPosition(x, y, {
    itemCount: hasDeleteItem ? 2 : 1,
    width: entryContextMenuWidth,
  });
  return { x: position.left, y: position.top };
}

function folderContextMenuPosition(
  x: number,
  y: number,
): { x: number; y: number } {
  const position = clampFloatingMenuPosition(x, y, {
    itemCount: 1,
    width: entryContextMenuWidth,
  });
  return { x: position.left, y: position.top };
}

function applyAvailableShellPatch(
  current: AvailableShellInfo[],
  event: Extract<AvailableShellsTableEvent, { type: "patch" }>,
): AvailableShellInfo[] {
  const removeIds = new Set(event.removes.map((item) => item.shellId));
  const retained = current.filter((shell) => !removeIds.has(shell.shellId));
  const upserts = new Map(event.upserts.map((shell) => [shell.shellId, shell]));
  return [
    ...retained.filter((shell) => !upserts.has(shell.shellId)),
    ...event.upserts,
  ];
}

function DeleteEntryModal(
  { state, onClose, onDelete }: DeleteEntryModalProps,
) {
  return (
    <div
      className={modalBackdropClassName}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !state.isDeleting) {
          onClose();
        }
      }}
    >
      <section
        className={modalClassName}
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-entry-title"
      >
        <header className={modalHeadClassName}>
          <div>
            <span>File</span>
            <h2 id="delete-entry-title">Delete</h2>
          </div>
          <Button
            onClick={onClose}
            title="Close"
            aria-label="Close delete dialog"
            disabled={state.isDeleting}
            className={iconButtonClassName}
          >
            <X size={16} />
          </Button>
        </header>

        <div className={deleteDialogBodyClassName}>
          <div className={entrySummaryClassName}>
            <strong>{displayName(state.entry)}</strong>
            <span>{state.entry.path}</span>
          </div>
          <p>
            This moves the selected item to the system trash.
          </p>
          {state.error
            ? <div className={fieldErrorClassName}>{state.error}</div>
            : null}
          <div className={modalActionsClassName}>
            <Button onClick={onClose} disabled={state.isDeleting}>
              Cancel
            </Button>
            <Button
              className={dangerActionClassName}
              onClick={onDelete}
              disabled={state.isDeleting}
            >
              {state.isDeleting
                ? <Loader2 size={16} className="animate-spin" />
                : <Trash2 size={16} />}
              Delete
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}
