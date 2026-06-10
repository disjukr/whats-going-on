import React, { useEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { Handle, useLayout } from "panecake";
import {
  ChevronDown,
  Columns2,
  Folder,
  GripVertical,
  Plus,
  Rows2,
  X,
} from "lucide-react";
import {
  type TabDropPosition,
  workbenchPaneBunja,
  WorkbenchTabIdContext,
} from "../../../state/workbench.ts";
import { WorkbenchToolContent } from "../tool/index.tsx";
import {
  hasWorkbenchTabDragData,
  readWorkbenchTabDragData,
  type WorkbenchTabDragData,
  type WorkbenchTabDropTarget,
} from "./tab-drag.ts";
import { WorkbenchTabItem } from "./tab-item.tsx";

interface WorkbenchPaneViewProps {
  nodeId: string;
}

export function WorkbenchPaneView(
  {
    nodeId,
  }: WorkbenchPaneViewProps,
) {
  const paneState = useBunja(workbenchPaneBunja);
  const pane = useAtomValue(paneState.paneAtom);
  const paneCount = useAtomValue(paneState.paneCountAtom);
  const active = useAtomValue(paneState.activeAtom);
  const { removePane: removeLayoutPane, split } = useLayout();
  const [newTabMenuOpen, setNewTabMenuOpen] = useState(false);
  const [draggingTabId, setDraggingTabId] = useState<string>();
  const [tabDropTarget, setTabDropTarget] = useState<WorkbenchTabDropTarget>();
  const newTabMenuRef = useRef<HTMLDivElement>(null);
  const canClosePane = paneCount > 1;

  useEffect(() => {
    if (!newTabMenuOpen) return;

    function closeNewTabMenu(event: MouseEvent) {
      const target = event.target;
      if (target instanceof Node && newTabMenuRef.current?.contains(target)) {
        return;
      }
      setNewTabMenuOpen(false);
    }

    function closeNewTabMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setNewTabMenuOpen(false);
    }

    globalThis.addEventListener("mousedown", closeNewTabMenu);
    globalThis.addEventListener("keydown", closeNewTabMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeNewTabMenu);
      globalThis.removeEventListener("keydown", closeNewTabMenuOnEscape);
    };
  }, [newTabMenuOpen]);

  useEffect(() => {
    if (!draggingTabId) return;

    function clearTabDragState() {
      setDraggingTabId(undefined);
      setTabDropTarget(undefined);
    }

    globalThis.addEventListener("dragend", clearTabDragState, true);
    globalThis.addEventListener("drop", clearTabDragState, true);
    return () => {
      globalThis.removeEventListener("dragend", clearTabDragState, true);
      globalThis.removeEventListener("drop", clearTabDragState, true);
    };
  }, [draggingTabId]);

  function splitPane(direction: "horizontal" | "vertical") {
    const newPaneId = paneState.addPane();
    split(nodeId, direction, newPaneId, "after");
  }

  function closePane() {
    if (!canClosePane) return;
    removeLayoutPane(nodeId);
    paneState.removePane();
  }

  function closeWorkbenchTab(tabId: string) {
    if (!pane) return;
    if (pane.tabs.length > 1) {
      paneState.closeTab(tabId);
      return;
    }
    closePane();
  }

  function openFilesTab() {
    paneState.addFilesTab();
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

  return (
    <section
      className={active ? "workbench-pane active" : "workbench-pane"}
      onPointerDownCapture={paneState.focusPane}
      onFocusCapture={paneState.focusPane}
    >
      <header className="workbench-pane-head">
        <Handle className="pane-handle">
          <GripVertical size={14} />
        </Handle>
        <div
          className={tabDropTarget?.position === "end"
            ? "workbench-tabs drop-at-end"
            : "workbench-tabs"}
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
        <div className="pane-actions">
          <div className="new-tab-menu-wrap" ref={newTabMenuRef}>
            <button
              type="button"
              className="icon-button compact new-tab-trigger"
              onClick={() => setNewTabMenuOpen((open) => !open)}
              title="Open tab"
              aria-label="Open tab"
              aria-haspopup="menu"
              aria-expanded={newTabMenuOpen}
            >
              <Plus size={13} />
              <ChevronDown size={11} />
            </button>
            {newTabMenuOpen
              ? (
                <div className="new-tab-menu" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    onClick={openFilesTab}
                  >
                    <Folder size={14} />
                    Files
                  </button>
                </div>
              )
              : null}
          </div>
          <button
            type="button"
            className="icon-button compact"
            onClick={() => splitPane("horizontal")}
            title="Split right"
            aria-label="Split right"
          >
            <Columns2 size={14} />
          </button>
          <button
            type="button"
            className="icon-button compact"
            onClick={() => splitPane("vertical")}
            title="Split down"
            aria-label="Split down"
          >
            <Rows2 size={14} />
          </button>
          <button
            type="button"
            className="icon-button compact"
            onClick={closePane}
            disabled={!canClosePane}
            title="Close pane"
            aria-label="Close pane"
          >
            <X size={14} />
          </button>
        </div>
      </header>
      <div className="workbench-pane-body">
        {pane.tabs.map((tab) => (
          <WorkbenchTabIdContext key={tab.id} value={tab.id}>
            <section
              className="workbench-tab-page"
              hidden={tab.id !== pane.activeTabId}
            >
              <WorkbenchToolContent />
            </section>
          </WorkbenchTabIdContext>
        ))}
      </div>
    </section>
  );
}
