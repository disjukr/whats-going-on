import { createContext } from "react";
import { bunja } from "bunja";
import { createScopeFromContext } from "bunja/react";
import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { createLayout, type LayoutState } from "panecake";
import { copyExplorerNavigationState } from "./explorer.ts";
import { JotaiStoreScope } from "./jotai-store.ts";
import { MachineIdScope } from "./machine-id.tsx";

export type WorkbenchTool = "daemon" | "files" | "processes" | "terminal";

export interface WorkbenchTab {
  id: string;
  title: string;
  tool: WorkbenchTool;
}

export interface WorkbenchPane {
  id: string;
  tabs: WorkbenchTab[];
  activeTabId: string;
}

export interface MoveTabToNewPaneResult {
  newPaneId: string;
  sourcePaneRemoved: boolean;
}

interface WorkbenchState {
  layout: LayoutState;
  activePaneId?: string;
  panes: WorkbenchPane[];
}

export type TabDropPosition = "before" | "after" | "end";

export const WorkbenchPaneIdContext = createContext<string | undefined>(
  undefined,
);
export const WorkbenchTabIdContext = createContext<string | undefined>(
  undefined,
);
export const WorkbenchPaneIdScope = createScopeFromContext(
  WorkbenchPaneIdContext,
);
export const WorkbenchTabIdScope = createScopeFromContext(
  WorkbenchTabIdContext,
);

