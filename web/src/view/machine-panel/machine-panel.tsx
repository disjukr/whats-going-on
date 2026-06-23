import React from "react";
import { ChevronDown, WifiOff } from "lucide-react";
import type { AvailableShellInfo } from "../../protocol/rpc.ts";
import type { Machine } from "../../state/machines.ts";
import type { ConnectionState } from "../../state/types.ts";
import type { WorkbenchTool } from "../../state/workbench.ts";
import { className } from "../class-name.ts";
import { ToolMenu } from "./tool-menu.tsx";

interface MachinePanelProps {
  activeTool: WorkbenchTool;
  connection: ConnectionState;
  machine?: Machine;
  machinePanelCollapsed: boolean;
  machinePanelMaxWidth: number;
  machinePanelMinWidth: number;
  machinePanelWidth: number;
  onOpenMachineMenu: (
    event: React.MouseEvent<HTMLButtonElement>,
    machine: Machine,
  ) => void;
  onOpenTerminalShell: (shell?: AvailableShellInfo) => void;
  onResizeKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => void;
  onResizePointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
  onSelectTool: (tool: WorkbenchTool) => void;
  terminalShells: AvailableShellInfo[];
}

const machinePanelClassName = [
  "machine-panel relative [grid-column:2] [grid-row:2] grid",
  "[grid-template-rows:auto_minmax(0,1fr)] min-w-0 min-h-0 overflow-hidden",
  "border-r border-r-[#d8dde7] rounded-tl-[16px] bg-[#fbfcfe]",
].join(" ");
const machinePanelSummaryClassName =
  "grid h-[48px] min-h-[48px] border-b border-b-[#d8dde7] px-[8px] py-0";
const machineTitleClassName = [
  "flex items-center min-w-0",
  "[&_h1]:flex [&_h1]:items-center [&_h1]:m-0 [&_h1]:min-w-0",
].join(" ");
const machineTitleButtonClassName = [
  "machine-title-button inline-flex appearance-none items-center justify-start gap-[5px]",
  "h-[48px] max-w-full min-h-[48px] overflow-visible",
  "cursor-pointer border-0 rounded-[6px] bg-transparent text-[#20242d]",
  "px-[8px] font-700 leading-none tracking-[0] [font-family:inherit]",
  "hover:bg-[#eef2f7] [&_svg]:flex-[0_0_auto]",
].join(" ");
const machineTitleTextClassName = [
  "machine-title-text block flex-[1_1_auto] min-w-0 overflow-hidden",
  "leading-[1.25] text-ellipsis whitespace-nowrap",
].join(" ");
const machineTitleConnectionIndicatorClassName =
  "flex-[0_0_auto] text-[#d92d20] [stroke-width:2.4]";
const machinePanelResizerClassName = [
  "absolute top-0 right-[-4px] bottom-0 z-[8] w-[8px] cursor-col-resize touch-none",
  "after:content-[''] after:absolute after:top-0 after:bottom-0 after:left-[3px]",
  "after:w-[1px] after:bg-transparent",
  "hover:after:w-[2px] hover:after:bg-[#4f8cff]",
  "focus-visible:after:w-[2px] focus-visible:after:bg-[#4f8cff] focus-visible:outline-0",
].join(" ");

export function MachinePanel(
  {
    activeTool,
    connection,
    machine,
    machinePanelCollapsed,
    machinePanelMaxWidth,
    machinePanelMinWidth,
    machinePanelWidth,
    onOpenMachineMenu,
    onOpenTerminalShell,
    onResizeKeyDown,
    onResizePointerDown,
    onSelectTool,
    terminalShells,
  }: MachinePanelProps,
) {
  return (
    <aside
      className={machinePanelClassName}
      aria-label="Machine workspace"
      aria-hidden={machinePanelCollapsed}
    >
      {!machinePanelCollapsed
        ? (
          <>
            <section className={machinePanelSummaryClassName}>
              <div className={machineTitleClassName}>
                <h1>
                  {machine
                    ? (
                      <button
                        type="button"
                        className={className(
                          machineTitleButtonClassName,
                          connection.phase === "checking" && "checking",
                        )}
                        onMouseDown={(event) => event.stopPropagation()}
                        onClick={(event) => onOpenMachineMenu(event, machine)}
                        title="Machine actions"
                        aria-label={`${machine.name} machine actions`}
                      >
                        <span className={machineTitleTextClassName}>
                          {machine.name}
                        </span>
                        {connection.phase === "offline"
                          ? (
                            <WifiOff
                              size={14}
                              className={machineTitleConnectionIndicatorClassName}
                              aria-hidden="true"
                            />
                          )
                          : null}
                        <ChevronDown size={16} />
                      </button>
                    )
                    : "No machine"}
                </h1>
              </div>
            </section>

            <ToolMenu
              activeTool={activeTool}
              terminalShells={terminalShells}
              onOpenTerminalShell={onOpenTerminalShell}
              onSelect={onSelectTool}
            />
            <div
              className={machinePanelResizerClassName}
              role="separator"
              aria-label="Resize machine panel"
              aria-orientation="vertical"
              aria-valuemin={machinePanelMinWidth}
              aria-valuemax={machinePanelMaxWidth}
              aria-valuenow={machinePanelWidth}
              tabIndex={0}
              onPointerDown={onResizePointerDown}
              onKeyDown={onResizeKeyDown}
            />
          </>
        )
        : null}
    </aside>
  );
}
