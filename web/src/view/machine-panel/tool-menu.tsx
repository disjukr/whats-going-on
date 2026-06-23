import { type MouseEvent as ReactMouseEvent, useRef, useState } from "react";
import { Activity, ChevronDown, Folder, Info, Terminal } from "lucide-react";
import type { AvailableShellInfo } from "../../protocol/rpc.ts";
import type { WorkbenchTool } from "../../state/workbench.ts";
import { className } from "../class-name.ts";
import {
  FloatingMenu,
  FloatingMenuItem,
  type FloatingMenuPosition,
  floatingMenuPositionFromRect,
  useFloatingMenuDismiss,
} from "../ui/floating-menu.tsx";

const tools: {
  id: WorkbenchTool;
  label: string;
  disabled?: boolean;
  Icon: typeof Folder;
}[] = [
  {
    id: "daemon",
    label: "Daemon",
    Icon: Info,
  },
  {
    id: "files",
    label: "Files",
    Icon: Folder,
  },
  {
    id: "terminal",
    label: "Terminal",
    Icon: Terminal,
  },
  {
    id: "processes",
    label: "Processes",
    Icon: Activity,
    disabled: true,
  },
];

const SHELL_MENU_WIDTH = 260;
const SHELL_MENU_MAX_HEIGHT = 360;
const SHELL_MENU_TRIGGER_GAP = 5;

const toolMenuClassName =
  "grid content-start gap-0 min-h-0 overflow-visible px-[8px] py-[12px]";
const toolItemFrameClassName = "h-[48px] box-border py-[2px]";
const toolItemRowClassName = [
  "relative grid h-[48px] box-border py-[2px]",
  "[grid-template-columns:minmax(0,1fr)_36px]",
].join(" ");
const toolItemClassName = [
  "inline-flex appearance-none items-center justify-start gap-[8px]",
  "w-full h-full min-h-0 border-0 rounded-[6px]",
  "cursor-pointer bg-transparent px-[10px] text-left text-[#475467] [font-family:inherit]",
  "hover:bg-[#eef3fb] hover:text-[#20242d]",
  "[&.active]:bg-[#eef3fb] [&.active]:text-[#20242d]",
  "disabled:opacity-56",
  "[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis",
  "[&_span]:whitespace-nowrap [&_span]:text-[0.8rem] [&_span]:font-700",
].join(" ");
const terminalMainButtonClassName = [
  toolItemClassName,
  "rounded-r-[3px]",
].join(" ");
const terminalDropdownButtonClassName = [
  "inline-flex appearance-none items-center justify-center",
  "h-full min-h-0 w-[36px] min-w-[36px] p-0 rounded-l-[3px] rounded-r-[6px]",
  "cursor-pointer border-0 bg-transparent text-[#475467] [font-family:inherit]",
  "hover:bg-[#eef3fb] hover:text-[#20242d]",
  "[&.active]:bg-[#eef3fb] [&.active]:text-[#20242d]",
].join(" ");
const shellMenuItemClassName = [
  "!grid min-w-0 grid-cols-[minmax(0,1fr)_auto] !gap-[8px]",
  "font-650",
].join(" ");
const shellMenuDefaultItemClassName = "bg-[#eef3fb]";
const shellMenuItemLabelClassName =
  "flex min-w-0 items-center gap-[6px] text-left";
const shellMenuShellNameClassName =
  "block min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-left";
const shellMenuDefaultBadgeClassName = [
  "rounded-[999px] bg-white px-[6px] py-[1px]",
  "text-[10px] font-700 text-[#475467]",
].join(" ");
const shellMenuCommandClassName = [
  "block max-w-[88px] min-w-0 overflow-hidden text-right",
  "text-ellipsis whitespace-nowrap text-[#667085]",
].join(" ");

interface ToolMenuProps {
  activeTool: WorkbenchTool;
  terminalShells: AvailableShellInfo[];
  onOpenTerminalShell: (shell?: AvailableShellInfo) => void;
  onSelect: (tool: WorkbenchTool) => void;
}

