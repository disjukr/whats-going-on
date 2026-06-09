import { bunja } from "bunja";
import { atom } from "jotai";
import { checkReachable, isDatagramPingTimeoutError } from "../protocol/rpc.ts";
import { JotaiStoreScope } from "./jotai-store.ts";
import { Machine } from "./machines.ts";
import { machineStoreBunja } from "./machine-store.ts";
import { ConnectionState } from "./types.ts";

const STATUS_PING_INTERVAL_MS = 5_000;

export const connectionBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const machines = bunja.use(machineStoreBunja);

  const connectionAtom = atom<ConnectionState>({
    phase: "idle",
    message: "No machine selected",
  });
  const connectionEpochAtom = atom(0);

  let connectionReachable = false;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function setChecking(message: string) {
    store.set(connectionAtom, { phase: "checking", message });
  }

  function markReachable(message: string, latencyMs: number) {
    if (!connectionReachable) {
      store.set(connectionEpochAtom, (current) => current + 1);
    }
    connectionReachable = true;
    store.set(connectionAtom, { phase: "reachable", message, latencyMs });
  }

  function markOffline(message: string) {
    connectionReachable = false;
    store.set(connectionAtom, { phase: "offline", message });
  }

  function markDatagramPingTimeout() {
    connectionReachable = false;
    store.set(connectionAtom, { phase: "offline", message: "No pong" });
  }

  function restartPingLoop() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
    connectionReachable = false;
    const selected = store.get(machines.selectedAtom);
    if (!selected) {
      store.set(connectionAtom, {
        phase: "idle",
        message: "No machine selected",
      });
      return;
    }
    void pingStatus(selected, true);
  }

  async function pingStatus(machine: Machine, showChecking: boolean) {
    const key = connectionKey(machine);
    if (showChecking) setChecking("Checking transport");

    try {
      const latencyMs = await checkReachable(machine);
      if (stopped || connectionKey(store.get(machines.selectedAtom)) !== key) {
        return;
      }
      markReachable(formatLatency(latencyMs), latencyMs);
    } catch (err) {
      if (stopped || connectionKey(store.get(machines.selectedAtom)) !== key) {
        return;
      }
      if (isDatagramPingTimeoutError(err)) {
        markDatagramPingTimeout();
        return;
      }
      markOffline(connectionErrorMessage(err, machine));
    } finally {
      if (!stopped && connectionKey(store.get(machines.selectedAtom)) === key) {
        timer = setTimeout(
          () => void pingStatus(machine, false),
          STATUS_PING_INTERVAL_MS,
        );
      }
    }
  }

  async function checkSelected() {
    const selected = store.get(machines.selectedAtom);
    if (!selected) return;

    setChecking("Checking transport");
    try {
      const latencyMs = await checkReachable(selected);
      if (
        connectionKey(store.get(machines.selectedAtom)) !==
          connectionKey(selected)
      ) {
        return;
      }
      markReachable(formatLatency(latencyMs), latencyMs);
    } catch (err) {
      if (isDatagramPingTimeoutError(err)) {
        markDatagramPingTimeout();
        return;
      }
      markOffline(connectionErrorMessage(err, selected));
    }
  }

  bunja.effect(() => {
    const unsubscribe = store.sub(machines.selectedAtom, restartPingLoop);
    restartPingLoop();
    return () => {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      unsubscribe();
    };
  });

  return {
    connectionAtom,
    connectionEpochAtom,
    setChecking,
    markReachable,
    markOffline,
    checkSelected,
  };
});

function connectionKey(machine?: Machine): string {
  if (!machine) return "";
  return [
    machine.id,
    machine.baseUrl,
    machine.clientId ?? "",
    machine.clientSecret ?? "",
  ].join("\n");
}

function formatLatency(latencyMs: number): string {
  return `${Math.max(1, Math.round(latencyMs))}ms`;
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function connectionErrorMessage(
  err: unknown,
  machine?: Machine,
): string {
  const message = errorMessage(err);
  if (!message.toLowerCase().includes("handshake")) return message;

  const host = machine ? safeHost(machine.baseUrl) : "";
  if (host === "localhost" || host === "127.0.0.1" || host === "::1") {
    return "WebTransport TLS handshake failed. Use the daemon URL printed by the pairing command; localhost will fail when the daemon certificate is issued for another host.";
  }
  return "WebTransport TLS handshake failed. Check that the daemon URL matches a trusted certificate host.";
}

function safeHost(raw: string): string {
  try {
    return new URL(raw).hostname.toLowerCase();
  } catch {
    return "";
  }
}
