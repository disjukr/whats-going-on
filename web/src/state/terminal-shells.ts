import { bunja } from "bunja";
import { atom } from "jotai";
import { JotaiStoreScope } from "unsaturated/store";
import {
  type AvailableShellInfo,
  type AvailableShellsTableEvent,
  subscribeAvailableShells,
} from "../protocol/rpc.ts";
import { machineStoreBunja } from "./machine-store.ts";
import type { Machine } from "./machines.ts";
import { rpcSessionBunja } from "./rpc-session.ts";

export const terminalShellsBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const machines = bunja.use(machineStoreBunja);
  const rpcSession = bunja.use(rpcSessionBunja);

  const terminalShellsAtom = atom<AvailableShellInfo[]>([]);
  const defaultShellAtom = atom((get) => {
    const shells = get(terminalShellsAtom);
    return shells.find((item) => item.isDefault) ?? shells[0];
  });
  const terminalShellsSubscriptionKeyAtom = atom((get) =>
    terminalShellsSubscriptionKey(
      get(machines.selectedAtom),
      get(machines.selectedIsPairedAtom),
      get(rpcSession.connectionEpochAtom),
    )
  );

  bunja.effect(() => {
    let stopCurrent: (() => void) | undefined;

    function start() {
      stopCurrent?.();
      stopCurrent = undefined;
      store.set(terminalShellsAtom, []);

      const machine = store.get(machines.selectedAtom);
      if (!machine || !store.get(machines.selectedIsPairedAtom)) return;

      let cancelled = false;
      const iterator = subscribeAvailableShells(
        machine,
        machines.rpcCallOptions(rpcSession.rpcCallOptions()),
      );
      stopCurrent = () => {
        cancelled = true;
        void iterator.return(undefined);
      };
      void (async () => {
        try {
          for await (const event of iterator) {
            if (cancelled) break;
            if (event.type === "snapshot") {
              store.set(terminalShellsAtom, event.rows);
            } else {
              store.set(
                terminalShellsAtom,
                applyShellPatch(store.get(terminalShellsAtom), event),
              );
            }
          }
        } catch {
          if (!cancelled) store.set(terminalShellsAtom, []);
        }
      })();
    }

    const unsubscribe = store.sub(terminalShellsSubscriptionKeyAtom, start);
    start();
    return () => {
      unsubscribe();
      stopCurrent?.();
    };
  });

  return {
    defaultShellAtom,
    terminalShellsAtom,
  };
});

function terminalShellsSubscriptionKey(
  machine: Machine | undefined,
  selectedIsPaired: boolean,
  connectionEpoch: number,
): string {
  if (!machine || !selectedIsPaired) return "idle";
  return [
    machine.id,
    machine.baseUrl,
    machine.clientId ?? "",
    machine.clientSecret ?? "",
    connectionEpoch,
  ].join("\n");
}

function applyShellPatch(
  current: AvailableShellInfo[],
  event: Extract<AvailableShellsTableEvent, { type: "patch" }>,
): AvailableShellInfo[] {
  const removeIds = new Set(event.removes.map((item) => item.shellId));
  const retained = current.filter((shell) => !removeIds.has(shell.shellId));
  const upserts = new Map(event.upserts.map((shell) => [shell.shellId, shell]));
  return [
    ...retained.filter((shell) => !upserts.has(shell.shellId)),
    ...event.upserts,
  ];
}
