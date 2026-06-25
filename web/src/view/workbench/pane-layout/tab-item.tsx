import React from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { Folder, Info, Terminal, X } from "lucide-react";
import {
  displayName,
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
import { className } from "../../class-name.ts";

interface WorkbenchTabItemProps {
  dragging: boolean;
  dropPosition?: TabDropPosition;
  nodeId: string;
  paneActive: boolean;
  onClose: () => void;
  onContextMenu: (
    tab: WorkbenchTab,
    event: React.MouseEvent<HTMLDivElement>,
  ) => void;
  onDragStart: () => void;
  onDragEnd: () => void;
  onDragOverTab: (tabId: string, position: TabDropPosition) => void;
  onDropTab: (
    dragData: WorkbenchTabDragData,
    targetTabId: string,
    position: TabDropPosition,
  ) => void;
}

const workbenchTabClassName = [
  "workbench-tab relative flex items-center min-w-0 max-w-[168px] h-full box-border leading-[1.6]",
  "bg-[#eef1f5]",
  "before:content-[''] before:absolute before:top-[4px] before:bottom-[4px]",
  "before:z-[2] before:w-[2px] before:rounded-full before:bg-transparent",
  "before:pointer-events-none before:left-0",
  "after:content-[''] after:absolute after:top-[4px] after:bottom-[4px]",
  "after:z-[2] after:w-[2px] after:rounded-full after:bg-transparent",
  "after:pointer-events-none after:right-0",
  "[&.drop-before::before]:bg-[#4f8cff]",
  "[&.drop-after::after]:bg-[#4f8cff]",
  "[&.dragging]:opacity-48",
  "[&:not(.active)]:[box-shadow:inset_-1px_0_0_#e4e8ef]",
  "[&.active]:bg-white",
  "[&>button]:inline-flex [&>button]:appearance-none [&>button]:items-center",
  "[&>button]:justify-center [&>button]:[font-family:inherit] [&>button]:leading-[1.6]",
  "[&>button]:min-w-0 [&>button]:h-full [&>button]:min-h-0",
  "[&>button]:cursor-pointer [&>button]:border-0 [&>button]:rounded-0 [&>button]:bg-transparent",
  "[&>button]:px-[6px] [&>button]:text-[#344054]",
  "[&>button]:font-700",
  "[&>button:hover]:bg-transparent",
  "[&>button[role='tab']]:justify-start",
  "[&>button[role='tab']]:flex-[1_1_auto]",
  "[&>button[role='tab']]:gap-[6px]",
  "[&>button[role='tab']]:overflow-hidden",
  "[&>button[role='tab']]:text-ellipsis",
  "[&>button[role='tab']]:whitespace-nowrap",
  "[&>button[role='tab']]:cursor-grab",
  "[&>button[role='tab']:active]:cursor-grabbing",
  "[&.active>button]:text-[#20242d]",
  "[&_.tab-close]:flex-[0_0_auto] [&_.tab-close]:w-[2em]",
  "[&_.tab-close]:min-w-[2em] [&_.tab-close]:p-0 [&_.tab-close]:text-[#667085]",
].join(" ");
const workbenchTabIconClassName = "mr-[5px] flex-[0_0_auto] text-[#667085]";
const workbenchTabTitleClassName =
  "min-w-0 overflow-hidden text-ellipsis whitespace-nowrap";
const activePaneTabClassName = [
  "z-[8]",
  "[box-shadow:inset_2px_0_0_#7f9abf,inset_-2px_0_0_#7f9abf,inset_0_2px_0_#7f9abf]",
].join(" ");
const activeTabBottomCoverClassName = [
  "pointer-events-none absolute left-[2px] right-[2px] bottom-0",
  "z-[9] h-[1px] bg-white",
].join(" ");

export function WorkbenchTabItem(
  {
    dragging,
    dropPosition,
    nodeId,
    paneActive,
    onClose,
    onContextMenu,
    onDragStart,
    onDragEnd,
    onDragOverTab,
    onDropTab,
  }: WorkbenchTabItemProps,
) {
  const tabState = useBunja(workbenchTabBunja);
  const tab = useAtomValue(tabState.tabAtom);
  const active = useAtomValue(tabState.activeAtom);
  const dirty = useAtomValue(tabState.dirtyAtom);
  const showClose = useAtomValue(tabState.showCloseAtom);
  const label = useWorkbenchTabLabel(tabState.tabId, tab);
  const tabClassName = className(
    workbenchTabClassName,
    active && "active",
    active && paneActive && activePaneTabClassName,
    dragging && "dragging",
    dropPosition === "before" && "drop-before",
    dropPosition === "after" && "drop-after",
  );

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
      className={tabClassName}
      onContextMenu={(event) => onContextMenu(tab, event)}
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
        <WorkbenchTabIcon
          tool={tab.tool}
          className={workbenchTabIconClassName}
        />
        <span className={workbenchTabTitleClassName}>{label}</span>
      </button>
      {showClose
        ? (
          <button
            type="button"
            className="tab-close"
            draggable={false}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={onClose}
            title={dirty ? "Unsaved changes" : "Close tab"}
            aria-label={dirty
              ? `Close ${label} with unsaved changes`
              : `Close ${label}`}
          >
            {dirty
              ? (
                <span
                  className="block size-[6px] rounded-full bg-[#667085]"
                  aria-hidden="true"
                />
              )
              : <X size={11} />}
          </button>
        )
        : null}
      {active && paneActive
        ? <span className={activeTabBottomCoverClassName} aria-hidden="true" />
        : null}
    </div>
  );
}

interface WorkbenchTabIconProps {
  className: string;
  tool: WorkbenchTab["tool"];
}

function WorkbenchTabIcon(
  { className, tool }: WorkbenchTabIconProps,
) {
  if (tool === "daemon") {
    return <Info size={12} className={className} />;
  }
  if (tool === "terminal") {
    return <Terminal size={12} className={className} />;
  }
  return <Folder size={12} className={className} />;
}

function useWorkbenchTabLabel(
  tabId: string,
  tab: WorkbenchTab | undefined,
): string {
  const navigation = useBunja(explorerNavigationBunja, [
    ExplorerPaneScope.bind(tabId),
  ]);
  const currentPath = useAtomValue(navigation.currentPathAtom);
  const openedFile = useAtomValue(navigation.openedFileAtom);

  if (!tab) return "Files";
  if (tab.tool === "files") {
    return openedFile
      ? displayName(openedFile)
      : folderNameFromPath(currentPath);
  }
  return tab.title;
}

function folderNameFromPath(path?: string): string {
  const crumbs = pathCrumbs(path);
  return crumbs[crumbs.length - 1]?.label ?? "Files";
}
