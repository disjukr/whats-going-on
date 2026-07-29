import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useDroppable } from "@dnd-kit/core";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { Handle, useLayout } from "panecake";
import {
  Activity,
  AppWindow,
  Bot,
  Columns2,
  Copy,
  Folder,
  GripVertical,
  Info,
  MoreHorizontal,
  Plus,
  Rows2,
  Terminal,
  Unlink,
  X,
} from "lucide-react";
import { closeTerminalSession } from "../../../protocol/generated/client.ts";
import { machineStoreBunja } from "../../../state/machine-store.ts";
import { rpcSessionBunja } from "../../../state/rpc-session.ts";
import { terminalShellsBunja } from "../../../state/terminal-shells.ts";
import {
  type TabDropPosition,
  workbenchBunja,
  workbenchPaneBunja,
  type WorkbenchTab,
  WorkbenchTabIdContext,
} from "../../../state/workbench.ts";
import { WorkbenchToolContent } from "../tool/index.tsx";
import {
  type TabSplitDropSide,
  workbenchPaneBodyDropId,
  type WorkbenchTabDropTarget,
  workbenchTabStripDropId,
} from "./tab-drag.ts";
import { WorkbenchTabItem } from "./tab-item.tsx";
import { className } from "../../class-name.ts";
import { Button } from "../../ui/button.tsx";
import {
  clampFloatingMenuPosition,
  FloatingMenu,
  FloatingMenuItem,
  useFloatingMenuDismiss,
} from "../../ui/floating-menu.tsx";

interface WorkbenchPaneViewProps {
  canSplit: boolean;
  draggingTabId?: string;
  nodeId: string;
  tabDropTarget?: WorkbenchTabDropTarget;
  tabSplitDropSide?: TabSplitDropSide;
  topRight: boolean;
}

type PendingCloseRequest =
  | { dirtyCount: number; kind: "pane" }
  | { dirtyCount: number; kind: "tab"; tabId: string };
type SplitDirection = "horizontal" | "vertical";

interface TabContextMenuState {
  tab: WorkbenchTab;
  x: number;
  y: number;
}

const workbenchPaneOuterClassName = [
  "workbench-pane relative",
  "m-[4px] h-[calc(100%-8px)] w-[calc(100%-8px)] min-w-0 min-h-0",
  "overflow-visible rounded-[14px]",
  "transition-[filter,opacity,transform] duration-150 ease-out",
  "[&:not(.active)]:opacity-86",
  "[&:not(.active)_.workbench-pane-head]:opacity-80",
  "[&:not(.active)_.workbench-tab.active]:opacity-72",
  "[&:not(.active)_.workbench-pane-actions]:opacity-54",
  "[&:not(.active)_.file-row.selected]:bg-[rgba(62,84,116,0.09)]",
  "[&:not(.active)_.file-row.selected]:shadow-none",
  "[&:not(.active)_.file-row.selected_.file-cell]:text-rieul-text-2",
  "[&.active_.workbench-pane-surface]:after:border-[3px]",
  "[&.active_.workbench-pane-surface]:after:border-rieul-focus",
  "max-[680px]:m-0 max-[680px]:h-full max-[680px]:w-full",
  "max-[680px]:rounded-none",
  "max-[680px]:[&_.workbench-pane-surface]:after:hidden",
].join(" ");
const workbenchPaneSurfaceClassName = [
  "workbench-pane-surface relative grid h-full w-full min-w-0 min-h-0",
  "[grid-template-rows:auto_minmax(0,1fr)] overflow-hidden rounded-[14px]",
  "border border-transparent bg-[rgba(248,248,249,0.72)] backdrop-blur-xl",
  "after:content-[''] after:pointer-events-none after:absolute after:inset-0",
  "after:z-[8] after:box-border after:rounded-[14px]",
  "after:border-[1.5px] after:border-[rgba(108,126,151,0.38)]",
  "after:transition-[border-color,border-width] after:duration-150 after:ease-out",
  "max-[680px]:rounded-none max-[680px]:border-x-0 max-[680px]:border-t-0",
].join(" ");
const workbenchPaneHeadClassName = [
  "workbench-pane-head",
  "grid [grid-template-columns:1em_minmax(0,1fr)_auto]",
  "items-center h-[36px] min-h-[36px] box-border overflow-visible leading-none",
  "rounded-t-[14px] bg-transparent",
  "px-[6px] pt-0 backdrop-blur-2xl",
  "max-[680px]:[grid-template-columns:minmax(0,1fr)_auto]",
  "max-[680px]:h-[34px] max-[680px]:min-h-[34px] max-[680px]:rounded-none",
  "max-[680px]:px-[4px]",
].join(" ");
const paneHandleClassName =
  "flex items-center justify-center self-stretch text-rieul-muted cursor-grab max-[680px]:hidden";
