import { bunja } from "bunja";
import { JotaiStoreScope } from "unsaturated/store";
import type { AvailableShellInfo } from "../../protocol/rpc.ts";
import { machineMenuBunja } from "../../state/machine-menu.ts";
import { machineStoreBunja } from "../../state/machine-store.ts";
import type { Machine } from "../../state/machines.ts";
import { rpcSessionBunja } from "../../state/rpc-session.ts";
import { terminalShellsBunja } from "../../state/terminal-shells.ts";
import { workbenchBunja } from "../../state/workbench.ts";
import { layoutBunja } from "../state.tsx";

const machineMenuWidth = 176;

export const machinePanelBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const layout = bunja.use(layoutBunja);
  const machineStore = bunja.use(machineStoreBunja);
  const machineMenu = bunja.use(machineMenuBunja);
  const rpcSession = bunja.use(rpcSessionBunja);
  const terminalShells = bunja.use(terminalShellsBunja);
  const workbench = bunja.use(workbenchBunja);

  function openMachineTitleMenu(
    event: React.MouseEvent<HTMLButtonElement>,
    machine: Machine,
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (store.get(machineMenu.machineMenuAtom)?.machineId === machine.id) {
      machineMenu.closeMachineMenu();
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    machineMenu.openMachineMenu(
      machine.id,
      rect.right - machineMenuWidth,
      rect.bottom,
    );
  }

  function openTerminalShell(shell?: AvailableShellInfo) {
    const selectedShell = shell ??
      store.get(terminalShells.defaultShellAtom);
    if (!selectedShell) return;
    workbench.openTerminalTab(
      {
        launch: {
          command: selectedShell.command,
          args: selectedShell.args,
        },
        title: selectedShell.name,
      },
    );
  }

  return {
    activeToolAtom: workbench.activeToolAtom,
    connectionAtom: rpcSession.connectionAtom,
    machineAtom: machineStore.selectedAtom,
    machinePanelCollapsedAtom: layout.machinePanelCollapsedAtom,
    machinePanelMaxWidth: layout.machinePanelMaxWidth,
    machinePanelMinWidth: layout.machinePanelMinWidth,
    machinePanelWidthAtom: layout.machinePanelWidthAtom,
    openMachineTitleMenu,
    openTerminalShell,
    resizeMachinePanelWithKeyboard: layout.resizeMachinePanelWithKeyboard,
    selectTool: workbench.selectTool,
    startMachinePanelResize: layout.startMachinePanelResize,
    defaultShellAtom: terminalShells.defaultShellAtom,
    terminalShellsAtom: terminalShells.terminalShellsAtom,
  };
});
