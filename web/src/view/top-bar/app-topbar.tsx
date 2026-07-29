import {
  Activity,
  AppWindow,
  Bot,
  CheckCircle2,
  CircuitBoard,
  Cpu,
  Folder,
  HardDrive,
  Laptop,
  Monitor,
  MoreHorizontal,
  PanelLeftClose,
  PanelLeftOpen,
  PcCase,
  Plus,
  Radio,
  RefreshCw,
  Router,
  Server,
  Settings,
  Terminal,
  Trash2,
  Unlink,
} from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useRef,
  useState,
} from "react";
import type { AvailableShellInfo } from "../../protocol/generated/rpc.ts";
import { type Machine, type MachineIconName } from "../../state/machines.ts";
import type { DaemonInfoState } from "../../state/rpc-session.ts";
import type { ConnectionState } from "../../state/types.ts";
import type {
  WorkbenchAgentTabConfig,
  WorkbenchFilesTabConfig,
  WorkbenchPane,
  WorkbenchTab,
  WorkbenchTerminalTabConfig,
  WorkbenchTool,
} from "../../state/workbench.ts";
import { className } from "../class-name.ts";
import {
  clampFloatingMenuPosition,
  FloatingMenu,
  FloatingMenuItem,
  type FloatingMenuPosition,
  useFloatingMenuDismiss,
} from "../ui/floating-menu.tsx";

const projectLogoUrl = new URL("../../assets/rieul.svg", import.meta.url).href;

const globalTopbarClassName = [
  "app-rail relative [grid-column:1] [grid-row:1] flex flex-col",
  "h-full min-h-0 min-w-0 overflow-hidden",
  "box-border bg-transparent",
  "gap-[12px] py-[14px] pl-[12px] pr-0 leading-[1.45] text-rieul-text-2",
  "max-[680px]:[grid-column:1] max-[680px]:[grid-row:1]",
  "max-[680px]:h-full max-[680px]:w-full max-[680px]:overflow-x-hidden max-[680px]:overflow-y-auto",
  "max-[680px]:items-stretch max-[680px]:justify-start max-[680px]:gap-[14px]",
  "max-[680px]:px-[14px] max-[680px]:py-[14px]",
].join(" ");
const globalTopbarLeftClassName = [
  "app-rail-section grid min-w-0",
].join(" ");
const globalTopbarCenterClassName = "min-w-0 pointer-events-none hidden";
const globalTopbarRightClassName =
  "app-rail-section grid min-w-0 content-start gap-[12px]";
const topbarBrandClassName = [
  "inline-flex h-[34px] min-w-0 items-center gap-[8px] px-[2px]",
  "text-[13px] font-780 text-rieul-text",
  "[&_img]:h-[23px] [&_img]:w-[23px]",
].join(" ");
const topbarLeftDividerClassName = "hidden";
const globalIconButtonClassName = [
  "inline-flex appearance-none items-center justify-center w-[28px] min-w-[28px] h-[28px] min-h-[28px]",
  "box-border cursor-pointer border border-transparent rounded-rieul-md bg-transparent text-rieul-text-3 p-0 leading-none",
  "[font-family:inherit]",
  "opacity-72 hover:opacity-100 hover:border-white/44 hover:bg-white/36 hover:text-rieul-text-2",
  "active:bg-rieul-active",
  "max-[680px]:hidden",
].join(" ");
const toolSwitcherClassName = [
  "rail-tool-switcher grid w-full min-w-0 content-start gap-[2px] rounded-[14px]",
  "border border-white/34 bg-[rgba(248,248,248,0.28)] p-[6px] backdrop-blur-2xl",
  "shadow-[inset_0_1px_0_rgba(255,255,255,0.68),0_8px_18px_rgba(18,25,38,0.045)]",
  "max-[680px]:gap-[6px]",
].join(" ");
const toolSplitClassName = [
  "inline-flex h-[31px] w-full min-w-0 items-center rounded-[10px]",
  "border border-transparent text-[12px] font-650 text-rieul-text-3/68",
  "rieul-transition",
  "hover:bg-white/20 hover:text-rieul-text-2",
  "[&.active]:border-white/74 [&.active]:bg-[rgba(255,255,255,0.72)] [&.active]:text-rieul-text",
  "[&.active]:shadow-[0_6px_14px_rgba(20,30,46,0.095),inset_0_1px_0_rgba(255,255,255,0.86),inset_0_-1px_0_rgba(32,48,70,0.035)]",
  "max-[680px]:h-[42px] max-[680px]:justify-start",
].join(" ");
const toolNameButtonClassName = [
  "inline-flex h-[29px] min-w-0 flex-1 appearance-none items-center justify-start gap-[7px]",
  "rounded-[9px] border-0 bg-transparent px-[8px] py-0 text-inherit [font-family:inherit]",
  "cursor-pointer rieul-transition",
  "hover:bg-white/18 hover:text-rieul-text",
  "[&_svg]:flex-[0_0_auto] [&_svg]:opacity-86 [&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis [&_span]:whitespace-nowrap",
  "max-[680px]:h-full max-[680px]:justify-start max-[680px]:gap-[8px] max-[680px]:px-[9px]",
].join(" ");
const toolMenuButtonClassName = [
  "app-rail-expanded inline-flex h-[29px] w-[28px] min-w-[28px] appearance-none items-center justify-center",
  "rounded-[9px] border-0 bg-transparent p-0 text-inherit [font-family:inherit]",
  "cursor-pointer opacity-72 rieul-transition",
  "hover:bg-white/18 hover:text-rieul-text hover:opacity-100",
  "focus-visible:bg-white/18 focus-visible:text-rieul-text focus-visible:opacity-100 focus-visible:outline-0",
  "max-[680px]:h-full",
].join(" ");
const toolNameLabelClassName = "app-rail-label";
const topbarMenuClassName = [
  "z-[60] w-[244px] gap-[2px] rounded-rieul-lg p-[4px]",
  "rieul-material-floating text-rieul-text",
].join(" ");
const topbarMenuHeaderClassName = [
  "px-[8px] py-[6px] text-[11px] font-740 text-rieul-text-3",
].join(" ");
const topbarMenuMetaClassName =
  "ml-auto min-w-0 max-w-[92px] overflow-hidden text-ellipsis whitespace-nowrap text-[12px] text-rieul-text-3";