const workbenchTabsClassName = [
  "workbench-tabs flex h-full min-w-0 flex-1 items-center gap-[4px] overflow-x-auto overflow-y-visible",
  "[scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
].join(" ");
const paneAddTabButtonWrapClassName = [
  "sticky right-0 z-[12] flex h-[28px] flex-[0_0_auto]",
  "bg-[rgba(248,248,249,0.86)] pl-[4px]",
  "shadow-[-10px_0_14px_rgba(248,248,249,0.86)] backdrop-blur-xl",
].join(" ");
const paneActionsClassName = "workbench-pane-actions flex items-center";
const paneActionButtonGroupClassName =
  "inline-flex h-[28px] items-center gap-[4px] box-border p-0";
const paneOverflowMenuWrapClassName = "relative flex h-full";
const compactIconButtonClassName =
  "!w-[28px] !min-w-[28px] !h-full !min-h-0 !box-border !rounded-rieul-sm !p-0 !border-transparent !bg-transparent !text-rieul-text-3 hover:!border-transparent hover:!bg-white/24 hover:!text-rieul-text";
const buttonGroupFirstClassName = "";
const buttonGroupLastClassName = "";
const standaloneButtonClassName = "!rounded-[4px]";
const paneCreateMenuClassName = "top-full right-0 z-[12] w-[188px]";
const paneOverflowMenuClassName = "top-full right-0 z-[12] w-[172px]";
const tabContextMenuWidth = 168;
const paneOverflowMenuItemClassName = "";
const paneMobileTabMenuDividerClassName =
  "hidden max-[680px]:block border-t border-t-rieul-border";