export function ToolMenu(
  { activeTool, terminalShells, onOpenTerminalShell, onSelect }: ToolMenuProps,
) {
  const [shellMenuOpen, setShellMenuOpen] = useState(false);
  const [shellMenuPosition, setShellMenuPosition] = useState<
    FloatingMenuPosition | undefined
  >(undefined);
  const shellMenuRef = useRef<HTMLDivElement>(null);
  useFloatingMenuDismiss(shellMenuOpen, shellMenuRef, closeShellMenu, {
    closeOnScroll: true,
  });

  function closeShellMenu() {
    setShellMenuOpen(false);
    setShellMenuPosition(undefined);
  }

  function toggleShellMenu(event: ReactMouseEvent<HTMLButtonElement>) {
    if (shellMenuOpen) {
      closeShellMenu();
      return;
    }
    setShellMenuPosition(
      shellMenuPositionFromRect(
        event.currentTarget.getBoundingClientRect(),
        terminalShells.length,
      ),
    );
    setShellMenuOpen(true);
  }

  function openShellTerminal(shell: AvailableShellInfo) {
    closeShellMenu();
    onOpenTerminalShell(shell);
  }

  function openDefaultTerminal() {
    closeShellMenu();
    onOpenTerminalShell();
  }

  return (
    <nav className={toolMenuClassName} aria-label="Workspace tools">
      {tools.map(({ id, label, disabled, Icon }) =>
        id === "terminal"
          ? (
            <div
              key={id}
              className={toolItemRowClassName}
              ref={shellMenuRef}
            >
              <button
                type="button"
                className={className(
                  terminalMainButtonClassName,
                  activeTool === id && "active",
                )}
                onClick={() => {
                  openDefaultTerminal();
                }}
                disabled={disabled}
                aria-current={activeTool === id ? "page" : undefined}
              >
                <Icon size={17} />
                <span>{label}</span>
              </button>
              <button
                type="button"
                className={className(
                  terminalDropdownButtonClassName,
                  (activeTool === id || shellMenuOpen) && "active",
                )}
                onClick={toggleShellMenu}
                disabled={disabled}
                aria-label="Open terminal shell menu"
                aria-haspopup="menu"
                aria-expanded={shellMenuOpen}
                title="Open terminal shell"
              >
                <ChevronDown size={14} />
              </button>
              {shellMenuOpen
                ? (
                  <FloatingMenu
                    className="z-[80] w-[260px] overflow-x-hidden overflow-y-auto"
                    position={shellMenuPosition}
                  >
                    {terminalShells.map((shell) => (
                      <FloatingMenuItem
                        key={shell.shellId}
                        className={className(
                          shellMenuItemClassName,
                          shell.isDefault && shellMenuDefaultItemClassName,
                        )}
                        onClick={() => openShellTerminal(shell)}
                        title={`${shell.name} (${commandName(shell.command)})`}
                      >
                        <span className={shellMenuItemLabelClassName}>
                          <span className={shellMenuShellNameClassName}>
                            {shell.name}
                          </span>
                          {shell.isDefault
                            ? (
                              <span className={shellMenuDefaultBadgeClassName}>
                                Default
                              </span>
                            )
                            : null}
                        </span>
                        <small className={shellMenuCommandClassName}>
                          {commandName(shell.command)}
                        </small>
                      </FloatingMenuItem>
                    ))}
                  </FloatingMenu>
                )
                : null}
            </div>
          )
          : (
            <div key={id} className={toolItemFrameClassName}>
              <button
                type="button"
                className={className(
                  toolItemClassName,
                  activeTool === id && "active",
                )}
                onClick={() => onSelect(id)}
                disabled={disabled}
                aria-current={activeTool === id ? "page" : undefined}
              >
                <Icon size={17} />
                <span>{label}</span>
              </button>
            </div>
          )
      )}
    </nav>
  );
}

function shellMenuPositionFromRect(
  rect: DOMRect,
  shellCount: number,
): FloatingMenuPosition {
  return floatingMenuPositionFromRect(
    rect,
    {
      itemCount: shellCount,
      maxHeight: SHELL_MENU_MAX_HEIGHT,
      minHeight: 120,
      width: SHELL_MENU_WIDTH,
    },
    SHELL_MENU_TRIGGER_GAP,
  );
}

function commandName(command: string): string {
  const normalized = command.replaceAll("\\", "/");
  const segments = normalized.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? command;
}
