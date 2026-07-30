import { bunja } from "bunja";
import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { createLayout, layoutReducer, type LayoutState } from "panecake";
import { JotaiStoreScope } from "unsaturated/store";
import type { TerminalLaunchSpec } from "../protocol/generated/rpc.ts";
import { copyExplorerNavigationState } from "./explorer.ts";
import { copyFileViewerState } from "./file-viewer.ts";
import { MachineIdScope } from "./machine.tsx";
import {
  WorkbenchPaneIdContext,
  WorkbenchPaneIdScope,
  WorkbenchTabIdContext,
  WorkbenchTabIdScope,
} from "./workbench-context.ts";

export {
  WorkbenchPaneIdContext,
  WorkbenchPaneIdScope,
  WorkbenchTabIdContext,
  WorkbenchTabIdScope,
};

export type WorkbenchTool =
  | "agent"
  | "daemon"
  | "files"
  | "processes"
  | "terminal"
  | "windows";
export type WorkbenchFilesView = "roots" | "home" | "trash";
export type WorkbenchProcessPage =
  | "detail"
  | "children"
  | "heldResources"
  | "heldSockets"
  | "modules";

export interface WorkbenchTab {
  agentSessionId?: string;
  daemonClientDetailId?: string;
  daemonClientsPageOpen?: boolean;
  dirty?: boolean;
  filesView?: WorkbenchFilesView;
  id: string;
  previousActiveTabId?: string;
  processDetailPid?: number;
  processPage?: WorkbenchProcessPage;
  title: string;
  tool: WorkbenchTool;
  terminalLaunch?: TerminalLaunchSpec;
  terminalLastKnownCwd?: string;
  terminalLastKnownTitle?: string;
  terminalSessionId?: string;
  windowDetailId?: string;
}

export interface WorkbenchAgentTabConfig {
  sessionId?: string;
  title?: string;
}

export interface WorkbenchTerminalTabConfig {
  cwd?: string;
  launch?: TerminalLaunchSpec;
  terminalSessionId?: string;
  terminalTitle?: string;
  title?: string;
}

export interface WorkbenchFilesTabConfig {
  filesView?: WorkbenchFilesView;
}

export interface WorkbenchProcessesTabConfig {
  processDetailPid?: number;
  processPage?: WorkbenchProcessPage;
  title?: string;
}

export interface WorkbenchWindowsTabConfig {
  title?: string;
  windowDetailId?: string;
}

export interface WorkbenchTerminalSessionSnapshot {
  lastKnownCwd?: string;
  lastKnownTitle?: string;
  launch?: TerminalLaunchSpec;
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
  const layoutAtom = atom((get) => {
    const state = get(stateAtom);
    return pruneStaleLayoutLeaves(state.layout, state.panes);
  });
  const activePaneIdAtom = atom((get) => {
    const state = get(stateAtom);
    return activePaneFromState(state)?.id;
  });
  const activeToolAtom = atom((get) => {
    const state = get(stateAtom);
    return activeTabFromPane(activePaneFromState(state))?.tool ?? "files";
  });
  const activeAgentSessionIdAtom = atom((get) => {
    const state = get(stateAtom);
    const activeTab = activeTabFromPane(activePaneFromState(state));
    return activeTab?.tool === "agent" ? activeTab.agentSessionId : undefined;
  });
  const panesAtom = atom((get) => get(stateAtom).panes);

