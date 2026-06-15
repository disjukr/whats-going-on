import { bunja } from "bunja";
import { atom } from "jotai";
import type { RpcCallOptions } from "../protocol/rpc.ts";
import { loadMachines, Machine, saveMachines } from "./machines.ts";
import { JotaiStoreScope } from "./jotai-store.ts";
import { MachineIdScope } from "./machine-id.tsx";

const initialMachines = loadMachines();

export const machineStoreBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);

  const machinesAtom = atom<Machine[]>(initialMachines);
  const selectedIdAtom = atom<string | undefined>(initialMachines[0]?.id);
  const selectedAtom = atom((get) =>
    getMachine(get(machinesAtom), get(selectedIdAtom))
  );
  const selectedIsPairedAtom = atom((get) => isPaired(get(selectedAtom)));

  function selectMachine(machineId?: string) {
    store.set(selectedIdAtom, machineId);
  }

  function findMachine(machineId?: string): Machine | undefined {
    return getMachine(store.get(machinesAtom), machineId);
  }

  function addMachine(machine: Machine) {
    store.set(machinesAtom, (current) => [...current, machine]);
    store.set(selectedIdAtom, machine.id);
  }

  function updateMachine(
    machineId: string,
    update: (machine: Machine) => Machine,
  ) {
    store.set(
      machinesAtom,
      (current) =>
        current.map((machine) =>
          machine.id === machineId ? update(machine) : machine
        ),
    );
  }

  function setMachineCredentials(
    machineId: string,
    credentials: {
      clientId: string;
      clientSecret: string;
      clientCredentialExpiresAtUnix: number;
    },
  ) {
    updateMachine(machineId, (machine) => ({ ...machine, ...credentials }));
  }

  function clearMachineCredentials(machineId: string) {
    updateMachine(
      machineId,
      ({ clientSecret: _clientSecret, ...machine }) => machine,
    );
  }

  function setMachineCredentialExpiry(
    machineId: string,
    clientCredentialExpiresAtUnix: number,
  ) {
    updateMachine(machineId, (machine) => ({
      ...machine,
      clientCredentialExpiresAtUnix,
    }));
  }

  function deleteSelectedMachine(): Machine | undefined {
    const selected = store.get(selectedAtom);
    if (!selected) return undefined;
    const remaining = store.get(machinesAtom).filter((machine) =>
      machine.id !== selected.id
    );
    store.set(machinesAtom, remaining);
    store.set(
      selectedIdAtom,
      (current) => current === selected.id ? remaining[0]?.id : current,
    );
    return selected;
  }

  bunja.effect(() =>
    store.sub(machinesAtom, () => {
      saveMachines(store.get(machinesAtom));
    })
  );

  function rpcCallOptions(): RpcCallOptions {
    return {
      onClientCredentialRenewal: (renewal) => {
        const machine = getMachine(store.get(machinesAtom), renewal.machineId);
        if (
          machine?.clientId !== renewal.clientId ||
          machine.clientSecret !== renewal.clientSecret
        ) {
          return;
        }
        setMachineCredentialExpiry(
          renewal.machineId,
          renewal.clientCredentialExpiresAtUnix,
        );
      },
    };
  }

  return {
    machinesAtom,
    selectedIdAtom,
    selectedAtom,
    selectedIsPairedAtom,
    selectMachine,
    findMachine,
    addMachine,
    updateMachine,
    setMachineCredentials,
    clearMachineCredentials,
    setMachineCredentialExpiry,
    rpcCallOptions,
    deleteSelectedMachine,
  };
});

export const machineBunja = bunja(() => {
  const machineId = bunja.use(MachineIdScope);
  const machines = bunja.use(machineStoreBunja);

  const machineAtom = atom((get) =>
    getMachine(get(machines.machinesAtom), machineId)
  );
  const isPairedAtom = atom((get) => isPaired(get(machineAtom)));

  return {
    machineId,
    machineAtom,
    isPairedAtom,
  };
});

function getMachine(
  machines: Machine[],
  machineId?: string,
): Machine | undefined {
  return machines.find((machine) => machine.id === machineId);
}

export function isPaired(machine?: Machine): boolean {
  return Boolean(machine?.clientId && machine?.clientSecret);
}