const machineMenuClassName = [
  "z-[60] w-[216px] gap-[3px] rounded-rieul-lg p-[5px]",
  "rieul-material-floating text-rieul-text",
].join(" ");
const mobileMachineMenuClassName = [
  machineMenuClassName,
  "max-[680px]:w-[252px]",
].join(" ");
const machineMenuHeaderClassName =
  "px-[8px] pb-[5px] pt-[6px] text-[11px] font-760 tracking-[0] text-rieul-text-3";
const machineMenuDividerClassName =
  "mx-[5px] my-[5px] h-px bg-[rgba(18,25,38,0.12)] shadow-[0_1px_0_rgba(255,255,255,0.64)]";
const machineMenuDangerItemClassName = [
  "text-rieul-danger hover:bg-rieul-danger-soft hover:text-rieul-danger",
  "[&_svg]:text-rieul-danger",
].join(" ");
const statusDotClassName = "h-[6px] w-[6px] rounded-full";
const railBrandActionsClassName = "inline-flex items-center gap-[4px]";
const railStatusButtonClassName = [
  "inline-flex h-[28px] min-w-[22px] appearance-none items-center justify-center",
  "rounded-rieul-md border border-transparent bg-transparent p-0 [font-family:inherit]",
  "cursor-pointer opacity-82 rieul-transition",
  "hover:border-white/44 hover:bg-white/36 hover:opacity-100",
  "disabled:cursor-default disabled:opacity-45",
].join(" ");
const connectionPopoverClassName = [
  "z-[60] w-[320px] gap-0 rounded-rieul-xl p-0",
  "rieul-material-floating text-rieul-text",
].join(" ");
const popoverHeaderClassName = [
  "grid gap-[8px] border-b border-b-black/6 px-[12px] py-[11px]",
].join(" ");
const popoverTitleClassName =
  "flex min-w-0 items-center gap-[8px] text-[13px] font-760";
const popoverMetaClassName =
  "min-w-0 overflow-hidden text-ellipsis whitespace-nowrap text-[12px] font-650 text-rieul-text-3";
const popoverRowClassName = [
  "grid min-h-[32px] grid-cols-[92px_minmax(0,1fr)] items-center gap-[10px]",
  "border-b border-b-black/5 px-[12px] text-[12px] last:border-b-0",
].join(" ");
const popoverLabelClassName = "font-650 text-rieul-text-3";
const popoverValueClassName =
  "min-w-0 overflow-hidden text-right text-ellipsis whitespace-nowrap font-720 text-rieul-text-2";
const railBrandRowClassName =
  "flex min-w-0 items-center justify-between gap-[6px]";
