import { bunja } from "bunja";
import { atom } from "jotai";
import { createLayout, type LayoutState } from "panecake";
import { JotaiStoreScope } from "./jotai-store.ts";

export type WorkbenchFeature = "files" | "processes" | "terminal";
export type WorkbenchTool = "files";

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

export type TabDropPosition = "before" | "after" | "end";

const initialPaneId = "pane-1";
const initialTab = createFilesTab();

const initialLayout = createLayout((builder) => builder.leaf(initialPaneId));

export const workbenchBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);

  const layoutAtom = atom<LayoutState>(initialLayout);
  const activeFeatureAtom = atom<WorkbenchFeature>("files");
  const panesAtom = atom<WorkbenchPane[]>([
    {
      id: initialPaneId,
      tabs: [initialTab],
      activeTabId: initialTab.id,
    },
  ]);

  function setLayout(layout: LayoutState) {
    store.set(layoutAtom, layout);
  }

  function selectFeature(feature: WorkbenchFeature) {
    store.set(activeFeatureAtom, feature);
  }

  function addPane(): string {
    const pane: WorkbenchPane = {
      id: `pane-${crypto.randomUUID()}`,
      tabs: [createFilesTab()],
      activeTabId: "",
    };
    pane.activeTabId = pane.tabs[0].id;
    store.set(panesAtom, (current) => [...current, pane]);
    return pane.id;
  }

  function removePane(paneId: string) {
    store.set(
      panesAtom,
      (current) =>
        current.length <= 1
          ? current
          : current.filter((pane) => pane.id !== paneId),
    );
  }

  function addFilesTab(paneId: string) {
    const tab = createFilesTab();
    store.set(
      panesAtom,
      (current) =>
        current.map((pane) =>
          pane.id === paneId
            ? {
              ...pane,
              tabs: [...pane.tabs, tab],
              activeTabId: tab.id,
            }
            : pane
        ),
    );
  }

  function selectTab(paneId: string, tabId: string) {
    store.set(
      panesAtom,
      (current) =>
        current.map((pane) =>
          pane.id === paneId ? { ...pane, activeTabId: tabId } : pane
        ),
    );
  }

  function closeTab(paneId: string, tabId: string) {
    store.set(
      panesAtom,
      (current) =>
        current.map((pane) => {
          if (pane.id !== paneId || pane.tabs.length <= 1) return pane;
          const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
          const activeTabId = pane.activeTabId === tabId
            ? tabs[0].id
            : pane.activeTabId;
          return { ...pane, tabs, activeTabId };
        }),
    );
  }

  function moveTab(
    sourcePaneId: string,
    tabId: string,
    targetPaneId: string,
    targetTabId: string | undefined,
    position: TabDropPosition,
  ) {
    store.set(panesAtom, (current) => {
      const sourcePane = current.find((pane) => pane.id === sourcePaneId);
      const targetPane = current.find((pane) => pane.id === targetPaneId);
      const movingTab = sourcePane?.tabs.find((tab) => tab.id === tabId);
      if (!sourcePane || !targetPane || !movingTab) return current;

      if (sourcePaneId !== targetPaneId && sourcePane.tabs.length <= 1) {
        return current;
      }

      return current.map((pane) => {
        if (sourcePaneId === targetPaneId && pane.id === sourcePaneId) {
          return moveTabWithinPane(pane, tabId, targetTabId, position);
        }

        if (pane.id === sourcePaneId) {
          const tabs = pane.tabs.filter((tab) => tab.id !== tabId);
          const activeTabId = pane.activeTabId === tabId
            ? tabs[0]?.id ?? ""
            : pane.activeTabId;
          return { ...pane, tabs, activeTabId };
        }

        if (pane.id === targetPaneId) {
          const tabs = insertTab(pane.tabs, movingTab, targetTabId, position);
          return { ...pane, tabs, activeTabId: movingTab.id };
        }

        return pane;
      });
    });
  }

  return {
    layoutAtom,
    activeFeatureAtom,
    panesAtom,
    setLayout,
    selectFeature,
    addPane,
    removePane,
    addFilesTab,
    selectTab,
    closeTab,
    moveTab,
  };
});

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
  return {
    id: `files-${crypto.randomUUID()}`,
    title: "Files",
    tool: "files",
  };
}