export const workbenchBunja = bunja(() => {
  const machineId = bunja.use(MachineIdScope);
  const store = bunja.use(JotaiStoreScope);
  const initialPaneId = `pane-${crypto.randomUUID()}`;
  const initialTab = createFilesTab();
  const initialLayout = createLayout((builder) => builder.leaf(initialPaneId));
  const initialState: WorkbenchState = {
    layout: initialLayout,
    activePaneId: initialPaneId,
    panes: [
      {
        id: initialPaneId,
        tabs: [initialTab],
        activeTabId: initialTab.id,
      },
    ],
  };
  const stateAtom = atomWithStorage<WorkbenchState>(
    workbenchStorageKey(machineId),
    initialState,
    undefined,
    { getOnInit: true },
  );
  const layoutAtom = atom((get) => get(stateAtom).layout);
  const activePaneIdAtom = atom((get) => {
    const state = get(stateAtom);
    return activePaneFromState(state)?.id;
  });
  const activeToolAtom = atom((get) => {
    const state = get(stateAtom);
    return activeTabFromPane(activePaneFromState(state))?.tool ?? "files";
  });
  const panesAtom = atom((get) => get(stateAtom).panes);

  function setLayout(layout: LayoutState) {
    store.set(stateAtom, (current) => ({ ...current, layout }));
  }

  function selectTool(tool: WorkbenchTool) {
    store.set(stateAtom, (current) => openToolTabInActivePane(current, tool));
  }

  function focusPane(paneId: string) {
    store.set(
      stateAtom,
      (current) => {
        if (current.activePaneId === paneId) return current;
        if (!current.panes.some((pane) => pane.id === paneId)) return current;
        return { ...current, activePaneId: paneId };
      },
    );
  }

  function addPane(sourcePaneId?: string): string {
    const sourcePane = sourcePaneId === undefined
      ? undefined
      : store.get(stateAtom).panes.find((pane) => pane.id === sourcePaneId);
    const sourceTab = sourcePane?.tabs.find((tab) =>
      tab.id === sourcePane.activeTabId
    ) ?? sourcePane?.tabs[0];
    const tab = sourceTab === undefined
      ? createFilesTab()
      : cloneWorkbenchTab(sourceTab);
    const pane: WorkbenchPane = {
      id: `pane-${crypto.randomUUID()}`,
      tabs: [tab],
      activeTabId: tab.id,
    };
    if (sourceTab) copyTabState(machineId, sourceTab, tab);
    store.set(stateAtom, (current) => ({
      ...current,
      activePaneId: pane.id,
      panes: [...current.panes, pane],
    }));
    return pane.id;
  }

  function removePane(paneId: string) {
    store.set(
      stateAtom,
      (current) => {
        if (current.panes.length <= 1) return current;
        const panes = current.panes.filter((pane) => pane.id !== paneId);
        return {
          ...current,
          panes,
          activePaneId: current.activePaneId === paneId
            ? panes[0]?.id
            : current.activePaneId,
        };
      },
    );
  }

  function updatePanes(update: (panes: WorkbenchPane[]) => WorkbenchPane[]) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: update(current.panes),
      }),
    );
  }

  function addFilesTab(paneId: string) {
    addToolTab(paneId, "files");
  }

  function addDaemonTab(paneId: string) {
    addToolTab(paneId, "daemon");
  }

  function addToolTab(paneId: string, tool: WorkbenchTool) {
    const tab = createWorkbenchTab(tool);
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        activePaneId: paneId,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: [...pane.tabs, tab],
              activeTabId: tab.id,
            }
            : pane
        ),
      }),
    );
  }

  function selectTab(paneId: string, tabId: string) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        activePaneId: paneId,
        panes: current.panes.map((pane) =>
          pane.id === paneId ? { ...pane, activeTabId: tabId } : pane
        ),
      }),
    );
  }

  function closeTab(paneId: string, tabId: string) {
    updatePanes((current) =>
      current.map((pane) => {
        if (pane.id !== paneId || pane.tabs.length <= 1) return pane;
        const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
        const activeTabId = pane.activeTabId === tabId
          ? tabs[0].id
          : pane.activeTabId;
        return { ...pane, tabs, activeTabId };
      })
    );
  }

  function moveTab(
    sourcePaneId: string,
    tabId: string,
    targetPaneId: string,
    targetTabId: string | undefined,
    position: TabDropPosition,
  ): boolean {
    const currentState = store.get(stateAtom);
    const currentSourcePane = currentState.panes.find((pane) =>
      pane.id === sourcePaneId
    );
    const currentTargetPane = currentState.panes.find((pane) =>
      pane.id === targetPaneId
    );
    const currentMovingTab = currentSourcePane?.tabs.find((tab) =>
      tab.id === tabId
    );
    const removeSourcePane = sourcePaneId !== targetPaneId &&
      currentSourcePane !== undefined &&
      currentTargetPane !== undefined &&
      currentMovingTab !== undefined &&
      currentSourcePane.tabs.length <= 1;

    store.set(stateAtom, (current) => {
      const sourcePane = current.panes.find((pane) => pane.id === sourcePaneId);
      const targetPane = current.panes.find((pane) => pane.id === targetPaneId);
      const movingTab = sourcePane?.tabs.find((tab) => tab.id === tabId);
      if (!sourcePane || !targetPane || !movingTab) return current;

      const panes = current.panes.flatMap((pane) => {
        if (sourcePaneId === targetPaneId && pane.id === sourcePaneId) {
          return [moveTabWithinPane(pane, tabId, targetTabId, position)];
        }

        if (pane.id === sourcePaneId) {
          const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
          if (tabs.length === 0) return [];
          const activeTabId = pane.activeTabId === tabId
            ? tabs[0]?.id ?? ""
            : pane.activeTabId;
          return [{ ...pane, tabs, activeTabId }];
        }

        if (pane.id === targetPaneId) {
          const tabs = insertTab(pane.tabs, movingTab, targetTabId, position);
          return [{ ...pane, tabs, activeTabId: movingTab.id }];
        }

        return [pane];
      });

      return {
        ...current,
        panes,
        activePaneId: targetPaneId,
      };
    });
    return removeSourcePane;
  }

  function moveTabToNewPane(
    sourcePaneId: string,
    tabId: string,
    targetPaneId: string,
  ): MoveTabToNewPaneResult | undefined {
    const currentState = store.get(stateAtom);
    const currentSourcePane = currentState.panes.find((pane) =>
      pane.id === sourcePaneId
    );
    const currentMovingTab = currentSourcePane?.tabs.find((tab) =>
      tab.id === tabId
    );
    if (!currentSourcePane || !currentMovingTab) return undefined;
    if (sourcePaneId === targetPaneId && currentSourcePane.tabs.length <= 1) {
      return undefined;
    }

    const newPaneId = `pane-${crypto.randomUUID()}`;
    const sourcePaneRemoved = currentSourcePane.tabs.length <= 1;
    store.set(stateAtom, (current) => {
      const sourcePane = current.panes.find((pane) => pane.id === sourcePaneId);
      const movingTab = sourcePane?.tabs.find((tab) => tab.id === tabId);
      if (!sourcePane || !movingTab) return current;

      const newPane: WorkbenchPane = {
        id: newPaneId,
        tabs: [movingTab],
        activeTabId: movingTab.id,
      };
      const panes = current.panes.flatMap((pane) => {
        if (pane.id !== sourcePaneId) return [pane];

        const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
        if (tabs.length === 0) return [];

        const activeTabId = pane.activeTabId === tabId
          ? tabs[0].id
          : pane.activeTabId;
        return [{ ...pane, tabs, activeTabId }];
      });

      return {
        ...current,
        activePaneId: newPaneId,
        panes: [...panes, newPane],
      };
    });

    return { newPaneId, sourcePaneRemoved };
  }

  return {
    layoutAtom,
    activeToolAtom,
    activePaneIdAtom,
    panesAtom,
    setLayout,
    selectTool,
    focusPane,
    addPane,
    removePane,
    addDaemonTab,
    addFilesTab,
    selectTab,
    closeTab,
    moveTab,
    moveTabToNewPane,
  };
});

