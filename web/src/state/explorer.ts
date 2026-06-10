import { bunja, createScope } from "bunja";
import { atom, type PrimitiveAtom, type SetStateAction } from "jotai";
import { atomWithStorage } from "jotai/utils";
import {
  DirectoryTableEvent,
  FsEntry,
  FsEntryKind,
  RootsTableEvent,
  subscribeDirectory,
  subscribeRoots,
} from "../protocol/rpc.ts";
import { type JotaiStore, JotaiStoreScope } from "./jotai-store.ts";
import { MachineIdScope } from "./machine-id.tsx";
import { isPaired, machineStoreBunja } from "./machine-store.ts";
import { Machine } from "./machines.ts";
import { StreamState } from "./types.ts";

export const ExplorerPaneScope = createScope<string>();

interface ExplorerNavigationState {
  currentPath?: string;
  history: ExplorerHistoryEntry[];
  openedFile?: FsEntry;
}

interface ExplorerHistoryEntry {
  path?: string;
  openedFile?: FsEntry;
}

export const explorerMachineBunja = bunja(() => {
  const machineId = bunja.use(MachineIdScope);
  const machines = bunja.use(machineStoreBunja);

  const machineAtom = atom((get) =>
    get(machines.machinesAtom).find((machine) => machine.id === machineId) ??
      undefined
  );
  const isPairedAtom = atom((get) => isPaired(get(machineAtom)));
  const connectionKeyAtom = atom((get) =>
    explorerConnectionKey(get(machineAtom))
  );

  return {
    machineAtom,
    isPairedAtom,
    connectionKeyAtom,
  };
});

export const explorerRefreshBunja = bunja(() => {
  bunja.use(MachineIdScope);
  bunja.use(ExplorerPaneScope);
  const store = bunja.use(JotaiStoreScope);
  const refreshAtom = atom(0);

  function refresh() {
    store.set(refreshAtom, (current) => current + 1);
  }

  return {
    refreshAtom,
    refresh,
  };
});

export const explorerNavigationBunja = bunja(() => {
  const machineId = bunja.use(MachineIdScope);
  const paneScopeId = bunja.use(ExplorerPaneScope);
  const store = bunja.use(JotaiStoreScope);

  const navigationStateAtom = atomWithStorage<ExplorerNavigationState>(
    explorerNavigationStorageKey(machineId, paneScopeId),
    { history: [] },
    undefined,
    { getOnInit: true },
  );
  const currentPathAtom = atom(
    (get) => get(navigationStateAtom).currentPath,
    (get, set, update: SetStateAction<string | undefined>) => {
      const next = resolveSetStateAction(
        update,
        get(navigationStateAtom).currentPath,
      );
      set(navigationStateAtom, (current) => ({
        ...current,
        currentPath: next,
      }));
    },
  );
  const openedFileAtom = atom(
    (get) => get(navigationStateAtom).openedFile,
    (get, set, update: SetStateAction<FsEntry | undefined>) => {
      const next = resolveSetStateAction(
        update,
        get(navigationStateAtom).openedFile,
      );
      set(navigationStateAtom, (current) => ({
        ...current,
        openedFile: next,
      }));
    },
  );
  const displayPathAtom = atom((get) =>
    get(openedFileAtom)?.path ?? get(currentPathAtom)
  );
  const historyAtom = atom(
    (get) => get(navigationStateAtom).history,
    (get, set, update: SetStateAction<ExplorerHistoryEntry[]>) => {
      const currentHistory = get(navigationStateAtom).history;
      const next = resolveSetStateAction(update, currentHistory);
      set(navigationStateAtom, (current) => ({
        ...current,
        history: next,
      }));
    },
  );
  const selectedPathAtom = atom<string | undefined>(undefined);

  function selectEntry(entry: FsEntry) {
    store.set(selectedPathAtom, entry.path);
  }

  function navigate(path?: string) {
    store.set(
      historyAtom,
      (current) => [...current, currentLocation()],
    );
    store.set(currentPathAtom, path);
    store.set(openedFileAtom, undefined);
    store.set(selectedPathAtom, undefined);
  }

  function goBack() {
    const history = store.get(historyAtom);
    if (history.length === 0) return;
    const next = history[history.length - 1];
    store.set(currentPathAtom, next.path);
    store.set(openedFileAtom, next.openedFile);
    store.set(selectedPathAtom, next.openedFile?.path);
    store.set(historyAtom, history.slice(0, -1));
  }

  function goUp() {
    if (store.get(openedFileAtom)) {
      navigate(store.get(currentPathAtom));
      return;
    }
    const currentPath = store.get(currentPathAtom);
    if (!currentPath) return;
    navigate(parentPath(currentPath));
  }

  function openEntry(entry: FsEntry) {
    if (entry.kind === FsEntryKind.Directory) {
      navigate(entry.path);
      return;
    }
    store.set(selectedPathAtom, entry.path);
  }

  function openFile(entry: FsEntry) {
    store.set(
      historyAtom,
      (current) => [...current, currentLocation()],
    );
    store.set(openedFileAtom, entry);
    store.set(selectedPathAtom, entry.path);
  }

  function currentLocation(): ExplorerHistoryEntry {
    return {
      path: store.get(currentPathAtom),
      openedFile: store.get(openedFileAtom),
    };
  }

  return {
    currentPathAtom,
    displayPathAtom,
    historyAtom,
    openedFileAtom,
    selectedPathAtom,
    selectEntry,
    navigate,
    goBack,
    goUp,
    openEntry,
    openFile,
  };
});