const paneMobileTabMenuItemClassName = "hidden max-[680px]:inline-flex";
const workbenchPaneBodyClassName = [
  "workbench-pane-body relative w-full h-full min-w-0 min-h-0 overflow-hidden",
  "rounded-[12px] border border-white/58 bg-[rgba(253,253,253,0.94)]",
  "max-[680px]:rounded-none max-[680px]:border-x-0 max-[680px]:border-b-0",
  "before:content-[''] before:absolute before:z-[4]",
  "before:border-2 before:border-rieul-accent",
  "before:bg-rieul-accent-muted before:opacity-0 before:pointer-events-none",
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
const closeConfirmBackdropClassName =
  "fixed inset-0 z-[20] grid place-items-center bg-rieul-overlay p-[24px]";
const closeConfirmModalClassName = [
  "w-[min(420px,100%)] overflow-hidden border border-rieul-border",
  "rounded-rieul-xl bg-rieul-surface shadow-rieul-lg",
].join(" ");
const closeConfirmHeadClassName = [
  "flex items-center justify-between gap-[12px] border-b border-b-rieul-border",
  "px-[16px] py-[14px]",
  "[&_div]:grid [&_div]:gap-[2px] [&_div]:min-w-0",
  "[&_span]:text-rieul-text-3 [&_span]:text-[13px] [&_span]:font-600",
  "[&_h2]:m-0 [&_h2]:text-rieul-text [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");
const closeConfirmIconButtonClassName = "!w-[36px] !min-w-[36px] !p-0";
const closeConfirmBodyClassName = [
  "grid gap-[14px] p-[16px]",
  "[&_p]:m-0 [&_p]:text-rieul-text-2 [&_p]:text-[14px]",
].join(" ");
const closeConfirmActionsClassName = "flex justify-end gap-[8px]";
const closeConfirmDangerButtonClassName = [
  "border-rieul-danger bg-rieul-danger-soft text-rieul-danger",
  "hover:border-rieul-danger hover:bg-rieul-danger-soft hover:text-rieul-danger",
].join(" ");

export function WorkbenchPaneView(
  {
    canSplit,
    draggingTabId,
    nodeId,
    tabDropTarget,
    tabSplitDropSide,
    topRight,
  }: WorkbenchPaneViewProps,
) {
  const machineStore = useBunja(machineStoreBunja);
  const rpcSession = useBunja(rpcSessionBunja);
  const terminalShellsState = useBunja(terminalShellsBunja);
  const workbench = useBunja(workbenchBunja);
  const paneState = useBunja(workbenchPaneBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const defaultShell = useAtomValue(terminalShellsState.defaultShellAtom);
  const panes = useAtomValue(workbench.panesAtom);
  const pane = useAtomValue(paneState.paneAtom);
  const paneCount = useAtomValue(paneState.paneCountAtom);
  const active = useAtomValue(paneState.activeAtom);
  const { removePane: removeLayoutPane, split } = useLayout();
  const [paneCreateMenuOpen, setPaneCreateMenuOpen] = useState(false);
  const [paneOverflowMenuOpen, setPaneOverflowMenuOpen] = useState(false);
  const [pendingCloseRequest, setPendingCloseRequest] = useState<
    PendingCloseRequest | undefined
  >();
  const [tabContextMenu, setTabContextMenu] = useState<TabContextMenuState>();
  const [preferredSplitDirection, setPreferredSplitDirection] = useState<
    SplitDirection
  >("horizontal");
  const paneRef = useRef<HTMLElement>(null);
  const paneOverflowMenuRef = useRef<HTMLDivElement>(null);
  const paneCreateMenuRef = useRef<HTMLDivElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);
  const tabContextMenuRef = useRef<HTMLDivElement>(null);
  const canClosePane = paneCount > 1;
  const { setNodeRef: setTabStripDropNodeRef } = useDroppable({
    id: workbenchTabStripDropId(paneState.paneId),
    data: {
      kind: "tab-strip",
      paneId: paneState.paneId,
      type: "workbench-tab-drop",
    },
  });
  const { setNodeRef: setPaneBodyDropNodeRef } = useDroppable({
    disabled: !canSplit,
    id: workbenchPaneBodyDropId(paneState.paneId),
    data: {
      kind: "pane-body",
      nodeId,
      paneId: paneState.paneId,
      type: "workbench-tab-drop",
    },
  });

  useFloatingMenuDismiss(
    paneCreateMenuOpen,
    paneCreateMenuRef,
    () => setPaneCreateMenuOpen(false),
  );
  useFloatingMenuDismiss(
    paneOverflowMenuOpen,
    paneOverflowMenuRef,
    () => setPaneOverflowMenuOpen(false),
  );
  useFloatingMenuDismiss(
    tabContextMenu !== undefined,
    tabContextMenuRef,
    () => setTabContextMenu(undefined),
  );

  useLayoutEffect(() => {
    const tabScroller = tabsRef.current;
    if (!pane?.activeTabId || !tabScroller) return;
    const activeTab = tabScroller.querySelector<HTMLElement>(
      ".workbench-tab.active",
    );
    if (!activeTab) return;

    const stickyAddButtonWidth = paneCreateMenuRef.current?.offsetWidth ?? 0;
    const gap = 4;
    const visibleLeft = tabScroller.scrollLeft;
    const visibleRight = tabScroller.scrollLeft + tabScroller.clientWidth -
      stickyAddButtonWidth - gap;
    const tabLeft = activeTab.offsetLeft;
    const tabRight = activeTab.offsetLeft + activeTab.offsetWidth;

    if (tabLeft < visibleLeft) {
      tabScroller.scrollTo({ left: tabLeft, behavior: "smooth" });
      return;
    }
    if (tabRight > visibleRight) {
      tabScroller.scrollTo({
        left: tabRight - tabScroller.clientWidth + stickyAddButtonWidth + gap,
        behavior: "smooth",
      });
    }
  }, [pane?.activeTabId, pane?.tabs.length]);

  useLayoutEffect(() => {
    const element = paneRef.current;
    if (!element) return;

    function updateSplitDirection(width: number, height: number) {
      if (width <= 0 || height <= 0) return;
      const next = width >= height ? "horizontal" : "vertical";
      setPreferredSplitDirection((current) =>
        current === next ? current : next
      );
    }

    const rect = element.getBoundingClientRect();
    updateSplitDirection(rect.width, rect.height);
    const resizeObserver = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      updateSplitDirection(entry.contentRect.width, entry.contentRect.height);
    });
    resizeObserver.observe(element);
    return () => resizeObserver.disconnect();
  }, []);

  function splitPane(direction: SplitDirection) {
    if (!canSplit) return;
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
    const newPaneId = paneState.addPane();
    split(nodeId, direction, newPaneId, "after");
  }

  useEffect(() => {
    if (!pendingCloseRequest) return;

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setPendingCloseRequest(undefined);
    }

    globalThis.addEventListener("keydown", closeOnEscape);
    return () => globalThis.removeEventListener("keydown", closeOnEscape);
  }, [pendingCloseRequest]);

  function requestClosePane() {
    if (!canClosePane) return;
    const dirtyCount = pane ? dirtyTabCount(pane.tabs) : 0;
    if (dirtyCount > 0) {
      setPendingCloseRequest({ dirtyCount, kind: "pane" });
      return;
    }
    performClosePane();
  }

  function performClosePane() {
    if (!canClosePane) return;
    setPaneOverflowMenuOpen(false);
    if (pane) closeTerminalSessions(pane.tabs);
    removeLayoutPane(nodeId);
    paneState.removePane();
  }

  function requestCloseWorkbenchTab(tabId: string) {
    setTabContextMenu(undefined);
    setPaneOverflowMenuOpen(false);
    if (!pane) return;
    const closingTab = pane.tabs.find((tab) => tab.id === tabId);
    const dirtyCount = closingTab?.dirty ? 1 : 0;
    if (dirtyCount > 0) {
      setPendingCloseRequest({ dirtyCount, kind: "tab", tabId });
      return;
    }
    performCloseWorkbenchTab(tabId);
  }

  function performCloseWorkbenchTab(tabId: string) {
    if (!pane) return;
    const closingTab = pane.tabs.find((tab) => tab.id === tabId);
    if (pane.tabs.length > 1) {
      if (closingTab) closeTerminalSessions([closingTab]);
      paneState.closeTab(tabId);
      return;
    }
    performClosePane();
  }

  function detachTerminalTab(tabId: string) {
    setTabContextMenu(undefined);
    setPaneOverflowMenuOpen(false);
    if (!pane) return;
    if (pane.tabs.length > 1) {
      paneState.closeTab(tabId);
      return;
    }
    if (canClosePane) {
      removeLayoutPane(nodeId);
      paneState.removePane();
      return;
    }
    paneState.addFilesTab();
    paneState.closeTab(tabId);
  }

  function duplicateWorkbenchTab(tabId: string) {
    setTabContextMenu(undefined);
    setPaneOverflowMenuOpen(false);
    paneState.duplicateTab(tabId);
  }

  function openTabContextMenu(
    tab: WorkbenchTab,
    event: React.MouseEvent<HTMLDivElement>,
  ) {
    event.preventDefault();
    event.stopPropagation();
    paneState.selectTab(tab.id);
    const position = tabContextMenuPosition(
      event.clientX,
      event.clientY,
      tab.tool === "terminal",
    );
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
    setTabContextMenu({ tab, ...position });
  }

  function confirmPendingCloseRequest() {
    const request = pendingCloseRequest;
    if (!request) return;
    setPendingCloseRequest(undefined);
    if (request.kind === "tab") {
      performCloseWorkbenchTab(request.tabId);
      return;
    }
    performClosePane();
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
      void (async () => {
        const transport = await rpcSession.webTransport();
        await closeTerminalSession(transport, { terminalSessionId });
      })().catch(() => {
        // Closing a tab should not be blocked by a stale connection or session.
      });
    }
  }

  function openFilesTab() {
    paneState.addFilesTab();
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  function openDaemonTab() {
    paneState.addDaemonTab();
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  function openAgentTab() {
    paneState.addAgentTab();
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  function openProcessesTab() {
    paneState.addProcessesTab();
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  function openWindowsTab() {
    paneState.addWindowsTab();
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  function openTerminalTab() {
    const shell = defaultShell;
    paneState.addTerminalTab(
      shell
        ? {
          launch: {
            command: shell.command,
            args: shell.args,
          },
          title: shell.name,
        }
        : {},
    );
    setPaneCreateMenuOpen(false);
    setPaneOverflowMenuOpen(false);
  }

  if (!pane) return null;

  const paneBodyClassName = className(
    workbenchPaneBodyClassName,
    tabSplitDropSide === "left" && "tab-split-left",
    tabSplitDropSide === "right" && "tab-split-right",
    tabSplitDropSide === "top" && "tab-split-top",
    tabSplitDropSide === "bottom" && "tab-split-bottom",
  );
  const activeTab = pane?.tabs.find((tab) => tab.id === pane.activeTabId);
  const splitButtonLabel = splitDirectionLabel(preferredSplitDirection);

  return (
    <section
      ref={paneRef}
      className={className(workbenchPaneOuterClassName, active && "active")}
      data-node-id={nodeId}
      data-pane-id={paneState.paneId}
      onPointerDownCapture={paneState.focusPane}
      onFocusCapture={paneState.focusPane}
    >
      <div className={workbenchPaneSurfaceClassName}>
        <header className={workbenchPaneHeadClassName}>
          <Handle className={paneHandleClassName}>
            <GripVertical size={13} strokeWidth={1.8} />
          </Handle>
          <div
            className={workbenchTabsClassName}
            ref={(element) => {
              tabsRef.current = element;
              setTabStripDropNodeRef(element);
            }}
            data-pane-id={paneState.paneId}
            role="tablist"
          >
            {pane.tabs.map((tab, index) => (
              <WorkbenchTabIdContext key={tab.id} value={tab.id}>
                <WorkbenchTabItem
                  dragging={draggingTabId === tab.id}
                  dropPosition={tabDropPositionForTab(
                    tabDropTarget,
                    tab,
                    index === pane.tabs.length - 1,
                  )}
                  nodeId={nodeId}
                  paneActive={active}
                  onClose={() =>
                    requestCloseWorkbenchTab(tab.id)}
                  onContextMenu={openTabContextMenu}
                />
              </WorkbenchTabIdContext>
            ))}
            <div
              className={paneAddTabButtonWrapClassName}
              ref={paneCreateMenuRef}
              role="presentation"
            >
              <Button
                size="icon"
                variant="ghost"
                className={className(
                  compactIconButtonClassName,
                  standaloneButtonClassName,
                )}
                onClick={() => {
                  setPaneOverflowMenuOpen(false);
                  setPaneCreateMenuOpen((open) => !open);
                }}
                title="New tab"
                aria-label="New tab"
                aria-haspopup="menu"
                aria-expanded={paneCreateMenuOpen}
              >
                <Plus size={13} />
              </Button>
              {paneCreateMenuOpen
                ? (
                  <FloatingMenu
                    align="end"
                    anchorRef={paneCreateMenuRef}
                    className={paneCreateMenuClassName}
                    strategy="absolute"
                  >
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openDaemonTab}
                    >
                      <Info size={14} />
                      New Daemon Tab
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openAgentTab}
                    >
                      <Bot size={14} />
                      New Agent Tab
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openFilesTab}
                    >
                      <Folder size={14} />
                      New Files Tab
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openTerminalTab}
                    >
                      <Terminal size={14} />
                      New Terminal Tab
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openProcessesTab}
                    >
                      <Activity size={14} />
                      New Processes Tab
                    </FloatingMenuItem>
                    <FloatingMenuItem
                      className={paneOverflowMenuItemClassName}
                      onClick={openWindowsTab}
                    >
                      <AppWindow size={14} />
                      New Windows Tab
                    </FloatingMenuItem>
                  </FloatingMenu>
                )
                : null}
            </div>
          </div>
          <div className={paneActionsClassName}>
            <div className={paneActionButtonGroupClassName}>
              {topRight &&
                  canSplit
                ? (
                  <Button
                    size="icon"
                    variant="ghost"
                    className={className(
                      compactIconButtonClassName,
                      buttonGroupFirstClassName,
                    )}
                    onClick={() => splitPane(preferredSplitDirection)}
                    title={splitButtonLabel}
                    aria-label={splitButtonLabel}
                  >
                    {preferredSplitDirection === "horizontal"
                      ? <Columns2 size={12} />
                      : <Rows2 size={12} />}
                  </Button>
                )
                : null}
              <div
                className={paneOverflowMenuWrapClassName}
                ref={paneOverflowMenuRef}
              >
                <Button
                  size="icon"
                  variant="ghost"
                  className={className(
                    compactIconButtonClassName,
                    topRight
                      ? buttonGroupLastClassName
                      : standaloneButtonClassName,
                  )}
                  onClick={() => {
                    setPaneCreateMenuOpen(false);
                    setPaneOverflowMenuOpen((open) => !open);
                  }}
                  title="Pane actions"
                  aria-label="Pane actions"
                  aria-haspopup="menu"
                  aria-expanded={paneOverflowMenuOpen}
                >
                  <MoreHorizontal size={13} />
                </Button>
                {paneOverflowMenuOpen
                  ? (
                    <FloatingMenu
                      align="end"
                      anchorRef={paneOverflowMenuRef}
                      className={paneOverflowMenuClassName}
                      strategy="absolute"
                    >
                      {activeTab
                        ? (
                          <>
                            <div
                              className={paneMobileTabMenuDividerClassName}
                              aria-hidden="true"
                            />
                            <FloatingMenuItem
                              className={paneMobileTabMenuItemClassName}
                              onClick={() =>
                                duplicateWorkbenchTab(activeTab.id)}
                            >
                              <Copy size={14} />
                              Duplicate
                            </FloatingMenuItem>
                            {activeTab.tool === "terminal"
                              ? (
                                <FloatingMenuItem
                                  className={paneMobileTabMenuItemClassName}
                                  onClick={() =>
                                    detachTerminalTab(activeTab.id)}
                                >
                                  <Unlink size={14} />
                                  Detach
                                </FloatingMenuItem>
                              )
                              : null}
                            <FloatingMenuItem
                              className={paneMobileTabMenuItemClassName}
                              tone={activeTab.dirty ? "danger" : "neutral"}
                              onClick={() =>
                                requestCloseWorkbenchTab(activeTab.id)}
                            >
                              <X size={14} />
                              Close tab
                            </FloatingMenuItem>
                            <div
                              className={paneMobileTabMenuDividerClassName}
                              aria-hidden="true"
                            />
                          </>
                        )
                        : null}
                      {canSplit
                        ? (
                          <>
                            <FloatingMenuItem
                              className={paneOverflowMenuItemClassName}
                              onClick={() => splitPane("horizontal")}
                            >
                              <Columns2 size={14} />
                              Split right
                            </FloatingMenuItem>
                            <FloatingMenuItem
                              className={paneOverflowMenuItemClassName}
                              onClick={() => splitPane("vertical")}
                            >
                              <Rows2 size={14} />
                              Split down
                            </FloatingMenuItem>
                          </>
                        )
                        : null}
                      <FloatingMenuItem
                        className={paneOverflowMenuItemClassName}
                        onClick={requestClosePane}
                        disabled={!canClosePane}
                      >
                        <X size={14} />
                        Close pane
                      </FloatingMenuItem>
                    </FloatingMenu>
                  )
                  : null}
              </div>
            </div>
          </div>
        </header>
        <div
          ref={setPaneBodyDropNodeRef}
          className={paneBodyClassName}
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
        </div>
      </div>
      {tabContextMenu
        ? (
          <FloatingMenu
            className="z-[30] w-[168px]"
            menuRef={tabContextMenuRef}
            position={{ left: tabContextMenu.x, top: tabContextMenu.y }}
          >
            <FloatingMenuItem
              onClick={() => duplicateWorkbenchTab(tabContextMenu.tab.id)}
            >
              <Copy size={14} />
              Duplicate
            </FloatingMenuItem>
            {tabContextMenu.tab.tool === "terminal"
              ? (
                <FloatingMenuItem
                  onClick={() => detachTerminalTab(tabContextMenu.tab.id)}
                >
                  <Unlink size={14} />
                  Detach
                </FloatingMenuItem>
              )
              : null}
            <FloatingMenuItem
              tone={tabContextMenu.tab.dirty ? "danger" : "neutral"}
              onClick={() => requestCloseWorkbenchTab(tabContextMenu.tab.id)}
            >
              <X size={14} />
              Close
            </FloatingMenuItem>
          </FloatingMenu>
        )
        : null}
      {pendingCloseRequest
        ? (
          <UnsavedCloseConfirmModal
            dirtyCount={pendingCloseRequest.dirtyCount}
            kind={pendingCloseRequest.kind}
            onCancel={() => setPendingCloseRequest(undefined)}
            onConfirm={confirmPendingCloseRequest}
          />
        )
        : null}
    </section>
  );
}