export const workbenchPaneBunja = bunja(() => {
  const paneId = requireScopeValue(
    bunja.use(WorkbenchPaneIdScope),
    "Workbench pane id",
  );
  const workbench = bunja.use(workbenchBunja);

  const paneAtom = atom((get) =>
    get(workbench.panesAtom).find((pane) => pane.id === paneId) ?? undefined
  );
  const paneCountAtom = atom((get) => get(workbench.panesAtom).length);
  const activeAtom = atom((get) => get(workbench.activePaneIdAtom) === paneId);

  function addPane(): string {
    return workbench.addPane(paneId);
  }

  function addFilesTab() {
    workbench.addFilesTab(paneId);
  }

  function addDaemonTab() {
    workbench.addDaemonTab(paneId);
  }

  function removePane() {
    workbench.removePane(paneId);
  }

  function focusPane() {
    workbench.focusPane(paneId);
  }

  function selectTab(tabId: string) {
    workbench.selectTab(paneId, tabId);
  }

  function closeTab(tabId: string) {
    workbench.closeTab(paneId, tabId);
  }

  function moveTab(
    sourcePaneId: string,
    tabId: string,
    targetTabId: string | undefined,
    position: TabDropPosition,
  ): boolean {
    return workbench.moveTab(
      sourcePaneId,
      tabId,
      paneId,
      targetTabId,
      position,
    );
  }

  function moveTabToNewPane(
    sourcePaneId: string,
    tabId: string,
  ): MoveTabToNewPaneResult | undefined {
    return workbench.moveTabToNewPane(sourcePaneId, tabId, paneId);
  }

  return {
    paneId,
    paneAtom,
    paneCountAtom,
    activeAtom,
    addPane,
    addDaemonTab,
    addFilesTab,
    removePane,
    focusPane,
    selectTab,
    closeTab,
    moveTab,
    moveTabToNewPane,
  };
});

export const workbenchTabBunja = bunja(() => {
  const tabId = requireScopeValue(
    bunja.use(WorkbenchTabIdScope),
    "Workbench tab id",
  );
  const pane = bunja.use(workbenchPaneBunja);

  const tabAtom = atom((get) =>
    get(pane.paneAtom)?.tabs.find((tab) => tab.id === tabId) ?? undefined
  );
  const activeAtom = atom((get) => get(pane.paneAtom)?.activeTabId === tabId);
  const showCloseAtom = atom((get) => {
    const paneValue = get(pane.paneAtom);
    return (paneValue?.tabs.length ?? 0) > 1 || get(pane.paneCountAtom) > 1;
  });

  function selectTab() {
    pane.selectTab(tabId);
  }

  return {
    paneId: pane.paneId,
    tabId,
    tabAtom,
    activeAtom,
    showCloseAtom,
    selectTab,
  };
});