const railMachineNameClassName = [
  "app-rail-expanded min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap",
  "text-[13px] font-720 text-rieul-text-2",
].join(" ");
const railMachineListClassName = [
  "app-rail-section grid min-w-0 grid-cols-[repeat(auto-fill,38px)] justify-start gap-[7px]",
  "rounded-[15px] border border-white/28 bg-[rgba(248,248,248,0.28)] p-[7px] backdrop-blur-xl",
  "max-[680px]:grid-cols-[repeat(auto-fill,48px)] max-[680px]:gap-[8px] max-[680px]:p-[8px]",
].join(" ");
const railMachineButtonClassName = [
  "group relative inline-flex h-[38px] w-[38px] min-w-[38px] appearance-none items-center justify-center",
  "rounded-[12px] border border-transparent bg-white/18 px-0",
  "text-[12px] font-650 text-rieul-text-3 [font-family:inherit]",
  "cursor-pointer rieul-transition hover:border-white/48 hover:bg-white/34 hover:text-rieul-text-2",
  "[&.active]:border-white/74 [&.active]:bg-white/70 [&.active]:text-rieul-text",
  "[&.active]:shadow-[inset_0_1px_0_rgba(255,255,255,0.86),0_6px_16px_rgba(18,25,38,0.08)]",
  "[&_svg]:[stroke-width:2]",
  "max-[680px]:h-[48px] max-[680px]:w-[48px] max-[680px]:min-w-[48px]",
].join(" ");
const railAddMachineButtonClassName = [
  "inline-flex h-[38px] w-[38px] min-w-[38px] appearance-none items-center justify-center",
  "rounded-[12px] border border-dashed border-white/34 bg-white/10 px-0",
  "text-rieul-text-3 [font-family:inherit]",
  "cursor-pointer rieul-transition hover:border-white/48 hover:bg-white/34 hover:text-rieul-text",
  "max-[680px]:mt-0 max-[680px]:h-[44px] max-[680px]:w-[44px] max-[680px]:min-w-[44px]",
  "max-[680px]:rounded-[14px] max-[680px]:px-0 max-[680px]:text-rieul-text-2",
  "max-[680px]:[&_span]:hidden",
].join(" ");
const mobileMachineButtonClassName = [
  "hidden h-[44px] w-[44px] min-w-[44px] appearance-none items-center justify-center",
  "relative rounded-[14px] border border-transparent bg-white/18 p-0 text-rieul-text-2",
  "[font-family:inherit] cursor-pointer rieul-transition",
  "hover:border-white/48 hover:bg-white/34 hover:text-rieul-text",
].join(" ");
const railMachineIconDotClassName = [
  "absolute bottom-[5px] right-[5px] h-[5px] w-[5px] rounded-full",
  "bg-rieul-text-3/42 group-[.active]:bg-rieul-accent",
].join(" ");
const iconPickerMenuClassName = [
  "grid grid-cols-[repeat(4,minmax(0,1fr))] gap-[5px] px-[5px] pb-[2px]",
].join(" ");
const iconPickerItemClassName = [
  "!h-[38px] !min-h-[38px] !justify-center !rounded-rieul-md !px-0",
].join(" ");

const topbarTools: {
  icon: typeof Folder;
  label: string;
  tool: WorkbenchTool;
}[] = [
  { icon: Radio, label: "Daemon", tool: "daemon" },
  { icon: Bot, label: "Agent", tool: "agent" },
  { icon: Folder, label: "Files", tool: "files" },
  { icon: Terminal, label: "Terminal", tool: "terminal" },
  { icon: AppWindow, label: "Windows", tool: "windows" },
  { icon: Activity, label: "Processes", tool: "processes" },
];

const machineIconOptions: {
  icon: typeof Monitor;
  label: string;
  name: MachineIconName;
}[] = [
  { icon: Monitor, label: "Monitor", name: "monitor" },
  { icon: Laptop, label: "Laptop", name: "laptop" },
  { icon: Server, label: "Server", name: "server" },
  { icon: Cpu, label: "CPU", name: "cpu" },
  { icon: HardDrive, label: "Drive", name: "hard-drive" },
  { icon: Router, label: "Router", name: "router" },
  { icon: CircuitBoard, label: "Board", name: "circuit-board" },
  { icon: PcCase, label: "PC case", name: "pc-case" },
];