export const explorerRootsBunja = bunja(() => {
  bunja.use(MachineIdScope);
  bunja.use(ExplorerPaneScope);
  const store = bunja.use(JotaiStoreScope);
  const machineState = bunja.use(explorerMachineBunja);
  const refresh = bunja.use(explorerRefreshBunja);

  const rootsAtom = atom<FsEntry[]>([]);
  const rootsStateAtom = atom<StreamState>({
    phase: "idle",
    message: "Roots idle",
  });
  const rootsSubscriptionKeyAtom = atom((get) =>
    [
      get(machineState.connectionKeyAtom),
      get(machineState.isPairedAtom) ? "paired" : "unpaired",
      get(refresh.refreshAtom),
    ].join("\n")
  );

  bunja.effect(() => {
    let stopCurrent: (() => void) | undefined;

    function start() {
      stopCurrent?.();
      stopCurrent = undefined;

      const machine = store.get(machineState.machineAtom);
      if (!machine || !store.get(machineState.isPairedAtom)) {
        store.set(rootsAtom, []);
        store.set(rootsStateAtom, { phase: "idle", message: "Roots idle" });
        return;
      }

      let cancelled = false;
      const iterator = subscribeRoots(machine);
      stopCurrent = () => {
        cancelled = true;
        void iterator.return(undefined);
      };
      store.set(rootsStateAtom, {
        phase: "connecting",
        message: "Opening roots",
      });
      void (async () => {
        try {
          for await (const event of iterator) {
            if (cancelled) break;
            applyRootsEvent(event, store, rootsAtom, rootsStateAtom);
          }
        } catch (err) {
          if (!cancelled) {
            store.set(rootsStateAtom, {
              phase: "error",
              message: errorMessage(err),
            });
          }
        }
      })();
    }

    const unsubscribe = store.sub(rootsSubscriptionKeyAtom, start);
    start();
    return () => {
      unsubscribe();
      stopCurrent?.();
    };
  });

  return {
    rootsAtom,
    rootsStateAtom,
  };
});

export const explorerDirectoryBunja = bunja(() => {
  bunja.use(MachineIdScope);
  bunja.use(ExplorerPaneScope);
  const store = bunja.use(JotaiStoreScope);
  const machineState = bunja.use(explorerMachineBunja);
  const refresh = bunja.use(explorerRefreshBunja);
  const navigation = bunja.use(explorerNavigationBunja);

  const directoryRowsAtom = atom<FsEntry[]>([]);
  const directoryStateAtom = atom<StreamState>({
    phase: "idle",
    message: "Directory idle",
  });
  const directorySubscriptionKeyAtom = atom((get) =>
    [
      get(machineState.connectionKeyAtom),
      get(machineState.isPairedAtom) ? "paired" : "unpaired",
      get(navigation.currentPathAtom) ?? "",
      get(refresh.refreshAtom),
    ].join("\n")
  );

  bunja.effect(() => {
    let stopCurrent: (() => void) | undefined;

    function start() {
      stopCurrent?.();
      stopCurrent = undefined;
      store.set(directoryRowsAtom, []);
      store.set(navigation.selectedPathAtom, undefined);

      const machine = store.get(machineState.machineAtom);
      const currentPath = store.get(navigation.currentPathAtom);
      if (!machine || !store.get(machineState.isPairedAtom) || !currentPath) {
        store.set(directoryStateAtom, {
          phase: "idle",
          message: "Directory idle",
        });
        return;
      }

      let cancelled = false;
      const iterator = subscribeDirectory(machine, currentPath);
      stopCurrent = () => {
        cancelled = true;
        void iterator.return(undefined);
      };
      store.set(directoryStateAtom, {
        phase: "connecting",
        message: "Opening directory",
      });
      void (async () => {
        try {
          for await (const event of iterator) {
            if (cancelled) break;
            applyDirectoryEvent(
              event,
              store,
              directoryRowsAtom,
              directoryStateAtom,
              (path) => {
                if (path) {
                  store.set(navigation.currentPathAtom, path);
                  store.set(navigation.openedFileAtom, undefined);
                  store.set(navigation.historyAtom, []);
                }
              },
            );
          }
        } catch (err) {
          if (!cancelled) {
            store.set(directoryStateAtom, {
              phase: "error",
              message: errorMessage(err),
            });
          }
        }
      })();
    }

    const unsubscribe = store.sub(directorySubscriptionKeyAtom, start);
    start();
    return () => {
      unsubscribe();
      stopCurrent?.();
    };
  });

  return {
    directoryRowsAtom,
    directoryStateAtom,
  };
});