  function setLayout(layout: LayoutState) {
    store.set(stateAtom, (current) => ({
      ...current,
      layout: pruneStaleLayoutLeaves(layout, current.panes),
    }));
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
          layout: pruneStaleLayoutLeaves(current.layout, panes),
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
      (current) => {
        const panes = update(current.panes);
        return {
          ...current,
          layout: pruneStaleLayoutLeaves(current.layout, panes),
          panes,
        };
      },
    );
  }

  function addFilesTab(
    paneId: string,
    config: WorkbenchFilesTabConfig = {},
  ): string {
    const tab = createFilesTab(config);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
    return tab.id;
  }

  function addDaemonTab(paneId: string): string {
    return addToolTab(paneId, "daemon");
  }

  function addAgentTab(
    paneId: string,
    config: WorkbenchAgentTabConfig = {},
  ): string {
    const tab = createAgentTab(config);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
    return tab.id;
  }

  function addProcessesTab(
    paneId: string,
    config: WorkbenchProcessesTabConfig = {},
  ): string {
    const tab = createProcessesTab(config);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
    return tab.id;
  }

  function addWindowsTab(
    paneId: string,
    config: WorkbenchWindowsTabConfig = {},
  ): string {
    const tab = createWindowsTab(config);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
    return tab.id;
  }

  function addTerminalTab(
    paneId: string,
    config: WorkbenchTerminalTabConfig = {},
  ) {
    const tab = createTerminalTab(config);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
  }

  function openTerminalTab(config: WorkbenchTerminalTabConfig = {}) {
    const tab = createTerminalTab(config);
    store.set(stateAtom, (current) => openTabInActivePane(current, tab));
  }

  function openFilesTab(config: WorkbenchFilesTabConfig = {}) {
    const tab = createFilesTab(config);
    store.set(stateAtom, (current) => openTabInActivePane(current, tab));
  }

  function openDaemonTab() {
    store.set(stateAtom, (current) =>
      openToolTabInActivePane(
        current,
        "daemon",
      ));
  }

  function openAgentTab(config: WorkbenchAgentTabConfig = {}) {
    store.set(stateAtom, (current) => {
      const activePane = activePaneFromState(current);
      if (!activePane) return current;
      const existing = activePane.tabs.find((tab) =>
        tab.tool === "agent" &&
        tab.agentSessionId === config.sessionId
      );
      if (existing) {
        return {
          ...current,
          activePaneId: activePane.id,
          panes: current.panes.map((pane) =>
            pane.id === activePane.id
              ? activatePaneTab(pane, existing.id)
              : pane
          ),
        };
      }
      return openTabInPane(current, activePane.id, createAgentTab(config));
    });
  }

  function openProcessesTab(config: WorkbenchProcessesTabConfig = {}) {
    const tab = createProcessesTab(config);
    store.set(stateAtom, (current) => openTabInActivePane(current, tab));
  }

  function openWindowsTab(config: WorkbenchWindowsTabConfig = {}) {
    const tab = createWindowsTab(config);
    store.set(stateAtom, (current) => openTabInActivePane(current, tab));
  }

  function setTerminalSessionId(
    paneId: string,
    tabId: string,
    terminalSessionId: string | undefined,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "terminal"
                  ? { ...tab, terminalSessionId }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setAgentSession(
    paneId: string,
    tabId: string,
    sessionId: string,
    title?: string,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "agent"
                  ? {
                    ...tab,
                    agentSessionId: sessionId,
                    title: title ?? tab.title,
                  }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setTerminalSessionSnapshot(
    paneId: string,
    tabId: string,
    snapshot: WorkbenchTerminalSessionSnapshot,
  ) {
    store.set(
      stateAtom,
      (current) => {
        let changed = false;
        const panes = current.panes.map((pane) => {
          if (pane.id !== paneId) return pane;
          let paneChanged = false;
          const tabs = pane.tabs.map((tab) => {
            if (tab.id !== tabId || tab.tool !== "terminal") return tab;
            if (
              terminalLaunchEquals(tab.terminalLaunch, snapshot.launch) &&
              tab.terminalLastKnownCwd === snapshot.lastKnownCwd &&
              tab.terminalLastKnownTitle === snapshot.lastKnownTitle
            ) {
              return tab;
            }
            changed = true;
            paneChanged = true;
            return {
              ...tab,
              terminalLastKnownCwd: snapshot.lastKnownCwd,
              terminalLastKnownTitle: snapshot.lastKnownTitle,
              terminalLaunch: snapshot.launch,
            };
          });
          return paneChanged ? { ...pane, tabs } : pane;
        });
        return changed ? { ...current, panes } : current;
      },
    );
  }

  function setTabDirty(paneId: string, tabId: string, dirty: boolean) {
    store.set(
      stateAtom,
      (current) => {
        let changed = false;
        const panes = current.panes.map((pane) => {
          if (pane.id !== paneId) return pane;
          let paneChanged = false;
          const tabs = pane.tabs.map((tab) => {
            if (tab.id !== tabId) return tab;
            if (Boolean(tab.dirty) === dirty) return tab;
            changed = true;
            paneChanged = true;
            return { ...tab, dirty: dirty ? true : undefined };
          });
          return paneChanged ? { ...pane, tabs } : pane;
        });
        return changed ? { ...current, panes } : current;
      },
    );
  }

  function setDaemonClientDetailId(
    paneId: string,
    tabId: string,
    daemonClientDetailId: string | undefined,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "daemon"
                  ? { ...tab, daemonClientDetailId }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setDaemonClientsPageOpen(
    paneId: string,
    tabId: string,
    daemonClientsPageOpen: boolean,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "daemon"
                  ? { ...tab, daemonClientsPageOpen }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setProcessDetailPid(
    paneId: string,
    tabId: string,
    processDetailPid: number | undefined,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "processes"
                  ? {
                    ...tab,
                    processDetailPid,
                    processPage: processDetailPid === undefined
                      ? undefined
                      : "detail",
                  }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setProcessPage(
    paneId: string,
    tabId: string,
    processPage: WorkbenchProcessPage,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "processes" &&
                  tab.processDetailPid !== undefined
                  ? { ...tab, processPage }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function setWindowDetailId(
    paneId: string,
    tabId: string,
    windowDetailId: string | undefined,
  ) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: pane.tabs.map((tab) =>
                tab.id === tabId && tab.tool === "windows"
                  ? { ...tab, windowDetailId }
                  : tab
              ),
            }
            : pane
        ),
      }),
    );
  }

  function addToolTab(paneId: string, tool: WorkbenchTool): string {
    const tab = createWorkbenchTab(tool);
    store.set(stateAtom, (current) => openTabInPane(current, paneId, tab));
    return tab.id;
  }

  function duplicateTab(paneId: string, tabId: string): string | undefined {
    const sourcePane = store.get(stateAtom).panes.find((pane) =>
      pane.id === paneId
    );
    const sourceTab = sourcePane?.tabs.find((tab) => tab.id === tabId);
    if (!sourcePane || !sourceTab) return undefined;

    const tab = cloneWorkbenchTab(sourceTab);
    copyTabState(machineId, sourceTab, tab);
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        activePaneId: paneId,
        panes: current.panes.map((pane) =>
          pane.id === paneId
            ? activatePaneTab(
              { ...pane, tabs: insertTab(pane.tabs, tab, tabId, "after") },
              tab.id,
            )
            : pane
        ),
      }),
    );
    return tab.id;
  }

  function selectTab(paneId: string, tabId: string) {
    store.set(
      stateAtom,
      (current) => ({
        ...current,
        activePaneId: paneId,
        panes: current.panes.map((pane) =>
          pane.id === paneId ? activatePaneTab(pane, tabId) : pane
        ),
      }),
    );
  }

  function closeTab(paneId: string, tabId: string) {
    updatePanes((current) =>
      current.map((pane) => {
        if (pane.id !== paneId || pane.tabs.length <= 1) return pane;
        const closingTab = pane.tabs.find((tab) => tab.id === tabId);
        const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
        const activeTabId = pane.activeTabId === tabId
          ? fallbackActiveTabId(closingTab, tabs)
          : pane.activeTabId;
        return normalizePaneTabHistory({ ...pane, tabs, activeTabId });
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
          const closingTab = pane.tabs.find((tab) => tab.id === tabId);
          const activeTabId = pane.activeTabId === tabId
            ? fallbackActiveTabId(closingTab, tabs)
            : pane.activeTabId;
          return [normalizePaneTabHistory({ ...pane, tabs, activeTabId })];
        }

        if (pane.id === targetPaneId) {
          const tabs = insertTab(pane.tabs, movingTab, targetTabId, position);
          return [activatePaneTab({ ...pane, tabs }, movingTab.id)];
        }

        return [pane];
      });

      return {
        ...current,
        layout: pruneStaleLayoutLeaves(current.layout, panes),
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
        tabs: [{ ...movingTab, previousActiveTabId: undefined }],
        activeTabId: movingTab.id,
      };
      const panes = current.panes.flatMap((pane) => {
        if (pane.id !== sourcePaneId) return [pane];

        const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
        if (tabs.length === 0) return [];

        const closingTab = pane.tabs.find((tab) => tab.id === tabId);
        const activeTabId = pane.activeTabId === tabId
          ? fallbackActiveTabId(closingTab, tabs)
          : pane.activeTabId;
        return [normalizePaneTabHistory({ ...pane, tabs, activeTabId })];
      });

      return {
        ...current,
        activePaneId: newPaneId,
        layout: pruneStaleLayoutLeaves(current.layout, panes),
        panes: [...panes, newPane],
      };
    });

    return { newPaneId, sourcePaneRemoved };
  }

  return {
    layoutAtom,
    activeToolAtom,
    activeAgentSessionIdAtom,
    activePaneIdAtom,
    panesAtom,
    setLayout,
    selectTool,
    focusPane,
    addPane,
    removePane,
    addDaemonTab,
    addAgentTab,
    addFilesTab,
    addProcessesTab,
    addWindowsTab,
    addTerminalTab,
    openDaemonTab,
    openAgentTab,
    openFilesTab,
    openProcessesTab,
    openTerminalTab,
    openWindowsTab,
    duplicateTab,
    selectTab,
    closeTab,
    moveTab,
    moveTabToNewPane,
    setAgentSession,
    setDaemonClientDetailId,
    setDaemonClientsPageOpen,
    setProcessDetailPid,
    setProcessPage,
    setWindowDetailId,
    setTabDirty,
    setTerminalSessionSnapshot,
    setTerminalSessionId,
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

  function addFilesTab(config: WorkbenchFilesTabConfig = {}): string {
    return workbench.addFilesTab(paneId, config);
  }

  function addDaemonTab(): string {
    return workbench.addDaemonTab(paneId);
  }

  function addAgentTab(config: WorkbenchAgentTabConfig = {}): string {
    return workbench.addAgentTab(paneId, config);
  }

  function addProcessesTab(
    config: WorkbenchProcessesTabConfig = {},
  ): string {
    return workbench.addProcessesTab(paneId, config);
  }

  function addWindowsTab(config: WorkbenchWindowsTabConfig = {}): string {
    return workbench.addWindowsTab(paneId, config);
  }

  function addTerminalTab(config: WorkbenchTerminalTabConfig = {}) {
    workbench.addTerminalTab(paneId, config);
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

  function duplicateTab(tabId: string): string | undefined {
    return workbench.duplicateTab(paneId, tabId);
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

  function setTerminalSessionId(
    tabId: string,
    terminalSessionId: string | undefined,
  ) {
    workbench.setTerminalSessionId(paneId, tabId, terminalSessionId);
  }

  function setAgentSession(
    tabId: string,
    sessionId: string,
    title?: string,
  ) {
    workbench.setAgentSession(paneId, tabId, sessionId, title);
  }

  function setTerminalSessionSnapshot(
    tabId: string,
    snapshot: WorkbenchTerminalSessionSnapshot,
  ) {
    workbench.setTerminalSessionSnapshot(paneId, tabId, snapshot);
  }

  function setDaemonClientDetailId(
    tabId: string,
    daemonClientDetailId: string | undefined,
  ) {
    workbench.setDaemonClientDetailId(paneId, tabId, daemonClientDetailId);
  }

  function setDaemonClientsPageOpen(
    tabId: string,
    daemonClientsPageOpen: boolean,
  ) {
    workbench.setDaemonClientsPageOpen(
      paneId,
      tabId,
      daemonClientsPageOpen,
    );
  }

  function setProcessDetailPid(
    tabId: string,
    processDetailPid: number | undefined,
  ) {
    workbench.setProcessDetailPid(paneId, tabId, processDetailPid);
  }

  function setProcessPage(tabId: string, processPage: WorkbenchProcessPage) {
    workbench.setProcessPage(paneId, tabId, processPage);
  }

  function setWindowDetailId(
    tabId: string,
    windowDetailId: string | undefined,
  ) {
    workbench.setWindowDetailId(paneId, tabId, windowDetailId);
  }

  function setTabDirty(tabId: string, dirty: boolean) {
    workbench.setTabDirty(paneId, tabId, dirty);
  }

  return {
    paneId,
    paneAtom,
    paneCountAtom,
    activeAtom,
    addPane,
    addDaemonTab,
    addAgentTab,
    addFilesTab,
    addProcessesTab,
    addWindowsTab,
    addTerminalTab,
    removePane,
    focusPane,
    selectTab,
    closeTab,
    duplicateTab,
    moveTab,
    moveTabToNewPane,
    setAgentSession,
    setDaemonClientDetailId,
    setDaemonClientsPageOpen,
    setProcessDetailPid,
    setProcessPage,
    setWindowDetailId,
    setTabDirty,
    setTerminalSessionSnapshot,
    setTerminalSessionId,
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
  const focusedAtom = atom((get) =>
    get(pane.activeAtom) && get(pane.paneAtom)?.activeTabId === tabId
  );
  const showCloseAtom = atom((get) => {
    const paneValue = get(pane.paneAtom);
    return (paneValue?.tabs.length ?? 0) > 1 || get(pane.paneCountAtom) > 1;
  });
  const dirtyAtom = atom((get) => Boolean(get(tabAtom)?.dirty));

  function selectTab() {
    pane.selectTab(tabId);
  }

  function setTerminalSessionId(terminalSessionId: string | undefined) {
    pane.setTerminalSessionId(tabId, terminalSessionId);
  }

  function setAgentSession(sessionId: string, title?: string) {
    pane.setAgentSession(tabId, sessionId, title);
  }

  function setTerminalSessionSnapshot(
    snapshot: WorkbenchTerminalSessionSnapshot,
  ) {
    pane.setTerminalSessionSnapshot(tabId, snapshot);
  }

  function setDaemonClientDetailId(daemonClientDetailId: string | undefined) {
    pane.setDaemonClientDetailId(tabId, daemonClientDetailId);
  }

  function setDaemonClientsPageOpen(daemonClientsPageOpen: boolean) {
    pane.setDaemonClientsPageOpen(tabId, daemonClientsPageOpen);
  }

  function setProcessDetailPid(processDetailPid: number | undefined) {
    pane.setProcessDetailPid(tabId, processDetailPid);
  }

  function setProcessPage(processPage: WorkbenchProcessPage) {
    pane.setProcessPage(tabId, processPage);
  }

  function setWindowDetailId(windowDetailId: string | undefined) {
    pane.setWindowDetailId(tabId, windowDetailId);
  }

  function setDirty(dirty: boolean) {
    pane.setTabDirty(tabId, dirty);
  }

  return {
    paneId: pane.paneId,
    tabId,
    tabAtom,
    activeAtom,
    dirtyAtom,
    focusedAtom,
    showCloseAtom,
    selectTab,
    setAgentSession,
    setDaemonClientDetailId,
    setDaemonClientsPageOpen,
    setProcessDetailPid,
    setProcessPage,
    setWindowDetailId,
    setDirty,
    setTerminalSessionSnapshot,
    setTerminalSessionId,
  };
});

function workbenchStorageKey(machineId: string | undefined): string {
  return `rieul.workbench.${machineId ?? "none"}.v1`;
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
  return activatePaneTab({ ...pane, tabs }, tabId);
}

function activatePaneTab(pane: WorkbenchPane, tabId: string): WorkbenchPane {
  if (!pane.tabs.some((tab) => tab.id === tabId)) {
    return normalizePaneTabHistory(pane);
  }
  if (pane.activeTabId === tabId) {
    return normalizePaneTabHistory(pane);
  }
  const previousActiveTabId =
    pane.tabs.some((tab) => tab.id === pane.activeTabId)
      ? pane.activeTabId
      : undefined;
  return normalizePaneTabHistory({
    ...pane,
    activeTabId: tabId,
    tabs: pane.tabs.map((tab) =>
      tab.id === tabId ? { ...tab, previousActiveTabId } : tab
    ),
  });
}

function fallbackActiveTabId(
  closingTab: WorkbenchTab | undefined,
  tabs: WorkbenchTab[],
): string {
  const previousActiveTabId = closingTab?.previousActiveTabId;
  if (
    previousActiveTabId &&
    tabs.some((tab) => tab.id === previousActiveTabId)
  ) {
    return previousActiveTabId;
  }
  return tabs[0]?.id ?? "";
}

function normalizePaneTabHistory(pane: WorkbenchPane): WorkbenchPane {
  const tabIds = new Set(pane.tabs.map((tab) => tab.id));
  const activeTabId = tabIds.has(pane.activeTabId)
    ? pane.activeTabId
    : pane.tabs[0]?.id ?? "";
  const tabs = pane.tabs.map((tab) => {
    if (
      tab.previousActiveTabId &&
      tab.previousActiveTabId !== tab.id &&
      tabIds.has(tab.previousActiveTabId)
    ) {
      return tab;
    }
    if (tab.previousActiveTabId === undefined) return tab;
    return { ...tab, previousActiveTabId: undefined };
  });
  return { ...pane, activeTabId, tabs };
}

function pruneStaleLayoutLeaves(
  layout: LayoutState,
  panes: WorkbenchPane[],
): LayoutState {
  if (panes.length === 0) return layout;

  const paneIds = new Set(panes.map((pane) => pane.id));
  let nextLayout = layout;
  let changed = false;

  while (true) {
    const staleNode = Object.values(nextLayout.nodes).find((node) =>
      node.type === "leaf" && !paneIds.has(node.paneId)
    );
    if (!staleNode) break;
    nextLayout = layoutReducer(nextLayout, {
      nodeId: staleNode.id,
      type: "REMOVE_PANE",
    });
    changed = true;
  }

  if (nextLayout.rootId === null) {
    return createLayout((builder) => builder.leaf(panes[0].id));
  }
  return changed ? nextLayout : layout;
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

function createFilesTab(config: WorkbenchFilesTabConfig = {}): WorkbenchTab {
  const filesView = config.filesView ?? "roots";
  return {
    ...createWorkbenchTab("files"),
    filesView,
    title: titleForFilesView(filesView),
  };
}

function titleForFilesView(filesView: WorkbenchFilesView): string {
  switch (filesView) {
    case "roots":
      return "Root";
    case "home":
      return "Home";
    case "trash":
      return "Trash";
  }
}

function createProcessesTab(
  config: WorkbenchProcessesTabConfig = {},
): WorkbenchTab {
  return {
    id: `processes-${crypto.randomUUID()}`,
    processDetailPid: config.processDetailPid,
    processPage: config.processDetailPid === undefined
      ? undefined
      : config.processPage ?? "detail",
    title: config.title ?? "Processes",
    tool: "processes",
  };
}

function createWindowsTab(
  config: WorkbenchWindowsTabConfig = {},
): WorkbenchTab {
  return {
    id: `windows-${crypto.randomUUID()}`,
    title: config.title ?? "Windows",
    tool: "windows",
    windowDetailId: config.windowDetailId,
  };
}

function createTerminalTab(config: WorkbenchTerminalTabConfig): WorkbenchTab {
  return {
    id: `terminal-${crypto.randomUUID()}`,
    title: config.title ?? "Terminal",
    tool: "terminal",
    terminalLastKnownCwd: config.cwd,
    terminalLastKnownTitle: config.terminalTitle ?? config.title,
    terminalLaunch: config.launch,
    terminalSessionId: config.terminalSessionId,
  };
}

function createAgentTab(
  config: WorkbenchAgentTabConfig = {},
): WorkbenchTab {
  return {
    agentSessionId: config.sessionId,
    id: `agent-${crypto.randomUUID()}`,
    title: config.title ?? "Agent",
    tool: "agent",
  };
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
    case "agent":
      return "Agent";
    case "daemon":
      return "Daemon";
    case "files":
      return "Files";
    case "processes":
      return "Processes";
    case "terminal":
      return "Terminal";
    case "windows":
      return "Windows";
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
      return activatePaneTab(pane, existingTab.id);
    }

    const tab = createWorkbenchTab(tool);
    opened = true;
    return activatePaneTab({ ...pane, tabs: [...pane.tabs, tab] }, tab.id);
  });

  if (!opened) return state;
  return {
    ...state,
    activePaneId,
    panes,
  };
}