interface AppTopbarProps {
  activeTool: WorkbenchTool;
  agentRail: ReactNode;
  connection?: ConnectionState;
  daemonInfo: DaemonInfoState;
  machine?: Machine;
  machinePanelCollapsed: boolean;
  machines: Machine[];
  panes: WorkbenchPane[];
  selectedMachineId?: string;
  terminalShells: AvailableShellInfo[];
  onAddMachine: () => void;
  onOpenAgentTab: (config?: WorkbenchAgentTabConfig) => void;
  onOpenDaemonTab: () => void;
  onOpenFilesTab: (config?: WorkbenchFilesTabConfig) => void;
  onOpenProcessesTab: () => void;
  onOpenTerminalTab: (config?: WorkbenchTerminalTabConfig) => void;
  onOpenWindowsTab: () => void;
  onSelectTab: (paneId: string, tabId: string) => void;
  onConfigureMachine: (machineId: string) => void;
  onDeleteMachine: (machineId: string) => void;
  onReconnectMachine: (machineId: string) => void;
  onSelectMachine: (machineId: string) => void;
  onToggleMachinePanel: () => void;
  onUnpairMachine: (machineId: string) => void;
  onUpdateMachineIcon: (machineId: string, icon: MachineIconName) => void;
}

interface ToolMenuState {
  position: FloatingMenuPosition;
  tool: WorkbenchTool;
}

interface MachineMenuState {
  kind: "connection" | "actions";
  machineId?: string;
  position: FloatingMenuPosition;
  source?: "desktop" | "mobile";
}

