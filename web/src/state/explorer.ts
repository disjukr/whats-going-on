import { bunja, createScope } from "bunja";
import { atom, type PrimitiveAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { type JotaiStore, JotaiStoreScope } from "unsaturated/store";
import { isInvalidCredentialsError } from "../protocol/client.ts";
import {
  subscribeDirectory,
  subscribeRoots,
} from "../protocol/generated/client.ts";
import {
  type DirectorySubscriptionCloseReason,
  type DirectoryTableEvent,
  type FsEntry,
  FsEntryKind,
  type RootsSubscriptionCloseReason,
  type RootsTableEvent,
} from "../protocol/generated/rpc.ts";
import { MachineIdScope } from "./machine.tsx";
import { machineBunja, machineStoreBunja } from "./machine-store.ts";
import { Machine } from "./machines.ts";
import { rpcSessionBunja } from "./rpc-session.ts";
import { StreamState } from "./types.ts";

export const ExplorerPaneScope = createScope<string>();
export const trashLocationPath = "wgo://trash";

export type ExplorerSpecialLocation = "trash";

export type ExplorerLocation =
  | { type: "root" }
  | { type: "directory"; path: string }
  | { type: "file"; directoryPath?: string; entry: FsEntry }
  | { type: "trash" };

interface ExplorerNavigationState {
  history: ExplorerLocation[];
  location: ExplorerLocation;
}

export const explorerMachineBunja = bunja(() => {
  const machine = bunja.use(machineBunja);

  const connectionKeyAtom = atom((get) =>
    explorerConnectionKey(get(machine.machineAtom))
  );

  return {
    machineAtom: machine.machineAtom,
    isPairedAtom: machine.isPairedAtom,
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
    { history: [], location: { type: "root" } },
    undefined,
    { getOnInit: true },
  );
  const locationAtom = atom((get) => get(navigationStateAtom).location);
  const currentPathAtom = atom((get) =>
    currentPathFromLocation(get(locationAtom))
  );
  const directoryPathAtom = atom((get) =>
    directoryPathFromLocation(get(locationAtom))
  );
  const openedFileAtom = atom((get) => {
    const location = get(locationAtom);
    return location.type === "file" ? location.entry : undefined;
  });
  const displayPathAtom = atom((get) =>
    displayPathFromLocation(get(locationAtom))
  );
  const specialLocationAtom = atom((get): ExplorerSpecialLocation | undefined =>
    get(locationAtom).type === "trash" ? "trash" : undefined
  );
  const historyAtom = atom((get) => get(navigationStateAtom).history);
  const selectedPathAtom = atom<string | undefined>(undefined);

  function selectEntry(entry: FsEntry) {
    store.set(selectedPathAtom, entry.path);
  }

  function navigate(path?: string) {
    if (path === trashLocationPath) {
      navigateTrash();
      return;
    }
    navigateTo(locationFromPath(path));
  }

  function navigateTo(location: ExplorerLocation) {
    store.set(navigationStateAtom, (current) => ({
      history: [...current.history, current.location],
      location,
    }));
    store.set(selectedPathAtom, undefined);
  }

  function navigateTrash() {
    navigateTo({ type: "trash" });
  }

  function replaceWithLocation(location: ExplorerLocation) {
    store.set(navigationStateAtom, (current) => ({
      ...current,
      location,
    }));
    store.set(selectedPathAtom, undefined);
  }

  function replaceWithTrash() {
    replaceWithLocation({ type: "trash" });
  }

  function replaceWithPath(path?: string) {
    replaceWithLocation(locationFromPath(path));
  }

  function replaceWithMovedDirectory(path?: string) {
    store.set(navigationStateAtom, {
      history: [],
      location: locationFromPath(path),
    });
    store.set(selectedPathAtom, undefined);
  }

  function goBack() {
    const history = store.get(navigationStateAtom).history;
    if (history.length === 0) return;
    const next = history[history.length - 1];
    store.set(navigationStateAtom, {
      history: history.slice(0, -1),
      location: next,
    });
    store.set(
      selectedPathAtom,
      next.type === "file" ? next.entry.path : undefined,
    );
  }

  function goUp() {
    const location = store.get(locationAtom);
    if (location.type === "trash" || location.type === "root") return;
    if (location.type === "file") {
      navigate(location.directoryPath);
      return;
    }
    navigate(parentPath(location.path));
  }

  function openEntry(entry: FsEntry) {
    if (entry.kind === FsEntryKind.Directory) {
      navigate(entry.path);
      return;
    }
    store.set(selectedPathAtom, entry.path);
  }

  function openFile(entry: FsEntry) {
    navigateTo({
      type: "file",
      directoryPath: directoryPathFromLocation(store.get(locationAtom)),
      entry,
    });
    store.set(selectedPathAtom, entry.path);
  }

  return {
    directoryPathAtom,
    currentPathAtom,
    displayPathAtom,
    historyAtom,
    openedFileAtom,
    selectedPathAtom,
    specialLocationAtom,
    selectEntry,
    navigate,
    navigateTrash,
    replaceWithPath,
    replaceWithMovedDirectory,
    replaceWithTrash,
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
  const machines = bunja.use(machineStoreBunja);
  const rpcSession = bunja.use(rpcSessionBunja);
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
      let iterator: AsyncGenerator<RootsTableEvent> | undefined;
      stopCurrent = () => {
        cancelled = true;
        void iterator?.return(undefined);
      };
      store.set(rootsStateAtom, {
        phase: "connecting",
        message: "Opening roots",
      });
      void (async () => {
        try {
          const transport = await rpcSession.webTransport();
          if (cancelled) return;
          iterator = subscribeRoots(transport);
          for await (const event of iterator) {
            if (cancelled) break;
            applyRootsEvent(event, store, rootsAtom, rootsStateAtom);
          }
        } catch (err) {
          if (!cancelled) {
            if (
              handleInvalidCredentials(
                machine,
                err,
                machines.clearMachineCredentials,
                rpcSession.closeRpcSession,
              )
            ) {
              store.set(rootsStateAtom, {
                phase: "error",
                message: "Pairing is no longer valid. Pair this machine again.",
              });
              return;
            }
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
  const machines = bunja.use(machineStoreBunja);
  const rpcSession = bunja.use(rpcSessionBunja);
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
      get(navigation.directoryPathAtom) ?? "",
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
      const directoryPath = store.get(navigation.directoryPathAtom);
      if (
        !machine || !store.get(machineState.isPairedAtom) || !directoryPath
      ) {
        store.set(directoryStateAtom, {
          phase: "idle",
          message: "Directory idle",
        });
        return;
      }

      let cancelled = false;
      let iterator: AsyncGenerator<DirectoryTableEvent> | undefined;
      stopCurrent = () => {
        cancelled = true;
        void iterator?.return(undefined);
      };
      store.set(directoryStateAtom, {
        phase: "connecting",
        message: "Opening directory",
      });
      void (async () => {
        try {
          const transport = await rpcSession.webTransport();
          if (cancelled) return;
          iterator = subscribeDirectory(transport, { path: directoryPath });
          for await (const event of iterator) {
            if (cancelled) break;
            applyDirectoryEvent(
              event,
              store,
              directoryRowsAtom,
              directoryStateAtom,
              (path) => {
                navigation.replaceWithMovedDirectory(path);
              },
            );
          }
        } catch (err) {
          if (!cancelled) {
            if (
              handleInvalidCredentials(
                machine,
                err,
                machines.clearMachineCredentials,
                rpcSession.closeRpcSession,
              )
            ) {
              store.set(directoryStateAtom, {
                phase: "error",
                message: "Pairing is no longer valid. Pair this machine again.",
              });
              return;
            }
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

  const rowsAtom = atom((get) => {
    if (get(navigation.directoryPathAtom)) {
      return get(directory.directoryRowsAtom);
    }
    return get(navigation.specialLocationAtom) ? [] : get(roots.rootsAtom);
  });
  const openedFileAtom = atom((get) => {
    const openedFile = get(navigation.openedFileAtom);
    if (!openedFile) return undefined;
    return get(rowsAtom).find((entry) => entry.path === openedFile.path) ??
      openedFile;
  });
  const visibleRowsAtom = atom((get) => sortEntries(get(rowsAtom)));
  const selectedEntryAtom = atom((get) =>
    get(rowsAtom).find((entry) =>
      entry.path === get(navigation.selectedPathAtom)
    ) ?? undefined
  );

  return {
    currentPathAtom: navigation.currentPathAtom,
    directoryPathAtom: navigation.directoryPathAtom,
    displayPathAtom: navigation.displayPathAtom,
    historyAtom: navigation.historyAtom,
    openedFileAtom,
    selectedPathAtom: navigation.selectedPathAtom,
    specialLocationAtom: navigation.specialLocationAtom,
    visibleRowsAtom,
    selectedEntryAtom,
    refresh: refresh.refresh,
    selectEntry: navigation.selectEntry,
    navigate: navigation.navigate,
    navigateTrash: navigation.navigateTrash,
    replaceWithPath: navigation.replaceWithPath,
    replaceWithMovedDirectory: navigation.replaceWithMovedDirectory,
    replaceWithTrash: navigation.replaceWithTrash,
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

function handleInvalidCredentials(
  machine: Machine,
  err: unknown,
  clearMachineCredentials: (machineId: string) => void,
  closeSession: () => void,
): boolean {
  if (!isInvalidCredentialsError(err)) return false;
  closeSession();
  clearMachineCredentials(machine.id);
  return true;
}

function explorerNavigationStorageKey(
  machineId: string | undefined,
  paneScopeId: string,
): string {
  return `wgo.explorer.navigation.${machineId ?? "none"}.${paneScopeId}.v2`;
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

export function writeExplorerFileNavigationState(
  machineId: string | undefined,
  paneScopeId: string,
  currentPath: string | undefined,
  openedFile: FsEntry,
) {
  try {
    const storage = globalThis.localStorage;
    const targetKey = explorerNavigationStorageKey(machineId, paneScopeId);
    const state: ExplorerNavigationState = {
      history: [],
      location: {
        type: "file",
        directoryPath: currentPath,
        entry: openedFile,
      },
    };
    storage.setItem(targetKey, JSON.stringify(state));
  } catch {
    // Opening a file should still work even if persisted tab state is unavailable.
  }
}

function locationFromPath(path?: string): ExplorerLocation {
  return path ? { type: "directory", path } : { type: "root" };
}

function currentPathFromLocation(
  location: ExplorerLocation,
): string | undefined {
  if (location.type === "directory") return location.path;
  if (location.type === "file") return location.directoryPath;
  return undefined;
}

function directoryPathFromLocation(
  location: ExplorerLocation,
): string | undefined {
  if (location.type === "directory") return location.path;
  if (location.type === "file") return location.directoryPath;
  return undefined;
}

function displayPathFromLocation(
  location: ExplorerLocation,
): string | undefined {
  switch (location.type) {
    case "root":
      return undefined;
    case "directory":
      return location.path;
    case "file":
      return location.entry.path;
    case "trash":
      return trashLocationPath;
  }
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
    message: `Roots closed: ${subscriptionCloseReasonLabel(event.reason)}`,
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
  if (event.reason.type === "moved") onMoved(event.reason.to);
  store.set(directoryStateAtom, {
    phase: "closed",
    message: `Directory closed: ${subscriptionCloseReasonLabel(event.reason)}`,
  });
}

function subscriptionCloseReasonLabel(
  reason: DirectorySubscriptionCloseReason | RootsSubscriptionCloseReason,
): string {
  return reason.type;
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
  if (!path) return [{ label: "Root" }];
  const { root, separator } = pathRoot(path);
  const crumbs = [{ label: "Root" }, { label: root, path: root }];
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