export const explorerBunja = bunja(() => {
  bunja.use(MachineIdScope);
  bunja.use(ExplorerPaneScope);
  const navigation = bunja.use(explorerNavigationBunja);
  const roots = bunja.use(explorerRootsBunja);
  const directory = bunja.use(explorerDirectoryBunja);
  const refresh = bunja.use(explorerRefreshBunja);

  const rowsAtom = atom((get) =>
    get(navigation.currentPathAtom)
      ? get(directory.directoryRowsAtom)
      : get(roots.rootsAtom)
  );
  const visibleRowsAtom = atom((get) => sortEntries(get(rowsAtom)));
  const selectedEntryAtom = atom((get) =>
    get(rowsAtom).find((entry) =>
      entry.path === get(navigation.selectedPathAtom)
    ) ?? undefined
  );

  return {
    currentPathAtom: navigation.currentPathAtom,
    displayPathAtom: navigation.displayPathAtom,
    historyAtom: navigation.historyAtom,
    openedFileAtom: navigation.openedFileAtom,
    selectedPathAtom: navigation.selectedPathAtom,
    visibleRowsAtom,
    selectedEntryAtom,
    refresh: refresh.refresh,
    selectEntry: navigation.selectEntry,
    navigate: navigation.navigate,
    goBack: navigation.goBack,
    goUp: navigation.goUp,
    openEntry: navigation.openEntry,
    openFile: navigation.openFile,
  };
});

function explorerConnectionKey(machine?: Machine): string {
  if (!machine) return "";
  return [
    machine.id,
    machine.baseUrl,
    machine.clientId ?? "",
    machine.clientSecret ?? "",
  ].join("\n");
}

function explorerNavigationStorageKey(
  machineId: string | undefined,
  paneScopeId: string,
): string {
  return `wgo.explorer.navigation.${machineId ?? "none"}.${paneScopeId}.v1`;
}

export function copyExplorerNavigationState(
  machineId: string | undefined,
  sourcePaneScopeId: string,
  targetPaneScopeId: string,
) {
  try {
    const storage = globalThis.localStorage;
    const sourceKey = explorerNavigationStorageKey(
      machineId,
      sourcePaneScopeId,
    );
    const targetKey = explorerNavigationStorageKey(
      machineId,
      targetPaneScopeId,
    );
    const sourceValue = storage.getItem(sourceKey);
    if (sourceValue === null) {
      storage.removeItem(targetKey);
      return;
    }
    storage.setItem(targetKey, sourceValue);
  } catch {
    // Keep pane splitting usable even if persisted tab state cannot be copied.
  }
}

function resolveSetStateAction<Value>(
  action: SetStateAction<Value>,
  current: Value,
): Value {
  return typeof action === "function"
    ? (action as (current: Value) => Value)(current)
    : action;
}

function applyRootsEvent(
  event: RootsTableEvent,
  store: JotaiStore,
  rootsAtom: PrimitiveAtom<FsEntry[]>,
  rootsStateAtom: PrimitiveAtom<StreamState>,
) {
  if (event.type === "snapshot") {
    store.set(rootsAtom, event.rows);
    store.set(rootsStateAtom, { phase: "live", message: "Roots live" });
    return;
  }
  if (event.type === "patch") {
    store.set(
      rootsAtom,
      (current) =>
        applyEntryPatch(
          current,
          event.removes.map((item) => item.path),
          event.upserts,
        ),
    );
    store.set(rootsStateAtom, { phase: "live", message: "Roots updated" });
    return;
  }
  store.set(rootsStateAtom, {
    phase: "closed",
    message: `Roots closed: ${event.reason}`,
  });
}