export function AppTopbar(
  {
    activeTool,
    agentRail,
    connection,
    daemonInfo,
    machine,
    machinePanelCollapsed,
    machines,
    panes,
    selectedMachineId,
    terminalShells,
    onAddMachine,
    onOpenAgentTab,
    onOpenDaemonTab,
    onOpenFilesTab,
    onOpenProcessesTab,
    onOpenTerminalTab,
    onOpenWindowsTab,
    onSelectTab,
    onConfigureMachine,
    onDeleteMachine,
    onReconnectMachine,
    onSelectMachine,
    onToggleMachinePanel,
    onUnpairMachine,
    onUpdateMachineIcon,
  }: AppTopbarProps,
) {
  const toolMenuRef = useRef<HTMLDivElement | null>(null);
  const machineMenuRef = useRef<HTMLDivElement | null>(null);
  const [toolMenu, setToolMenu] = useState<ToolMenuState | undefined>();
  const [machineMenu, setMachineMenu] = useState<
    MachineMenuState | undefined
  >();
  const defaultTerminalShell =
    terminalShells.find((shell) => shell.isDefault) ?? terminalShells[0];
  const openToolTab: Record<WorkbenchTool, () => void> = {
    agent: onOpenAgentTab,
    daemon: onOpenDaemonTab,
    files: onOpenFilesTab,
    processes: onOpenProcessesTab,
    terminal: openDefaultTerminal,
    windows: onOpenWindowsTab,
  };
  const closeToolMenu = useCallback(() => {
    setToolMenu(undefined);
  }, []);
  useFloatingMenuDismiss(
    toolMenu !== undefined,
    toolMenuRef,
    closeToolMenu,
    { closeOnScroll: true },
  );
  const closeMachineMenu = useCallback(() => {
    setMachineMenu(undefined);
  }, []);
  useFloatingMenuDismiss(
    machineMenu !== undefined,
    machineMenuRef,
    closeMachineMenu,
    { closeOnScroll: true },
  );

  const tabTargets = tabTargetsForPanes(panes);

  function openToolMenu(
    tool: WorkbenchTool,
    target: HTMLElement,
  ) {
    const rect = target.getBoundingClientRect();
    setMachineMenu(undefined);
    setToolMenu({
      tool,
      position: clampFloatingMenuPosition(
        rect.right + 8,
        rect.top,
        { itemCount: 4, width: 244 },
      ),
    });
  }

  function openMachineMenu(
    kind: "connection" | "actions",
    target: HTMLElement,
    machineId = selectedMachineId,
    source: MachineMenuState["source"] = "desktop",
  ) {
    const rect = target.getBoundingClientRect();
    const width = kind === "connection" ? 320 : source === "mobile" ? 252 : 216;
    setToolMenu(undefined);
    setMachineMenu({
      kind,
      machineId,
      position: clampFloatingMenuPosition(
        rect.right + 8,
        rect.top,
        {
          itemCount: kind === "connection" ? 7 : source === "mobile" ? 18 : 12,
          width,
        },
      ),
      source,
    });
  }

  function selectLatestToolTab(tool: WorkbenchTool) {
    const latest = latestTabTarget(tabTargets, tool);
    if (!latest) {
      openToolTab[tool]();
      revealWorkbenchOnMobile();
      return;
    }
    onSelectTab(latest.paneId, latest.tab.id);
    revealWorkbenchOnMobile();
  }

  function runMachineAction(
    machineId: string | undefined,
    action: (
      machineId: string,
    ) => void,
  ) {
    if (!machineId) return;
    closeMachineMenu();
    action(machineId);
  }

  function terminalTabConfigForShell(
    shell: AvailableShellInfo,
  ): WorkbenchTerminalTabConfig {
    return {
      launch: {
        args: shell.args,
        command: shell.command,
      },
      title: shell.name,
    };
  }

  function openDefaultTerminal() {
    const shell = defaultTerminalShell;
    closeToolMenu();
    if (!shell) {
      onOpenTerminalTab();
      revealWorkbenchOnMobile();
      return;
    }
    onOpenTerminalTab(terminalTabConfigForShell(shell));
    revealWorkbenchOnMobile();
  }

  function openTerminalForShell(shell: AvailableShellInfo) {
    closeToolMenu();
    onOpenTerminalTab(terminalTabConfigForShell(shell));
    revealWorkbenchOnMobile();
  }

  function openConnectionMenu(target: HTMLElement) {
    if (machineMenu?.kind === "connection") {
      closeMachineMenu();
      return;
    }
    openMachineMenu("connection", target);
  }

  function openActionsMenu(target: HTMLElement, machineId = selectedMachineId) {
    openMachineMenu("actions", target, machineId);
  }

  function openMobileMachineMenu(target: HTMLElement) {
    openMachineMenu("actions", target, selectedMachineId, "mobile");
  }

  function openToolTabFromMenu(tool: WorkbenchTool) {
    closeToolMenu();
    openToolTab[tool]();
    revealWorkbenchOnMobile();
  }

  function openFilesOption(filesView: "roots" | "home" | "trash") {
    closeToolMenu();
    onOpenFilesTab({ filesView });
    revealWorkbenchOnMobile();
  }

  function revealWorkbenchOnMobile() {
    if (!isMobileRailViewport()) return;
    const shell = document.querySelector<HTMLElement>(".app-shell");
    shell?.scrollTo({ left: shell.clientWidth, behavior: "smooth" });
  }

  function isMobileRailViewport() {
    return globalThis.matchMedia("(max-width: 680px)").matches;
  }

  function toolMenuTitle(tool: WorkbenchTool) {
    return topbarTools.find((item) => item.tool === tool)?.label ?? tool;
  }

  function onToolNameKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    tool: WorkbenchTool,
  ) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      selectLatestToolTab(tool);
    }
  }

  function menuMachine() {
    return machines.find((item) => item.id === machineMenu?.machineId) ??
      machine;
  }

  function deviceName(targetMachine = machine) {
    return targetMachine?.name ?? "No machine";
  }

  function isPaired(targetMachine = machine) {
    return Boolean(targetMachine?.clientId && targetMachine?.clientSecret);
  }

  function connectionIcon() {
    return (
      <span
        className={className(
          statusDotClassName,
          connection?.phase === "reachable"
            ? "bg-rieul-success"
            : connection?.phase === "idle"
            ? "bg-rieul-warning"
            : "bg-rieul-danger",
        )}
      />
    );
  }

  function connectionPopoverPosition() {
    return machineMenu?.kind === "connection"
      ? machineMenu.position
      : undefined;
  }

  function actionsPopoverPosition() {
    return machineMenu?.kind === "actions" ? machineMenu.position : undefined;
  }

  function openMachineLauncherContextMenu(
    event: MouseEvent<HTMLButtonElement>,
    machineId: string,
  ) {
    event.preventDefault();
    event.stopPropagation();
    onSelectMachine(machineId);
    openActionsMenu(event.currentTarget, machineId);
  }

  function onMachineLauncherKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    machineId: string,
  ) {
    if (
      event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")
    ) {
      return;
    }
    event.preventDefault();
    onSelectMachine(machineId);
    openActionsMenu(event.currentTarget, machineId);
  }

  function selectMachineIcon(machineId: string, icon: MachineIconName) {
    onUpdateMachineIcon(machineId, icon);
    closeMachineMenu();
  }

  function selectMachineFromMenu(machineId: string) {
    onSelectMachine(machineId);
    closeMachineMenu();
  }

  function addMachineFromMenu() {
    closeMachineMenu();
    onAddMachine();
  }

  function renderToolMenu() {
    if (!toolMenu) return null;
    return (
      <FloatingMenu
        className={topbarMenuClassName}
        menuRef={toolMenuRef}
        position={toolMenu.position}
      >
        <div className={topbarMenuHeaderClassName}>
          New {toolMenuTitle(toolMenu.tool)}
        </div>
        {toolMenu.tool === "files"
          ? (
            <>
              <FloatingMenuItem onClick={() => openFilesOption("roots")}>
                <Folder size={14} />
                Root
              </FloatingMenuItem>
              <FloatingMenuItem onClick={() => openFilesOption("home")}>
                <Folder size={14} />
                Home
              </FloatingMenuItem>
              <FloatingMenuItem onClick={() => openFilesOption("trash")}>
                <Trash2 size={14} />
                Trash
              </FloatingMenuItem>
            </>
          )
          : toolMenu.tool === "terminal"
          ? (
            terminalShells.length === 0
              ? (
                <FloatingMenuItem
                  disabled
                >
                  <Terminal size={14} />
                  No shells available
                </FloatingMenuItem>
              )
              : terminalShells.map((shell) => (
                <FloatingMenuItem
                  key={shell.shellId}
                  onClick={() => openTerminalForShell(shell)}
                >
                  <Terminal size={14} />
                  <span>{shell.name}</span>
                  {shell.isDefault
                    ? <span className={topbarMenuMetaClassName}>Default</span>
                    : null}
                </FloatingMenuItem>
              ))
          )
          : (
            <FloatingMenuItem
              onClick={() => openToolTabFromMenu(toolMenu.tool)}
            >
              <ToolIcon tool={toolMenu.tool} size={14} />
              New {toolMenuTitle(toolMenu.tool)} Tab
            </FloatingMenuItem>
          )}
      </FloatingMenu>
    );
  }

  function renderMachineActionsMenu() {
    const position = actionsPopoverPosition();
    if (!position) return null;
    const targetMachine = menuMachine();
    const machineId = targetMachine?.id;
    const mobileMenu = machineMenu?.source === "mobile";
    const otherMachines = machines.filter((item) => item.id !== machineId);
    return (
      <FloatingMenu
        className={mobileMenu
          ? mobileMachineMenuClassName
          : machineMenuClassName}
        menuRef={machineMenuRef}
        position={position}
      >
        <div className={machineMenuHeaderClassName}>
          {deviceName(targetMachine)}
        </div>
        <FloatingMenuItem
          disabled={!machineId}
          onClick={() => runMachineAction(machineId, onReconnectMachine)}
        >
          <RefreshCw size={15} />
          Reconnect
        </FloatingMenuItem>
        <FloatingMenuItem
          disabled={!machineId}
          onClick={() => runMachineAction(machineId, onConfigureMachine)}
        >
          <Settings size={15} />
          Configure
        </FloatingMenuItem>
        <div className={machineMenuDividerClassName} aria-hidden="true" />
        <FloatingMenuItem
          disabled={!machineId || !isPaired(targetMachine)}
          onClick={() => runMachineAction(machineId, onUnpairMachine)}
        >
          <Unlink size={15} />
          Unpair
        </FloatingMenuItem>
        <div className={machineMenuDividerClassName} aria-hidden="true" />
        <div className={machineMenuHeaderClassName}>
          Icon
        </div>
        <div className={iconPickerMenuClassName} role="group">
          {machineIconOptions.map(({ icon: Icon, label, name }) => (
            <FloatingMenuItem
              key={name}
              className={iconPickerItemClassName}
              disabled={!machineId}
              onClick={() => machineId && selectMachineIcon(machineId, name)}
              title={label}
              aria-label={label}
            >
              <Icon size={17} />
            </FloatingMenuItem>
          ))}
        </div>
        <div className={machineMenuDividerClassName} aria-hidden="true" />
        {mobileMenu
          ? (
            <>
              <div className={machineMenuHeaderClassName}>
                Other Devices
              </div>
              {otherMachines.length === 0
                ? (
                  <FloatingMenuItem disabled>
                    <Monitor size={15} />
                    No Other Devices
                  </FloatingMenuItem>
                )
                : otherMachines.map((item) => (
                  <FloatingMenuItem
                    key={item.id}
                    onClick={() => selectMachineFromMenu(item.id)}
                  >
                    <MachineIcon name={item.icon} size={15} />
                    <span>{item.name}</span>
                  </FloatingMenuItem>
                ))}
              <FloatingMenuItem onClick={addMachineFromMenu}>
                <Plus size={15} />
                Add Machine
              </FloatingMenuItem>
              <div
                className={machineMenuDividerClassName}
                aria-hidden="true"
              />
            </>
          )
          : null}
        <FloatingMenuItem
          tone="danger"
          className={machineMenuDangerItemClassName}
          disabled={!machineId}
          onClick={() => runMachineAction(machineId, onDeleteMachine)}
        >
          <Trash2 size={15} />
          Delete
        </FloatingMenuItem>
      </FloatingMenu>
    );
  }

  const connectionPosition = connectionPopoverPosition();

  function renderConnectionPopover() {
    if (!connectionPosition) return null;
    return (
      <ConnectionPopover
        connection={connection}
        daemonInfo={daemonInfo}
        machine={machine}
        menuRef={machineMenuRef}
        position={connectionPosition}
      />
    );
  }

  function canOpenMachineMenu() {
    return Boolean(machine);
  }

  function renderConnectionStatusButton() {
    return (
      <button
        type="button"
        className={railStatusButtonClassName}
        aria-haspopup="dialog"
        aria-expanded={machineMenu?.kind === "connection"}
        aria-label={`Connection: ${connectionDetail(connection, daemonInfo)}`}
        disabled={!canOpenMachineMenu()}
        onClick={(event) => openConnectionMenu(event.currentTarget)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown") return;
          event.preventDefault();
          openConnectionMenu(event.currentTarget);
        }}
        title="Connection details"
      >
        {connectionIcon()}
      </button>
    );
  }

  function renderToolLaunchers() {
    return topbarTools.map(({ icon: Icon, label, tool }) => (
      <div
        key={tool}
        className={className(
          "group",
          toolSplitClassName,
          activeTool === tool && "active",
        )}
      >
        <button
          type="button"
          className={toolNameButtonClassName}
          aria-haspopup="menu"
          aria-label={label}
          onClick={() => selectLatestToolTab(tool)}
          onContextMenu={(event) => {
            event.preventDefault();
            openToolMenu(tool, event.currentTarget);
          }}
          onKeyDown={(event) => onToolNameKeyDown(event, tool)}
          title={label}
        >
          <Icon size={13} />
          <span className={toolNameLabelClassName}>{label}</span>
        </button>
        {hasToolMenu(tool)
          ? (
            <button
              type="button"
              className={toolMenuButtonClassName}
              aria-label={`${label} menu`}
              aria-haspopup="menu"
              aria-expanded={toolMenu?.tool === tool}
              onClick={(event) => {
                event.stopPropagation();
                openToolMenu(tool, event.currentTarget);
              }}
              title={`${label} menu`}
            >
              <MoreHorizontal size={14} />
            </button>
          )
          : null}
      </div>
    ));
  }

  function renderMachineLaunchers() {
    return (
      <nav className={railMachineListClassName} aria-label="Machines">
        {machines.map((item) => (
          <button
            key={item.id}
            type="button"
            className={className(
              railMachineButtonClassName,
              item.id === selectedMachineId && "active",
            )}
            onClick={() => onSelectMachine(item.id)}
            onContextMenu={(event) =>
              openMachineLauncherContextMenu(event, item.id)}
            onKeyDown={(event) => onMachineLauncherKeyDown(event, item.id)}
            title={item.name}
            aria-label={item.name}
            aria-haspopup="menu"
            aria-current={item.id === selectedMachineId ? "true" : undefined}
          >
            <MachineIcon name={item.icon} size={18} />
            <span className={railMachineIconDotClassName} />
          </button>
        ))}
        <button
          type="button"
          className={railAddMachineButtonClassName}
          onClick={onAddMachine}
          title="Add machine"
          aria-label="Add machine"
        >
          <Plus size={17} strokeWidth={2.2} />
        </button>
      </nav>
    );
  }

  function renderMobileMachineButton() {
    return (
      <button
        type="button"
        className={mobileMachineButtonClassName}
        aria-haspopup="menu"
        aria-expanded={machineMenu?.kind === "actions" &&
          machineMenu.source === "mobile"}
        aria-label="Device management"
        onClick={(event) => openMobileMachineMenu(event.currentTarget)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown") return;
          event.preventDefault();
          openMobileMachineMenu(event.currentTarget);
        }}
        title="Device management"
      >
        <MachineIcon name={machine?.icon} size={18} />
        <span className={railMachineIconDotClassName} />
      </button>
    );
  }

  return (
    <>
      <header className={globalTopbarClassName}>
        <div className={globalTopbarLeftClassName}>
          <div className={railBrandRowClassName}>
            <div
              className={className(topbarBrandClassName, "app-rail-expanded")}
              aria-label="Rieul"
            >
              <img src={projectLogoUrl} alt="" aria-hidden="true" />
            </div>
            <div className={railMachineNameClassName}>
              {deviceName()}
            </div>
            <div className={railBrandActionsClassName}>
              {renderConnectionStatusButton()}
              <button
                type="button"
                className={globalIconButtonClassName}
                onClick={onToggleMachinePanel}
                title={machinePanelCollapsed
                  ? "Expand sidebar"
                  : "Collapse sidebar"}
                aria-label={machinePanelCollapsed
                  ? "Expand sidebar"
                  : "Collapse sidebar"}
                aria-pressed={machinePanelCollapsed}
              >
                {machinePanelCollapsed
                  ? <PanelLeftOpen size={12} />
                  : <PanelLeftClose size={12} />}
              </button>
            </div>
          </div>
        </div>
        <div className={topbarLeftDividerClassName} aria-hidden="true" />
        {renderMachineLaunchers()}
        <div className={topbarLeftDividerClassName} aria-hidden="true" />
        {renderMobileMachineButton()}
        <div className={globalTopbarCenterClassName} aria-hidden="true" />
        <div className={globalTopbarRightClassName}>
          <nav className={toolSwitcherClassName} aria-label="Workbench tools">
            {renderToolLaunchers()}
          </nav>
        </div>
        {agentRail}
      </header>
      {renderToolMenu()}
      {renderConnectionPopover()}
      {renderMachineActionsMenu()}
    </>
  );
}

