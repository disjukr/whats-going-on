import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { machineStoreBunja } from "../../state/machine-store.ts";
import { machineModalBunja } from "../../state/machine-modal.ts";
import { rpcSessionBunja } from "../../state/rpc-session.ts";
import { terminalShellsBunja } from "../../state/terminal-shells.ts";
import { workbenchBunja } from "../../state/workbench.ts";
import { machinePanelBunja } from "../machine-panel/state.ts";
import { layoutBunja } from "../state.tsx";
import { AgentRail } from "./agent-rail.tsx";
import { AppTopbar } from "./app-topbar.tsx";

export function TopBarRegion() {
  const layout = useBunja(layoutBunja);
  const machineStore = useBunja(machineStoreBunja);
  const machinePanel = useBunja(machinePanelBunja);
  const machineModal = useBunja(machineModalBunja);
  const rpcSession = useBunja(rpcSessionBunja);
  const terminalShellsState = useBunja(terminalShellsBunja);
  const workbench = useBunja(workbenchBunja);
  const selected = useAtomValue(machineStore.selectedAtom);
  const machines = useAtomValue(machineStore.machinesAtom);
  const selectedId = useAtomValue(machineStore.selectedIdAtom);
  const connection = useAtomValue(machinePanel.connectionAtom);
  const daemonInfo = useAtomValue(rpcSession.daemonInfoAtom);
  const activeTool = useAtomValue(workbench.activeToolAtom);
  const panes = useAtomValue(workbench.panesAtom);
  const terminalShells = useAtomValue(terminalShellsState.terminalShellsAtom);
  const machinePanelCollapsed = useAtomValue(
    layout.machinePanelCollapsedAtom,
  );

  function reconnectMachine(machineId: string) {
    machineStore.selectMachine(machineId);
    void rpcSession.reconnect();
  }

  function configureMachine(machineId: string) {
    machineModal.openConfigMachineModal(machineId);
  }

  function unpairMachine(machineId: string) {
    machineStore.clearMachineCredentials(machineId);
    machineStore.selectMachine(machineId);
    rpcSession.resetController();
  }

  function deleteMachine(machineId: string) {
    machineModal.openDeleteMachineModal(machineId);
  }

  function addMachine() {
    machineModal.openAddMachineModal();
  }

  return (
    <AppTopbar
      connection={connection}
      daemonInfo={daemonInfo}
      agentRail={<AgentRail />}
      activeTool={activeTool}
      machine={selected}
      machinePanelCollapsed={machinePanelCollapsed}
      machines={machines}
      panes={panes}
      selectedMachineId={selectedId}
      terminalShells={terminalShells}
      onAddMachine={addMachine}
      onOpenAgentTab={workbench.openAgentTab}
      onOpenDaemonTab={workbench.openDaemonTab}
      onOpenFilesTab={workbench.openFilesTab}
      onOpenProcessesTab={workbench.openProcessesTab}
      onOpenTerminalTab={workbench.openTerminalTab}
      onOpenWindowsTab={workbench.openWindowsTab}
      onSelectTab={workbench.selectTab}
      onConfigureMachine={configureMachine}
      onDeleteMachine={deleteMachine}
      onReconnectMachine={reconnectMachine}
      onSelectMachine={machineStore.selectMachine}
      onToggleMachinePanel={layout.toggleMachinePanel}
      onUnpairMachine={unpairMachine}
      onUpdateMachineIcon={(machineId, icon) =>
        machineStore.updateMachine(machineId, (machine) => ({
          ...machine,
          icon,
        }))}
    />
  );
}
