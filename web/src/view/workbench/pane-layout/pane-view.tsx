import React, { useEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { Handle, useLayout } from "panecake";
import {
  ChevronDown,
  Columns2,
  Folder,
  GripVertical,
  Info,
  Plus,
  Rows2,
  Terminal,
  X,
} from "lucide-react";
import { closeTerminalSession } from "../../../protocol/rpc.ts";
import { machineStoreBunja } from "../../../state/machine-store.ts";
import {
  type TabDropPosition,
  workbenchBunja,
  workbenchPaneBunja,
  type WorkbenchTab,
  WorkbenchTabIdContext,
} from "../../../state/workbench.ts";
import { WorkbenchToolContent } from "../tool/index.tsx";
import {
  hasWorkbenchTabDragData,
  readWorkbenchTabDragData,
  type TabSplitDropSide,
  type WorkbenchTabDragData,
  type WorkbenchTabDropTarget,
} from "./tab-drag.ts";
import { WorkbenchTabItem } from "./tab-item.tsx";
import { className } from "../../class-name.ts";
import { Button } from "../../ui/button.tsx";
import {
  FloatingMenu,
  FloatingMenuItem,
  useFloatingMenuDismiss,
} from "../../ui/floating-menu.tsx";

interface WorkbenchPaneViewProps {
  nodeId: string;
}

const workbenchPaneClassName = [
  "workbench-pane relative grid [grid-template-rows:auto_minmax(0,1fr)]",
  "w-full h-full min-w-0 min-h-0 overflow-hidden bg-white",
].join(" ");
const workbenchPaneHeadClassName = [
  "grid [grid-template-columns:0.8em_minmax(0,1fr)_auto]",
  "items-center h-[1.6em] min-h-[1.6em] box-border leading-[1.6]",
  "border-b border-b-[#d8dde7] bg-[#f6f8fb]",
].join(" ");
const paneHandleClassName =
  "flex items-center justify-center self-stretch text-[#98a2b3] cursor-grab";
const workbenchTabsClassName = [
  "flex items-end min-w-0 h-full overflow-visible",
  "[&.drop-at-end]:[box-shadow:inset_-2px_0_0_#4f8cff]",
].join(" ");
const paneActionsClassName = "flex items-center";
const paneActionButtonGroupClassName =
  "inline-flex h-[1.6rem] items-center box-border p-[2px]";
const newTabMenuWrapClassName = "relative flex h-full";
const iconButtonClassName = [
  "!w-[36px] !min-w-[36px] !p-0",
].join(" ");
const newTabTriggerClassName = [
  "!w-[1.8em] !min-w-[1.8em] !h-full !min-h-0 !box-border !gap-[1px] !p-0",
].join(" ");
const compactIconButtonClassName =
  "!w-[1.6em] !min-w-[1.6em] !h-full !min-h-0 !box-border !p-0";
const buttonGroupFirstClassName = "!rounded-l-[4px] !rounded-r-0";
const buttonGroupMiddleClassName = "-ml-px !rounded-0";
const buttonGroupLastClassName = "-ml-px !rounded-l-0 !rounded-r-[4px]";
const workbenchPaneBodyClassName = [
  "workbench-pane-body relative w-full h-full min-w-0 min-h-0 overflow-visible",
  "before:content-[''] before:absolute before:z-[4]",
  "before:border-2 before:border-[#4f8cff]",
  "before:bg-[rgb(79_140_255_/_16%)] before:opacity-0 before:pointer-events-none",
  "[&.tab-split-left::before]:top-0 [&.tab-split-left::before]:bottom-0",
  "[&.tab-split-left::before]:left-0 [&.tab-split-left::before]:w-1/2",
  "[&.tab-split-left::before]:opacity-100",
  "[&.tab-split-right::before]:top-0 [&.tab-split-right::before]:right-0",
  "[&.tab-split-right::before]:bottom-0 [&.tab-split-right::before]:w-1/2",
  "[&.tab-split-right::before]:opacity-100",
  "[&.tab-split-top::before]:top-0 [&.tab-split-top::before]:right-0",
  "[&.tab-split-top::before]:left-0 [&.tab-split-top::before]:h-1/2",
  "[&.tab-split-top::before]:opacity-100",
  "[&.tab-split-bottom::before]:right-0 [&.tab-split-bottom::before]:bottom-0",
  "[&.tab-split-bottom::before]:left-0 [&.tab-split-bottom::before]:h-1/2",
  "[&.tab-split-bottom::before]:opacity-100",
].join(" ");
const workbenchTabPageClassName = [
  "block w-full h-full min-w-0 min-h-0 overflow-hidden",
  "[container:workbench-tab-page_/_inline-size]",
  "[&[hidden]]:hidden",
].join(" ");
const activePaneOutlineClassName = [
  "pointer-events-none absolute top-[-2px] right-0 bottom-0 left-0 z-[6]",
  "[box-shadow:inset_0_0_0_2px_#7f9abf]",
].join(" ");

export function WorkbenchPaneView(
  {
    nodeId,
  }: WorkbenchPaneViewProps,
) {
  const machineStore = useBunja(machineStoreBunja);
  const workbench = useBunja(workbenchBunja);
  const paneState = useBunja(workbenchPaneBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const panes = useAtomValue(workbench.panesAtom);
  const pane = useAtomValue(paneState.paneAtom);
  const paneCount = useAtomValue(paneState.paneCountAtom);
  const active = useAtomValue(paneState.activeAtom);
  const { removePane: removeLayoutPane, split } = useLayout();
  const [newTabMenuOpen, setNewTabMenuOpen] = useState(false);
  const [draggingTabId, setDraggingTabId] = useState<string>();
  const [tabDropTarget, setTabDropTarget] = useState<WorkbenchTabDropTarget>();
  const [tabSplitDropSide, setTabSplitDropSide] = useState<
    TabSplitDropSide | undefined
  >();
  const newTabMenuRef = useRef<HTMLDivElement>(null);
  const canClosePane = paneCount > 1;
  const hasTabDragState = draggingTabId !== undefined ||
    tabDropTarget !== undefined ||
    tabSplitDropSide !== undefined;

  useFloatingMenuDismiss(
    newTabMenuOpen,
    newTabMenuRef,
    () => setNewTabMenuOpen(false),
  );

  useEffect(() => {
    if (!hasTabDragState) return;

    function clearTabDragState() {
      setDraggingTabId(undefined);
      setTabDropTarget(undefined);
      setTabSplitDropSide(undefined);
    }

    globalThis.addEventListener("dragend", clearTabDragState, true);
    globalThis.addEventListener("drop", clearTabDragState, true);
    return () => {
      globalThis.removeEventListener("dragend", clearTabDragState, true);
      globalThis.removeEventListener("drop", clearTabDragState, true);
    };
  }, [hasTabDragState]);

  function splitPane(direction: "horizontal" | "vertical") {
    const newPaneId = paneState.addPane();
    split(nodeId, direction, newPaneId, "after");
  }

  function closePane() {
    if (!canClosePane) return;
    if (pane) closeTerminalSessions(pane.tabs);
    removeLayoutPane(nodeId);
    paneState.removePane();
  }

  function closeWorkbenchTab(tabId: string) {
    if (!pane) return;
    const closingTab = pane.tabs.find((tab) => tab.id === tabId);
    if (pane.tabs.length > 1) {
      if (closingTab) closeTerminalSessions([closingTab]);
      paneState.closeTab(tabId);
      return;
    }
    closePane();
  }

  function closeTerminalSessions(tabs: WorkbenchTab[]) {
    if (!machine) return;
    const closingTabIds = new Set(tabs.map((tab) => tab.id));
    const remainingTerminalSessionIds = new Set(
      panes.flatMap((pane) => pane.tabs)
        .filter((tab) => !closingTabIds.has(tab.id))
        .flatMap((tab) =>
          tab.tool === "terminal" && tab.terminalSessionId
            ? [tab.terminalSessionId]
            : []
        ),
    );
    const closingTerminalSessionIds = new Set<string>();

    for (const tab of tabs) {
      if (tab.tool !== "terminal" || !tab.terminalSessionId) continue;
      if (remainingTerminalSessionIds.has(tab.terminalSessionId)) continue;
      closingTerminalSessionIds.add(tab.terminalSessionId);
    }

    for (const terminalSessionId of closingTerminalSessionIds) {
      void closeTerminalSession(
        machine,
        terminalSessionId,
        machineStore.rpcCallOptions(),
      ).catch(() => {
        // Closing a tab should not be blocked by a stale connection or session.
      });
    }
  }

  function openFilesTab() {
    paneState.addFilesTab();
    setNewTabMenuOpen(false);
  }

  function openDaemonTab() {
    paneState.addDaemonTab();
    setNewTabMenuOpen(false);
  }

  function openTerminalTab() {
    paneState.addTerminalTab();
    setNewTabMenuOpen(false);
  }

  function moveDroppedTab(
    dragData: WorkbenchTabDragData,
    targetTabId: string | undefined,
    position: TabDropPosition,
  ) {
    const sourcePaneRemoved = paneState.moveTab(
      dragData.paneId,
      dragData.tabId,
      targetTabId,
      position,
    );
    if (sourcePaneRemoved) {
      removeLayoutPane(dragData.nodeId);
    }
    setDraggingTabId(undefined);
    setTabDropTarget(undefined);
    setTabSplitDropSide(undefined);
  }

  function splitDroppedTab(
    dragData: WorkbenchTabDragData,
    side: TabSplitDropSide,
  ) {
    const result = paneState.moveTabToNewPane(
      dragData.paneId,
      dragData.tabId,
    );
    if (!result) {
      setDraggingTabId(undefined);
      setTabDropTarget(undefined);
      setTabSplitDropSide(undefined);
      return;
    }

    const direction = side === "left" || side === "right"
      ? "horizontal"
      : "vertical";
    const position = side === "left" || side === "top" ? "before" : "after";
    split(nodeId, direction, result.newPaneId, position);
    if (result.sourcePaneRemoved) {
      removeLayoutPane(dragData.nodeId);
    }
    setDraggingTabId(undefined);
    setTabDropTarget(undefined);
    setTabSplitDropSide(undefined);
  }

  if (!pane) return null;

  function handleTabStripDragOver(event: React.DragEvent<HTMLDivElement>) {
    if (!hasWorkbenchTabDragData(event)) return;
    if (
      event.target instanceof Element &&
      event.target.closest(".workbench-tab")
    ) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setTabDropTarget({ position: "end" });
  }

  function handleTabStripDrop(event: React.DragEvent<HTMLDivElement>) {
    if (
      event.target instanceof Element &&
      event.target.closest(".workbench-tab")
    ) {
      return;
    }
    const dragData = readWorkbenchTabDragData(event);
    if (!dragData) return;
    event.preventDefault();
    event.stopPropagation();
    moveDroppedTab(dragData, undefined, "end");
  }

  function handleTabStripDragLeave(event: React.DragEvent<HTMLDivElement>) {
    const relatedTarget = event.relatedTarget;
    if (
      relatedTarget instanceof Node &&
      event.currentTarget.contains(relatedTarget)
    ) {
      return;
    }
    setTabDropTarget(undefined);
  }

  function handlePaneBodyDragOver(event: React.DragEvent<HTMLDivElement>) {
    if (!hasWorkbenchTabDragData(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "move";
    setTabSplitDropSide(tabSplitSideFromEvent(event));
  }

  function handlePaneBodyDrop(event: React.DragEvent<HTMLDivElement>) {
    const dragData = readWorkbenchTabDragData(event);
    if (!dragData) return;
    event.preventDefault();
    event.stopPropagation();
    splitDroppedTab(dragData, tabSplitSideFromEvent(event));
  }

  function handlePaneBodyDragLeave(event: React.DragEvent<HTMLDivElement>) {
    const relatedTarget = event.relatedTarget;
    if (
      relatedTarget instanceof Node &&
      event.currentTarget.contains(relatedTarget)
    ) {
      return;
    }
    setTabSplitDropSide(undefined);
  }

  const paneBodyClassName = className(
    workbenchPaneBodyClassName,
    tabSplitDropSide === "left" && "tab-split-left",
    tabSplitDropSide === "right" && "tab-split-right",
    tabSplitDropSide === "top" && "tab-split-top",
    tabSplitDropSide === "bottom" && "tab-split-bottom",
  );

  return (
    <section
      className={className(workbenchPaneClassName, active && "active")}
      onPointerDownCapture={paneState.focusPane}
      onFocusCapture={paneState.focusPane}
    >
      <header className={workbenchPaneHeadClassName}>
        <Handle className={paneHandleClassName}>
          <GripVertical size={8} />
        </Handle>
        <div
          className={className(
            workbenchTabsClassName,
            tabDropTarget?.position === "end" && "drop-at-end",
          )}
          role="tablist"
          onDragOver={handleTabStripDragOver}
          onDrop={handleTabStripDrop}
          onDragLeave={handleTabStripDragLeave}
        >
          {pane.tabs.map((tab) => (
            <WorkbenchTabIdContext key={tab.id} value={tab.id}>
              <WorkbenchTabItem
                dragging={draggingTabId === tab.id}
                dropPosition={tabDropTarget?.tabId === tab.id
                  ? tabDropTarget.position
                  : undefined}
                nodeId={nodeId}
                paneActive={active}
                onClose={() =>
                  closeWorkbenchTab(tab.id)}
                onDragStart={() =>
                  setDraggingTabId(tab.id)}
                onDragEnd={() => {
                  setDraggingTabId(undefined);
                  setTabDropTarget(undefined);
                }}
                onDragOverTab={(tabId, position) =>
                  setTabDropTarget({ tabId, position })}
                onDropTab={moveDroppedTab}
              />
            </WorkbenchTabIdContext>
          ))}
        </div>
        <div className={paneActionsClassName}>
          <div className={paneActionButtonGroupClassName}>
            <div className={newTabMenuWrapClassName} ref={newTabMenuRef}>
              <Button
                className={className(
                  newTabTriggerClassName,
                  buttonGroupFirstClassName,
                )}
                onClick={() => setNewTabMenuOpen((open) => !open)}
                title="Open tab"
                aria-label="Open tab"
                aria-haspopup="menu"
                aria-expanded={newTabMenuOpen}
              >
                <Plus size={12} />
                <ChevronDown size={10} />
              </Button>
              {newTabMenuOpen
                ? (
                  <FloatingMenu
                    className="top-[calc(100%+5px)] right-0 z-[12] w-[148px]"
                    strategy="absolute"
                  >
                    <FloatingMenuItem
                      className="font-650"
                      onClick={openDaemonTab}
                    >
                      <Info size={14} />
                      Daemon
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className="font-650"
                      onClick={openFilesTab}
                    >
                      <Folder size={14} />
                      Files
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className="font-650"
                      onClick={openTerminalTab}
                    >
                      <Terminal size={14} />
                      Terminal
                    </FloatingMenuItem>
                  </FloatingMenu>
                )
                : null}
            </div>
            <Button
              className={className(
                compactIconButtonClassName,
                buttonGroupMiddleClassName,
              )}
              onClick={() => splitPane("horizontal")}
              title="Split right"
              aria-label="Split right"
            >
              <Columns2 size={12} />
            </Button>
            <Button
              className={className(
                compactIconButtonClassName,
                buttonGroupMiddleClassName,
              )}
              onClick={() => splitPane("vertical")}
              title="Split down"
              aria-label="Split down"
            >
              <Rows2 size={12} />
            </Button>
            <Button
              className={className(
                compactIconButtonClassName,
                buttonGroupLastClassName,
              )}
              onClick={closePane}
              disabled={!canClosePane}
              title="Close pane"
              aria-label="Close pane"
            >
              <X size={12} />
            </Button>
          </div>
        </div>
      </header>
      <div
        className={paneBodyClassName}
        onDragOver={handlePaneBodyDragOver}
        onDrop={handlePaneBodyDrop}
        onDragLeave={handlePaneBodyDragLeave}
      >
        {pane.tabs.map((tab) => (
          <WorkbenchTabIdContext key={tab.id} value={tab.id}>
            <section
              className={workbenchTabPageClassName}
              hidden={tab.id !== pane.activeTabId}
            >
              <WorkbenchToolContent />
            </section>
          </WorkbenchTabIdContext>
        ))}
        {active ? <div className={activePaneOutlineClassName} /> : null}
      </div>
    </section>
  );
}

function tabSplitSideFromEvent(
  event: React.DragEvent<HTMLElement>,
): TabSplitDropSide {
  const rect = event.currentTarget.getBoundingClientRect();
  const left = event.clientX - rect.left;
  const right = rect.right - event.clientX;
  const top = event.clientY - rect.top;
  const bottom = rect.bottom - event.clientY;
  const nearest = Math.min(left, right, top, bottom);

  if (nearest === left) return "left";
  if (nearest === right) return "right";
  if (nearest === top) return "top";
  return "bottom";
}