function tabDropPositionForTab(
  target: WorkbenchTabDropTarget | undefined,
  tab: WorkbenchTab,
  last: boolean,
): TabDropPosition | undefined {
  if (!target) return undefined;
  if (target.tabId === tab.id) return target.position;
  if (target.position === "end" && last) return "after";
  return undefined;
}

function tabContextMenuPosition(
  x: number,
  y: number,
  terminal: boolean,
): { x: number; y: number } {
  const position = clampFloatingMenuPosition(x, y, {
    itemCount: terminal ? 3 : 2,
    width: tabContextMenuWidth,
  });
  return { x: position.left, y: position.top };
}

function splitDirectionLabel(direction: SplitDirection): string {
  return direction === "horizontal" ? "Split right" : "Split down";
}

function dirtyTabCount(tabs: WorkbenchTab[]): number {
  return tabs.filter((tab) => tab.dirty).length;
}

interface UnsavedCloseConfirmModalProps {
  dirtyCount: number;
  kind: PendingCloseRequest["kind"];
  onCancel: () => void;
  onConfirm: () => void;
}

function UnsavedCloseConfirmModal(
  { dirtyCount, kind, onCancel, onConfirm }: UnsavedCloseConfirmModalProps,
) {
  const closingPane = kind === "pane";
  return (
    <div
      className={closeConfirmBackdropClassName}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <section
        className={closeConfirmModalClassName}
        role="dialog"
        aria-modal="true"
        aria-labelledby="unsaved-close-title"
      >
        <header className={closeConfirmHeadClassName}>
          <div>
            <span>{closingPane ? "Pane" : "Tab"}</span>
            <h2 id="unsaved-close-title">Unsaved changes</h2>
          </div>
          <Button
            onClick={onCancel}
            title="Close"
            aria-label="Close unsaved changes dialog"
            className={closeConfirmIconButtonClassName}
          >
            <X size={16} />
          </Button>
        </header>
        <div className={closeConfirmBodyClassName}>
          <p>
            {closingPane
              ? dirtyCount === 1
                ? "This pane contains a tab with unsaved changes."
                : `This pane contains ${dirtyCount} tabs with unsaved changes.`
              : "This tab has unsaved changes."}
          </p>
          <p>Close it anyway?</p>
          <div className={closeConfirmActionsClassName}>
            <Button onClick={onCancel}>Cancel</Button>
            <Button
              className={closeConfirmDangerButtonClassName}
              onClick={onConfirm}
            >
              Close without saving
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}