function applyDirectoryEvent(
  event: DirectoryTableEvent,
  store: JotaiStore,
  directoryRowsAtom: PrimitiveAtom<FsEntry[]>,
  directoryStateAtom: PrimitiveAtom<StreamState>,
  onMoved: (path: string | undefined) => void,
) {
  if (event.type === "snapshot") {
    store.set(directoryRowsAtom, event.rows);
    store.set(directoryStateAtom, {
      phase: "live",
      message: "Directory live",
    });
    return;
  }
  if (event.type === "patch") {
    store.set(directoryRowsAtom, (current) => {
      const removedNames = new Set(event.removes.map((item) => item.name));
      const remaining = current.filter((entry) =>
        !removedNames.has(entry.name)
      );
      const upsertNames = new Set(event.upserts.map((entry) => entry.name));
      return sortEntries([
        ...remaining.filter((entry) => !upsertNames.has(entry.name)),
        ...event.upserts,
      ]);
    });
    store.set(directoryStateAtom, {
      phase: "live",
      message: "Directory updated",
    });
    return;
  }
  if (event.reason === "Moved") onMoved(event.to);
  store.set(directoryStateAtom, {
    phase: "closed",
    message: `Directory closed: ${event.reason}`,
  });
}

function applyEntryPatch(
  current: FsEntry[],
  removedPaths: string[],
  upserts: FsEntry[],
): FsEntry[] {
  const removed = new Set(removedPaths);
  const upsertPaths = new Set(upserts.map((entry) => entry.path));
  return sortEntries([
    ...current.filter((entry) =>
      !removed.has(entry.path) && !upsertPaths.has(entry.path)
    ),
    ...upserts,
  ]);
}

function sortEntries(entries: FsEntry[]): FsEntry[] {
  return [...entries].sort((left, right) => {
    const leftRank = kindRank(left.kind);
    const rightRank = kindRank(right.kind);
    if (leftRank !== rightRank) return leftRank - rightRank;
    return displayName(left).localeCompare(displayName(right), undefined, {
      numeric: true,
      sensitivity: "base",
    });
  });
}

function kindRank(kind: FsEntryKind): number {
  if (kind === FsEntryKind.Directory) return 0;
  if (kind === FsEntryKind.Symlink) return 1;
  if (kind === FsEntryKind.File) return 2;
  return 3;
}

export function pathCrumbs(
  path?: string,
): { label: string; path?: string }[] {
  if (!path) return [{ label: "Files" }];
  const { root, separator } = pathRoot(path);
  const crumbs = [{ label: "Files", path: root }];
  if (root !== "/") crumbs.push({ label: root, path: root });
  const rest = path.slice(root.length).replace(/[\\/]+$/g, "");
  if (!rest) return crumbs;
  let cursor = root;
  for (const part of rest.split(/[\\/]+/).filter(Boolean)) {
    cursor = cursor.endsWith(separator)
      ? `${cursor}${part}`
      : `${cursor}${separator}${part}`;
    crumbs.push({ label: part, path: cursor });
  }
  return crumbs;
}

function pathRoot(path: string): { root: string; separator: "\\" | "/" } {
  const drive = /^[A-Za-z]:[\\/]/.exec(path);
  if (drive) {
    const root = drive[0];
    return {
      root,
      separator: root.endsWith("/") ? "/" : "\\",
    };
  }
  const unc = /^\\\\[^\\]+\\[^\\]+\\?/.exec(path);
  if (unc) {
    const root = unc[0].endsWith("\\") ? unc[0] : `${unc[0]}\\`;
    return { root, separator: "\\" };
  }
  if (path.startsWith("/")) return { root: "/", separator: "/" };
  return { root: path, separator: path.includes("/") ? "/" : "\\" };
}

function parentPath(path: string): string | undefined {
  const { root } = pathRoot(path);
  const trimmed = path.replace(/[\\/]+$/g, "");
  if (trimmed === root.replace(/[\\/]+$/g, "")) return undefined;
  const index = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (index < 0) return undefined;
  if (index < root.length) return root;
  return trimmed.slice(0, index) || undefined;
}

export function displayName(entry: FsEntry): string {
  return entry.name.trim() || entry.path;
}

export function kindLabel(kind: FsEntryKind): string {
  switch (kind) {
    case FsEntryKind.File:
      return "File";
    case FsEntryKind.Directory:
      return "Directory";
    case FsEntryKind.Symlink:
      return "Link";
    case FsEntryKind.Other:
      return "Other";
  }
}

export function formatSize(size: number | undefined): string {
  if (size === undefined) return "";
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = size / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex++;
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${
    units[unitIndex]
  }`;
}

export function formatDate(ms: number | undefined): string {
  if (ms === undefined) return "";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(ms));
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
