import { FormEvent, useEffect, useRef, useState } from "react";
import { useAtomValue, useSetAtom } from "jotai";
import { useBunja } from "bunja/react";
import { machineModalBunja } from "../../state/machine-modal.ts";
import { machineStoreBunja } from "../../state/machine-store.ts";
import { rpcSessionBunja } from "../../state/rpc-session.ts";
import { AddMachineForm } from "./add-machine-form.tsx";
import { MachineModal } from "./machine-modal.tsx";
import { MachinePanel } from "./machine-panel.tsx";
import { machinePanelBunja } from "./state.ts";

interface MachineAddFormContainerProps {
  showCancel: boolean;
}

export function MachinePanelRegion() {
  const machinePanel = useBunja(machinePanelBunja);
  const selected = useAtomValue(machinePanel.machineAtom);
  const connection = useAtomValue(machinePanel.connectionAtom);
  const activeTool = useAtomValue(machinePanel.activeToolAtom);
  const machinePanelCollapsed = useAtomValue(
    machinePanel.machinePanelCollapsedAtom,
  );
  const machinePanelWidth = useAtomValue(machinePanel.machinePanelWidthAtom);
  const terminalShells = useAtomValue(machinePanel.terminalShellsAtom);

  return (
    <MachinePanel
      activeTool={activeTool}
      connection={connection}
      machine={selected}
      machinePanelCollapsed={machinePanelCollapsed}
      machinePanelMaxWidth={machinePanel.machinePanelMaxWidth}
      machinePanelMinWidth={machinePanel.machinePanelMinWidth}
      machinePanelWidth={machinePanelWidth}
      onOpenMachineMenu={machinePanel.openMachineTitleMenu}
      onResizeKeyDown={machinePanel.resizeMachinePanelWithKeyboard}
      onResizePointerDown={machinePanel.startMachinePanelResize}
      onOpenTerminalShell={machinePanel.openTerminalShell}
      onSelectTool={machinePanel.selectTool}
      terminalShells={terminalShells}
    />
  );
}

export function MachineAddFormContainer(
  { showCancel }: MachineAddFormContainerProps,
) {
  const machineModal = useBunja(machineModalBunja);
  const machineStore = useBunja(machineStoreBunja);
  const machineName = useAtomValue(machineModal.machineNameAtom);
  const baseUrl = useAtomValue(machineModal.baseUrlAtom);
  const machineFormError = useAtomValue(machineModal.machineFormErrorAtom);
  const machineModalMode = useAtomValue(machineModal.machineModalModeAtom);
  const machines = useAtomValue(machineStore.machinesAtom);
  const machineNameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (machines.length === 0 && !machineModalMode) {
      machineNameInputRef.current?.focus();
    }
  }, [machineModalMode, machines.length]);

  function addMachine(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    machineModal.addMachine();
  }

  return (
    <AddMachineForm
      baseUrl={baseUrl}
      error={machineFormError}
      machineName={machineName}
      machineNameInputRef={machineNameInputRef}
      showCancel={showCancel}
      onBaseUrlChange={machineModal.updateBaseUrlDraft}
      onCancel={machineModal.closeMachineModal}
      onMachineNameChange={machineModal.updateMachineNameDraft}
      onSubmit={addMachine}
    />
  );
}

