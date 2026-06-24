import React, { useEffect } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import {
  MachineBaseUrlContext,
  MachineIdContext,
} from "../../state/machine.tsx";
import { machineMenuBunja } from "../../state/machine-menu.ts";
import { machineModalBunja } from "../../state/machine-modal.ts";
import { machineStoreBunja } from "../../state/machine-store.ts";
import type { Machine } from "../../state/machines.ts";
import { rpcSessionBunja } from "../../state/rpc-session.ts";
import type { MachineMenuState } from "../../state/types.ts";
import { MachineContextMenu } from "./machine-context-menu.tsx";
import { MachineRail } from "./machine-rail.tsx";

const projectLogoUrl = new URL(
  "../../assets/wgo.svg",
  import.meta.url,
).href;

export function MachineRailRegion() {
  const machineStore = useBunja(machineStoreBunja);
  const machineMenuState = useBunja(machineMenuBunja);
  const machineModal = useBunja(machineModalBunja);
  const machines = useAtomValue(machineStore.machinesAtom);
  const selectedId = useAtomValue(machineStore.selectedIdAtom);
  const machineMenu = useAtomValue(machineMenuState.machineMenuAtom);
  const menuMachine = useAtomValue(machineMenuState.menuMachineAtom);
  const railTooltip = useAtomValue(machineMenuState.railTooltipAtom);

  useEffect(() => {
    if (!machineMenu) return;

    function closeMenu() {
      machineMenuState.closeMachineMenu();
    }

    function closeMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }

    globalThis.addEventListener("mousedown", closeMenu);
    globalThis.addEventListener("keydown", closeMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeMenu);
      globalThis.removeEventListener("keydown", closeMenuOnEscape);
    };
  }, [machineMenuState, machineMenu]);

  function openAddMachineModal() {
    machineModal.openAddMachineModal();
  }

  function openMachineContextMenu(
    event: React.MouseEvent<HTMLButtonElement>,
    machine: Machine,
  ) {
    event.preventDefault();
    event.stopPropagation();
    machineMenuState.openMachineMenu(machine.id, event.clientX, event.clientY);
  }

  function showRailTooltip(target: HTMLElement, name: string) {
    const rect = target.getBoundingClientRect();
    machineMenuState.showRailTooltip(
      name,
      rect.right + 12,
      rect.top + rect.height / 2,
    );
  }

  function openConfigMachineModal(machine: Machine) {
    machineModal.openConfigMachineModal(machine.id);
  }

  function openPairMachineModal(machine: Machine) {
    machineModal.openPairMachineModal(machine.id);
  }

  function openDeleteMachineModal(machine: Machine) {
    machineModal.openDeleteMachineModal(machine.id);
  }

  return (
    <>
      <MachineRail
        machines={machines}
        projectLogoUrl={projectLogoUrl}
        railTooltip={railTooltip}
        selectedId={selectedId}
        onAddMachine={openAddMachineModal}
        onContextMenu={openMachineContextMenu}
        onHideTooltip={machineMenuState.hideRailTooltip}
        onSelectMachine={machineMenuState.selectMachine}
        onShowTooltip={showRailTooltip}
      />

      {machineMenu && menuMachine
        ? (
          <MachineIdContext value={menuMachine.id}>
            <MachineBaseUrlContext value={menuMachine.baseUrl}>
              <MachineContextMenuHost
                machine={menuMachine}
                menu={machineMenu}
                onConfigure={openConfigMachineModal}
                onDelete={openDeleteMachineModal}
                onPair={openPairMachineModal}
              />
            </MachineBaseUrlContext>
          </MachineIdContext>
        )
        : null}
    </>
  );
}

interface MachineContextMenuHostProps {
  machine: Machine;
  menu: MachineMenuState;
  onConfigure: (machine: Machine) => void;
  onDelete: (machine: Machine) => void;
  onPair: (machine: Machine) => void;
}

function MachineContextMenuHost(
  { machine, menu, onConfigure, onDelete, onPair }: MachineContextMenuHostProps,
) {
  const machineMenuState = useBunja(machineMenuBunja);
  const machineStore = useBunja(machineStoreBunja);
  const rpcSession = useBunja(rpcSessionBunja);

  function reconnectMachine() {
    machineMenuState.closeMachineMenu();
    machineStore.selectMachine(machine.id);
    void rpcSession.reconnect();
  }

  function unpairMachine() {
    machineMenuState.closeMachineMenu();
    machineStore.clearMachineCredentials(machine.id);
    machineStore.selectMachine(machine.id);
    rpcSession.resetController();
  }

  return (
    <MachineContextMenu
      machine={machine}
      menu={menu}
      onConfigure={onConfigure}
      onDelete={onDelete}
      onPair={onPair}
      onReconnect={reconnectMachine}
      onUnpair={unpairMachine}
    />
  );
}
