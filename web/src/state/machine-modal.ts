import { bunja } from "bunja";
import { atom } from "jotai";
import { JotaiStoreScope } from "unsaturated/store";
import { machineMenuBunja } from "./machine-menu.ts";
import { machineStoreBunja } from "./machine-store.ts";
import { Machine, normalizeMachineUrl } from "./machines.ts";
import { rpcSessionBunja } from "./rpc-session.ts";
import { MachineModalMode } from "./types.ts";

export const machineModalBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const machines = bunja.use(machineStoreBunja);
  const menu = bunja.use(machineMenuBunja);
  const rpcSession = bunja.use(rpcSessionBunja);

  const machineNameAtom = atom("");
  const machineNameEditedAtom = atom(false);
  const baseUrlAtom = atom("");
  const configNameDraftAtom = atom("");
  const configUrlDraftAtom = atom("");
  const machineModalModeAtom = atom<MachineModalMode | undefined>(undefined);
  const machineFormErrorAtom = atom("");
  const pairingCodeAtom = atom("");
  const pairingConfirmationCodeAtom = atom<string | undefined>(undefined);
  const pairingCodeExpiresAtUnixAtom = atom<number | undefined>(undefined);
  const isRequestingPairingCodeAtom = atom(false);
  const isPairingAtom = atom(false);
  const modalTitleAtom = atom((get) =>
    machineModalTitle(get(machineModalModeAtom))
  );
  let pairingRequestSerial = 0;

  function addMachine() {
    store.set(machineFormErrorAtom, "");
    let normalizedBaseUrl;
    try {
      normalizedBaseUrl = normalizeMachineUrl(store.get(baseUrlAtom));
    } catch (err) {
      store.set(machineFormErrorAtom, errorMessage(err));
      return;
    }

    const machine: Machine = {
      id: crypto.randomUUID(),
      name: store.get(machineNameAtom).trim() ||
        inferMachineNameFromUrl(normalizedBaseUrl),
      baseUrl: normalizedBaseUrl,
    };
    machines.addMachine(machine);
    store.set(machineNameAtom, "");
    store.set(machineNameEditedAtom, false);
    store.set(baseUrlAtom, "");
    resetPairingCode();
    store.set(machineModalModeAtom, "pair");
    requestPairingCode(machine);
  }

  function closeMachineModal() {
    if (store.get(machines.machinesAtom).length === 0) return;
    store.set(machineModalModeAtom, undefined);
    store.set(machineFormErrorAtom, "");
    cancelPairingRequest();
    resetPairingCode();
  }

  function openAddMachineModal() {
    menu.closeMachineMenu();
    store.set(machineNameAtom, "");
    store.set(machineNameEditedAtom, false);
    store.set(baseUrlAtom, "");
    store.set(machineFormErrorAtom, "");
    cancelPairingRequest();
    resetPairingCode();
    if (store.get(machines.machinesAtom).length === 0) {
      store.set(machineModalModeAtom, undefined);
      return;
    }
    store.set(machineModalModeAtom, "add");
  }

  function openConfigMachineModal(machineId: string) {
    const machine = machines.findMachine(machineId);
    if (!machine) return;
    machines.selectMachine(machine.id);
    store.set(configNameDraftAtom, machine.name);
    store.set(configUrlDraftAtom, machine.baseUrl);
    store.set(machineFormErrorAtom, "");
    cancelPairingRequest();
    resetPairingCode();
    menu.closeMachineMenu();
    store.set(machineModalModeAtom, "config");
  }

  function openPairMachineModal(machineId: string) {
    const machine = machines.findMachine(machineId);
    if (!machine) return;
    machines.selectMachine(machine.id);
    cancelPairingRequest();
    resetPairingCode();
    store.set(machineFormErrorAtom, "");
    menu.closeMachineMenu();
    store.set(machineModalModeAtom, "pair");
    requestPairingCode(machine);
  }

  function openDeleteMachineModal(machineId: string) {
    const machine = machines.findMachine(machineId);
    if (!machine) return;
    machines.selectMachine(machine.id);
    store.set(machineFormErrorAtom, "");
    cancelPairingRequest();
    resetPairingCode();
    menu.closeMachineMenu();
    store.set(machineModalModeAtom, "delete");
  }

  function saveMachineConfig() {
    const selected = store.get(machines.selectedAtom);
    if (!selected) return;
    const name = store.get(configNameDraftAtom).trim();
    if (!name) {
      store.set(machineFormErrorAtom, "Name is required");
      return;
    }
    let normalizedBaseUrl;
    try {
      normalizedBaseUrl = normalizeMachineUrl(store.get(configUrlDraftAtom));
    } catch (err) {
      store.set(machineFormErrorAtom, errorMessage(err));
      return;
    }
    machines.updateMachine(selected.id, (machine) => ({
      ...machine,
      name,
      baseUrl: normalizedBaseUrl,
    }));
    store.set(machineFormErrorAtom, "");
    store.set(machineModalModeAtom, undefined);
  }

  function deleteSelectedMachine() {
    const deleted = machines.deleteSelectedMachine();
    if (!deleted) return;
    store.set(machineFormErrorAtom, "");
    cancelPairingRequest();
    resetPairingCode();
    store.set(machineModalModeAtom, undefined);
  }

  async function pairSelected() {
    const selected = store.get(machines.selectedAtom);
    if (!selected || store.get(isPairingAtom)) return;
    const code = store.get(pairingCodeAtom).trim();
    if (!code) return;
    store.set(isPairingAtom, true);
    store.set(machineFormErrorAtom, "");
    try {
      const credentials = await rpcSession.completePairingSession(code);
      machines.setMachineCredentials(selected.id, credentials);
      resetPairingCode();
      store.set(machineModalModeAtom, undefined);
    } catch (err) {
      store.set(machineFormErrorAtom, errorMessage(err));
    } finally {
      store.set(isPairingAtom, false);
    }
  }

  function requestPairingCode(machine: Machine) {
    const serial = ++pairingRequestSerial;
    const confirmationCode = createPairingConfirmationCode();
    const clientLabel = browserClientLabel();
    store.set(machineFormErrorAtom, "");
    resetPairingCode();
    store.set(pairingConfirmationCodeAtom, confirmationCode);
    store.set(isRequestingPairingCodeAtom, true);
    void (async () => {
      try {
        const { pairingCodeExpiresAtUnix } = await rpcSession
          .startPairingSession(
            confirmationCode,
            clientLabel,
          );
        if (!isCurrentPairingRequest(machine, serial)) return;
        store.set(pairingCodeExpiresAtUnixAtom, pairingCodeExpiresAtUnix);
      } catch (err) {
        if (!isCurrentPairingRequest(machine, serial)) return;
        store.set(machineFormErrorAtom, errorMessage(err));
      } finally {
        if (serial === pairingRequestSerial) {
          store.set(isRequestingPairingCodeAtom, false);
        }
      }
    })();
  }

  function requestSelectedPairingCode() {
    const selected = store.get(machines.selectedAtom);
    if (!selected) return;
    requestPairingCode(selected);
  }

  function cancelPairingRequest() {
    pairingRequestSerial++;
    store.set(isRequestingPairingCodeAtom, false);
  }

  function resetPairingCode() {
    store.set(pairingCodeAtom, "");
    store.set(pairingConfirmationCodeAtom, undefined);
    store.set(pairingCodeExpiresAtUnixAtom, undefined);
  }

  function isCurrentPairingRequest(machine: Machine, serial: number) {
    return serial === pairingRequestSerial &&
      store.get(machineModalModeAtom) === "pair" &&
      store.get(machines.selectedAtom)?.id === machine.id &&
      !!machines.findMachine(machine.id);
  }

  function updateMachineNameDraft(value: string) {
    store.set(machineNameAtom, value);
    store.set(machineNameEditedAtom, value.trim().length > 0);
  }

  function updateBaseUrlDraft(value: string) {
    store.set(baseUrlAtom, value);
    if (!store.get(machineNameEditedAtom)) {
      store.set(machineNameAtom, inferMachineNameFromUrl(value));
    }
  }

  return {
    machineNameAtom,
    baseUrlAtom,
    configNameDraftAtom,
    configUrlDraftAtom,
    machineModalModeAtom,
    machineFormErrorAtom,
    pairingCodeAtom,
    pairingConfirmationCodeAtom,
    pairingCodeExpiresAtUnixAtom,
    isRequestingPairingCodeAtom,
    isPairingAtom,
    modalTitleAtom,
    addMachine,
    closeMachineModal,
    openAddMachineModal,
    openConfigMachineModal,
    openPairMachineModal,
    openDeleteMachineModal,
    saveMachineConfig,
    deleteSelectedMachine,
    pairSelected,
    requestSelectedPairingCode,
    updateMachineNameDraft,
    updateBaseUrlDraft,
  };
});

function machineModalTitle(mode?: MachineModalMode): string {
  switch (mode) {
    case "pair":
      return "Pair machine";
    case "config":
      return "Machine config";
    case "delete":
      return "Delete machine";
    case "add":
    case undefined:
      return "Add machine";
  }
}

export function inferMachineNameFromUrl(raw: string): string {
  try {
    const hostname = new URL(normalizeMachineUrl(raw)).hostname;
    const firstLabel = hostname.split(".")[0] ?? "";
    if (hostname.includes(".") && /[a-z]/i.test(firstLabel)) {
      return firstLabel;
    }
    return hostname;
  } catch {
    return "";
  }
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function createPairingConfirmationCode(): string {
  const bytes = new Uint8Array(1);
  crypto.getRandomValues(bytes);
  return (bytes[0] % 100).toString().padStart(2, "0");
}

function browserClientLabel(): string {
  return globalThis.navigator?.userAgent.trim() || "web";
}