export function MachineModalHost() {
  const machineStore = useBunja(machineStoreBunja);
  const machineModal = useBunja(machineModalBunja);
  const rpcSession = useBunja(rpcSessionBunja);
  const machines = useAtomValue(machineStore.machinesAtom);
  const selected = useAtomValue(machineStore.selectedAtom);
  const machineName = useAtomValue(machineModal.machineNameAtom);
  const baseUrl = useAtomValue(machineModal.baseUrlAtom);
  const configNameDraft = useAtomValue(machineModal.configNameDraftAtom);
  const configUrlDraft = useAtomValue(machineModal.configUrlDraftAtom);
  const machineModalMode = useAtomValue(machineModal.machineModalModeAtom);
  const machineFormError = useAtomValue(machineModal.machineFormErrorAtom);
  const pairingCode = useAtomValue(machineModal.pairingCodeAtom);
  const pairingConfirmationCode = useAtomValue(
    machineModal.pairingConfirmationCodeAtom,
  );
  const pairingCodeExpiresAtUnix = useAtomValue(
    machineModal.pairingCodeExpiresAtUnixAtom,
  );
  const isRequestingPairingCode = useAtomValue(
    machineModal.isRequestingPairingCodeAtom,
  );
  const isPairing = useAtomValue(machineModal.isPairingAtom);
  const connection = useAtomValue(rpcSession.connectionAtom);
  const modalTitle = useAtomValue(machineModal.modalTitleAtom);
  const setConfigNameDraft = useSetAtom(machineModal.configNameDraftAtom);
  const setConfigUrlDraft = useSetAtom(machineModal.configUrlDraftAtom);
  const setPairingCode = useSetAtom(machineModal.pairingCodeAtom);
  const machineNameInputRef = useRef<HTMLInputElement>(null);
  const configNameInputRef = useRef<HTMLInputElement>(null);
  const pairingCodeInputRef = useRef<HTMLInputElement>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    if (machineModalMode === "add") {
      machineNameInputRef.current?.focus();
      return;
    }
    if (machineModalMode === "pair") {
      if (pairingCodeExpiresAtUnix !== undefined) {
        pairingCodeInputRef.current?.focus();
      }
      return;
    }
    if (machineModalMode === "config") {
      configNameInputRef.current?.focus();
      configNameInputRef.current?.select();
    }
  }, [machineModalMode, pairingCodeExpiresAtUnix]);

  useEffect(() => {
    if (!machineModalMode || machines.length === 0) return;

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        machineModal.closeMachineModal();
      }
    }

    globalThis.addEventListener("keydown", closeOnEscape);
    return () => globalThis.removeEventListener("keydown", closeOnEscape);
  }, [machineModal, machineModalMode, machines.length]);

  useEffect(() => {
    if (machineModalMode !== "pair") return;
    setNowMs(Date.now());
    const interval = globalThis.setInterval(() => {
      setNowMs(Date.now());
    }, 1_000);
    return () => globalThis.clearInterval(interval);
  }, [machineModalMode]);

  useEffect(() => {
    if (
      machineModalMode !== "pair" || !selected || isPairing ||
      !pairingCodeExpiresAtUnix
    ) {
      return;
    }
    const expiresAtMs = pairingCodeExpiresAtUnix * 1_000;
    const delayMs = Math.max(0, expiresAtMs - Date.now() + 250);
    const timeout = globalThis.setTimeout(() => {
      machineModal.requestSelectedPairingCode();
    }, delayMs);
    return () => globalThis.clearTimeout(timeout);
  }, [
    isPairing,
    machineModal,
    machineModalMode,
    pairingCodeExpiresAtUnix,
    selected?.id,
  ]);

  if (!machineModalMode) return null;

  function addMachine(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    machineModal.addMachine();
  }

  function saveMachineConfig(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    machineModal.saveMachineConfig();
  }

  async function pairSelected(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await machineModal.pairSelected();
  }

  const pairingCodeExpiresInSeconds = pairingCodeExpiresAtUnix === undefined
    ? undefined
    : Math.max(
      0,
      Math.ceil((pairingCodeExpiresAtUnix * 1_000 - nowMs) / 1_000),
    );

  return (
    <MachineModal
      baseUrl={baseUrl}
      configNameDraft={configNameDraft}
      configNameInputRef={configNameInputRef}
      configUrlDraft={configUrlDraft}
      connection={connection}
      isRequestingPairingCode={isRequestingPairingCode}
      isPairing={isPairing}
      machineCount={machines.length}
      machineFormError={machineFormError}
      machineName={machineName}
      machineNameInputRef={machineNameInputRef}
      mode={machineModalMode}
      modalTitle={modalTitle}
      pairingCode={pairingCode}
      pairingConfirmationCode={pairingConfirmationCode}
      pairingCodeExpiresInSeconds={pairingCodeExpiresInSeconds}
      pairingCodeInputRef={pairingCodeInputRef}
      selected={selected}
      onAddMachine={addMachine}
      onBaseUrlChange={machineModal.updateBaseUrlDraft}
      onClose={machineModal.closeMachineModal}
      onConfigNameChange={setConfigNameDraft}
      onConfigUrlChange={setConfigUrlDraft}
      onDeleteSelectedMachine={machineModal.deleteSelectedMachine}
      onMachineNameChange={machineModal.updateMachineNameDraft}
      onPairingCodeChange={setPairingCode}
      onPairSelected={pairSelected}
      onRequestPairingCode={machineModal.requestSelectedPairingCode}
      onSaveMachineConfig={saveMachineConfig}
    />
  );
}