function workbenchStorageKey(machineId: string | undefined): string {
  return `wgo.workbench.${machineId ?? "none"}.v1`;
}

function requireScopeValue(
  value: string | undefined,
  name: string,
): string {
  if (!value) throw new Error(`${name} is not provided.`);
  return value;
}

function moveTabWithinPane(
  pane: WorkbenchPane,
  tabId: string,
  targetTabId: string | undefined,
  position: TabDropPosition,
): WorkbenchPane {
  if (targetTabId === tabId) return pane;

  const movingTab = pane.tabs.find((tab) => tab.id === tabId);
  if (!movingTab) return pane;

  const remainingTabs = pane.tabs.filter((tab) => tab.id !== tabId);
  const tabs = insertTab(remainingTabs, movingTab, targetTabId, position);
  return { ...pane, tabs, activeTabId: tabId };
}

function insertTab(
  tabs: WorkbenchTab[],
  tab: WorkbenchTab,
  targetTabId: string | undefined,
  position: TabDropPosition,
): WorkbenchTab[] {
  if (position === "end" || !targetTabId) return [...tabs, tab];

  const targetIndex = tabs.findIndex((item) => item.id === targetTabId);
  if (targetIndex < 0) return [...tabs, tab];

  const insertIndex = position === "before" ? targetIndex : targetIndex + 1;
  return [
    ...tabs.slice(0, insertIndex),
    tab,
    ...tabs.slice(insertIndex),
  ];
}

function createFilesTab(): WorkbenchTab {
  return createWorkbenchTab("files");
}

function createWorkbenchTab(tool: WorkbenchTool): WorkbenchTab {
  return {
    id: `${tool}-${crypto.randomUUID()}`,
    title: titleForTool(tool),
    tool,
  };
}

function titleForTool(tool: WorkbenchTool): string {
  switch (tool) {
    case "daemon":
      return "Daemon";
    case "files":
      return "Files";
    case "processes":
      return "Processes";
    case "terminal":
      return "Terminal";
  }
}

function openToolTabInActivePane(
  state: WorkbenchState,
  tool: WorkbenchTool,
): WorkbenchState {
  const activePaneId = activePaneFromState(state)?.id;
  if (!activePaneId) return state;

  let opened = false;
  const panes = state.panes.map((pane) => {
    if (pane.id !== activePaneId) return pane;

    const existingTab = pane.tabs.find((tab) => tab.tool === tool);
    if (existingTab) {
      opened = true;
      return { ...pane, activeTabId: existingTab.id };
    }

    const tab = createWorkbenchTab(tool);
    opened = true;
    return {
      ...pane,
      tabs: [...pane.tabs, tab],
      activeTabId: tab.id,
    };
  });

  if (!opened) return state;
  return {
    ...state,
    activePaneId,
    panes,
  };
}

function activePaneFromState(
  state: WorkbenchState,
): WorkbenchPane | undefined {
  return state.panes.find((pane) => pane.id === state.activePaneId) ??
    state.panes[0];
}

function activeTabFromPane(
  pane: WorkbenchPane | undefined,
): WorkbenchTab | undefined {
  if (!pane) return undefined;
  return pane.tabs.find((tab) => tab.id === pane.activeTabId) ?? pane.tabs[0];
}

function cloneWorkbenchTab(tab: WorkbenchTab): WorkbenchTab {
  return {
    ...tab,
    id: `${tab.tool}-${crypto.randomUUID()}`,
  };
}

function copyTabState(
  machineId: string | undefined,
  sourceTab: WorkbenchTab,
  targetTab: WorkbenchTab,
) {
  if (sourceTab.tool === "files" && targetTab.tool === "files") {
    copyExplorerNavigationState(machineId, sourceTab.id, targetTab.id);
  }
}
