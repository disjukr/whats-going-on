import React from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { Folder, X } from "lucide-react";
import {
  explorerNavigationBunja,
  ExplorerPaneScope,
  pathCrumbs,
} from "../../../state/explorer.ts";
import {
  type TabDropPosition,
  type WorkbenchTab,
  workbenchTabBunja,
} from "../../../state/workbench.ts";
import {
  hasWorkbenchTabDragData,
  readWorkbenchTabDragData,
  type WorkbenchTabDragData,
  workbenchTabDragType,
} from "./tab-drag.ts";

interface WorkbenchTabItemProps {
  dragging: boolean;
  dropPosition?: TabDropPosition;
  nodeId: string;
  onClose: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDragOverTab: (tabId: string, position: TabDropPosition) => void;
  onDropTab: (
    dragData: WorkbenchTabDragData,
    targetTabId: string,
    position: TabDropPosition,
  ) => void;
}

export function WorkbenchTabItem(
  {
    dragging,
    dropPosition,
    nodeId,
    onClose,
    onDragStart,
    onDragEnd,
    onDragOverTab,
    onDropTab,
  }: WorkbenchTabItemProps,
) {
  const tabState = useBunja(workbenchTabBunja);
  const tab = useAtomValue(tabState.tabAtom);
  const active = useAtomValue(tabState.activeAtom);
  const showClose = useAtomValue(tabState.showCloseAtom);
  const label = useWorkbenchTabLabel(tabState.tabId, tab);
  const className = [
    "workbench-tab",
    active ? "active" : "",
    dragging ? "dragging" : "",
    dropPosition === "before" ? "drop-before" : "",
    dropPosition === "after" ? "drop-after" : "",
  ].filter(Boolean).join(" ");

  if (!tab) return null;
  const currentTabId = tab.id;

  function handleDragOver(event: React.DragEvent<HTMLDivElement>) {
    if (!hasWorkbenchTabDragData(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    const rect = event.currentTarget.getBoundingClientRect();
    const position = event.clientX < rect.left + rect.width / 2
      ? "before"
      : "after";
    onDragOverTab(currentTabId, position);
  }

  function handleDrop(event: React.DragEvent<HTMLDivElement>) {
    const dragData = readWorkbenchTabDragData(event);
    if (!dragData) return;
    event.preventDefault();
    event.stopPropagation();
    const rect = event.currentTarget.getBoundingClientRect();
    const position = event.clientX < rect.left + rect.width / 2
      ? "before"
      : "after";
    onDropTab(dragData, currentTabId, position);
  }

  return (
    <div
      className={className}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
    >
      <button
        type="button"
        role="tab"
        aria-selected={active}
        draggable
        onDragStart={(event) => {
          event.stopPropagation();
          event.dataTransfer.effectAllowed = "move";
          event.dataTransfer.setData(
            workbenchTabDragType,
            JSON.stringify({
              nodeId,
              paneId: tabState.paneId,
              tabId: currentTabId,
            }),
          );
          onDragStart();
        }}
        onDragEnd={onDragEnd}
        onClick={tabState.selectTab}
        title={label}
      >
        <Folder size={14} className="workbench-tab-icon" />
        <span className="workbench-tab-title">{label}</span>
      </button>
      {showClose
        ? (
          <button
            type="button"
            className="tab-close"
            draggable={false}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onClose}
            title="Close tab"
            aria-label={`Close ${label}`}
          >
            <X size={13} />
          </button>
        )
        : null}
    </div>
  );
}

function useWorkbenchTabLabel(
  tabId: string,
  tab: WorkbenchTab | undefined,
): string {
  const navigation = useBunja(explorerNavigationBunja, [
    ExplorerPaneScope.bind(tabId),
  ]);
  const currentPath = useAtomValue(navigation.currentPathAtom);

  if (!tab) return "Files";
  if (tab.tool === "files") return folderNameFromPath(currentPath);
  return tab.title;
}

function folderNameFromPath(path?: string): string {
  const crumbs = pathCrumbs(path);
  return crumbs[crumbs.length - 1]?.label ?? "Files";
}