function openTabInActivePane(
  state: WorkbenchState,
  tab: WorkbenchTab,
): WorkbenchState {
  const activePaneId = activePaneFromState(state)?.id;
  if (!activePaneId) return state;
  return openTabInPane(state, activePaneId, tab);
}

function openTabInPane(
  state: WorkbenchState,
  paneId: string,
  tab: WorkbenchTab,
): WorkbenchState {
  return {
    ...state,
    activePaneId: paneId,
    panes: state.panes.map((pane) =>
      pane.id === paneId
        ? activatePaneTab({ ...pane, tabs: [...pane.tabs, tab] }, tab.id)
        : pane
    ),
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
  if (tab.tool === "terminal") {
    return createTerminalTab({
      cwd: tab.terminalLastKnownCwd,
      launch: cloneTerminalLaunch(tab.terminalLaunch),
      terminalTitle: tab.terminalLastKnownTitle,
      title: tab.title,
    });
  }
  return {
    ...tab,
    dirty: undefined,
    id: `${tab.tool}-${crypto.randomUUID()}`,
    previousActiveTabId: undefined,
  };
}

function copyTabState(
  machineId: string | undefined,
  sourceTab: WorkbenchTab,
  targetTab: WorkbenchTab,
) {
  if (sourceTab.tool === "files" && targetTab.tool === "files") {
    copyExplorerNavigationState(machineId, sourceTab.id, targetTab.id);
    copyFileViewerState(machineId, sourceTab.id, targetTab.id);
  }
}

function cloneTerminalLaunch(
  launch: TerminalLaunchSpec | undefined,
): TerminalLaunchSpec | undefined {
  if (!launch) return undefined;
  return {
    command: launch.command,
    args: [...launch.args],
  };
}

function terminalLaunchEquals(
  left: TerminalLaunchSpec | undefined,
  right: TerminalLaunchSpec | undefined,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  if (left.command !== right.command) return false;
  if (left.args.length !== right.args.length) return false;
  return left.args.every((arg, index) => arg === right.args[index]);
}
