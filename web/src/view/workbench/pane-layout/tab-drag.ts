import type { DragEvent } from "react";

export const workbenchTabDragType = "application/x-wgo-workbench-tab";

export type TabSplitDropSide = "left" | "right" | "top" | "bottom";

export interface WorkbenchTabDragData {
  nodeId: string;
  paneId: string;
  tabId: string;
}

export interface WorkbenchTabDropTarget {
  tabId?: string;
  position: "before" | "after" | "end";
}

export function hasWorkbenchTabDragData(event: DragEvent): boolean {
  return Array.from(event.dataTransfer.types).includes(workbenchTabDragType);
}

export function readWorkbenchTabDragData(
  event: DragEvent,
): WorkbenchTabDragData | undefined {
  if (!hasWorkbenchTabDragData(event)) return undefined;
  try {
    const data = JSON.parse(event.dataTransfer.getData(workbenchTabDragType));
    if (
      typeof data?.nodeId !== "string" ||
      typeof data?.paneId !== "string" ||
      typeof data?.tabId !== "string"
    ) {
      return undefined;
    }
    return { nodeId: data.nodeId, paneId: data.paneId, tabId: data.tabId };
  } catch {
    return undefined;
  }
}