function ConnectionPopover(
  { connection, daemonInfo, machine, menuRef, position }: {
    connection?: ConnectionState;
    daemonInfo: DaemonInfoState;
    machine?: Machine;
    menuRef: RefObject<HTMLDivElement | null>;
    position?: FloatingMenuPosition;
  },
) {
  const daemon = daemonInfo.phase === "ready"
    ? daemonInfo.daemonInfo
    : undefined;
  return (
    <FloatingMenu
      className={connectionPopoverClassName}
      menuRef={menuRef}
      position={position}
      role="dialog"
    >
      <div className={popoverHeaderClassName}>
        <div className={popoverTitleClassName}>
          {connection?.phase === "reachable"
            ? <CheckCircle2 size={14} className="text-rieul-success" />
            : <Radio size={14} className="text-rieul-text-3" />}
          <span>{machine?.name ?? "No machine"}</span>
        </div>
        <div className={popoverMetaClassName}>
          {connectionDetail(connection, daemonInfo)}
        </div>
      </div>
      <ConnectionRow
        label="Transport"
        value={connection?.phase === "reachable" ? "WebTransport" : "-"}
      />
      <ConnectionRow
        label="Endpoint"
        value={machine?.baseUrl.replace(/^https?:\/\//, "") ?? "-"}
      />
      <ConnectionRow label="OS" value={daemon?.os ?? "-"} />
      <ConnectionRow label="Version" value={daemon?.version ?? "-"} />
      <ConnectionRow
        label="Instance"
        value={daemon?.instanceId ?? daemonInfo.phase}
      />
      <ConnectionRow
        label="RPC"
        value={daemon ? `${daemon.supportedProcIds.length} procedures` : "-"}
      />
    </FloatingMenu>
  );
}

function ConnectionRow({ label, value }: { label: string; value: string }) {
  return (
    <div className={popoverRowClassName}>
      <span className={popoverLabelClassName}>{label}</span>
      <span className={popoverValueClassName}>{value}</span>
    </div>
  );
}

function connectionDetail(
  connection: ConnectionState | undefined,
  daemonInfo: DaemonInfoState,
): string {
  if (connection?.phase === "reachable") {
    const rtt = connection.rttMs === undefined
      ? "live"
      : `${connection.rttMs} ms`;
    return daemonInfo.phase === "ready"
      ? `Daemon ready · ${rtt}`
      : `Transport ready · daemon ${daemonInfo.phase}`;
  }
  if (connection?.phase === "idle") return "Checking daemon availability";
  return "Daemon unavailable";
}

interface ToolTabTarget {
  paneId: string;
  tab: WorkbenchTab;
}

function tabTargetsForPanes(panes: WorkbenchPane[]): ToolTabTarget[] {
  return panes.flatMap((pane) =>
    pane.tabs.map((tab) => ({
      paneId: pane.id,
      tab,
    }))
  );
}

function latestTabTarget(
  targets: ToolTabTarget[],
  tool: WorkbenchTool,
): ToolTabTarget | undefined {
  for (let index = targets.length - 1; index >= 0; index--) {
    const target = targets[index];
    if (target.tab.tool === tool) return target;
  }
  return undefined;
}

function ToolIcon(
  { size, tool }: { size: number; tool: WorkbenchTool },
) {
  if (tool === "terminal") return <Terminal size={size} />;
  if (tool === "agent") return <Bot size={size} />;
  if (tool === "windows") return <AppWindow size={size} />;
  if (tool === "processes") return <Activity size={size} />;
  if (tool === "files") return <Folder size={size} />;
  return <Radio size={size} />;
}

function hasToolMenu(tool: WorkbenchTool): boolean {
  return tool === "files" || tool === "terminal";
}

function MachineIcon(
  { name = "monitor", size }: { name?: MachineIconName; size: number },
) {
  const option = machineIconOptions.find((item) => item.name === name) ??
    machineIconOptions[0];
  const Icon = option.icon;
  return <Icon size={size} />;
}
