import { bunja } from "bunja";
import { atom } from "jotai";
import { JotaiStoreScope } from "unsaturated/store";
import {
  createAgentProject,
  createAgentSession,
  listAgentSessions,
  subscribeAgentProjects,
  subscribeAgentProviders,
  updateAgentSession,
} from "../protocol/generated/client.ts";
import {
  type AgentProjectInfo,
  type AgentProjectsTableEvent,
  AgentProviderAvailability,
  type AgentProviderInfo,
  type AgentProvidersTableEvent,
  AgentSessionArchiveFilter,
  type AgentSessionInfo,
  type AgentSessionSummary,
  type CreateAgentWorkspace,
} from "../protocol/generated/rpc.ts";
import { machineStoreBunja } from "./machine-store.ts";
import type { Machine } from "./machines.ts";
import { rpcSessionBunja } from "./rpc-session.ts";

export type AgentCatalogPhase = "idle" | "loading" | "ready" | "error";

export function agentSessionTitle(
  session: Pick<AgentSessionSummary, "title" | "lastMessagePreview">,
): string {
  return session.title ?? session.lastMessagePreview ?? "New session";
}

export const agentsBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const machines = bunja.use(machineStoreBunja);
  const rpcSession = bunja.use(rpcSessionBunja);

  const providersAtom = atom<AgentProviderInfo[]>([]);
  const projectsAtom = atom<AgentProjectInfo[]>([]);
  const sessionsAtom = atom<AgentSessionSummary[]>([]);
  const phaseAtom = atom<AgentCatalogPhase>("idle");
  const errorAtom = atom<string | undefined>(undefined);
  const availableProvidersAtom = atom((get) =>
    get(providersAtom).filter((provider) =>
      provider.availability === AgentProviderAvailability.Available
    )
  );
  const subscriptionKeyAtom = atom((get) =>
    agentSubscriptionKey(
      get(machines.selectedAtom),
      get(machines.selectedIsPairedAtom),
      get(rpcSession.daemonInstanceIdAtom),
    )
  );

  let refreshVersion = 0;

  bunja.effect(() => {
    let stopCurrent: (() => void) | undefined;

    function start() {
      stopCurrent?.();
      stopCurrent = undefined;
      refreshVersion += 1;
      store.set(providersAtom, []);
      store.set(projectsAtom, []);
      store.set(sessionsAtom, []);
      store.set(errorAtom, undefined);

      const machine = store.get(machines.selectedAtom);
      const paired = store.get(machines.selectedIsPairedAtom);
      const daemonInstanceId = store.get(rpcSession.daemonInstanceIdAtom);
      if (!machine || !paired || !daemonInstanceId) {
        store.set(phaseAtom, "idle");
        return;
      }

      store.set(phaseAtom, "loading");
      let cancelled = false;
      let providers: AsyncGenerator<AgentProvidersTableEvent> | undefined;
      let projects: AsyncGenerator<AgentProjectsTableEvent> | undefined;
      stopCurrent = () => {
        cancelled = true;
        void providers?.return(undefined);
        void projects?.return(undefined);
      };

      void (async () => {
        try {
          const transport = await rpcSession.webTransport();
          if (cancelled) return;
          providers = subscribeAgentProviders(transport);
          for await (const event of providers) {
            if (cancelled) break;
            store.set(
              providersAtom,
              applyProviderEvent(store.get(providersAtom), event),
            );
          }
        } catch (error) {
          if (!cancelled) setError(error);
        }
      })();

      void (async () => {
        try {
          const transport = await rpcSession.webTransport();
          if (cancelled) return;
          projects = subscribeAgentProjects(transport);
          for await (const event of projects) {
            if (cancelled) break;
            store.set(
              projectsAtom,
              applyProjectEvent(store.get(projectsAtom), event),
            );
          }
        } catch (error) {
          if (!cancelled) setError(error);
        }
      })();

      void refreshSessions().then(() => {
        if (!cancelled && store.get(phaseAtom) !== "error") {
          store.set(phaseAtom, "ready");
        }
      });
    }

    const unsubscribe = store.sub(subscriptionKeyAtom, start);
    start();
    return () => {
      unsubscribe();
      stopCurrent?.();
    };
  });

  async function refreshSessions() {
    const version = ++refreshVersion;
    try {
      const response = await listAgentSessions(
        await rpcSession.webTransport(),
        {
          archived: AgentSessionArchiveFilter.ActiveOnly,
          limit: 100,
          workspace: { type: "any" },
        },
      );
      if (version !== refreshVersion) return;
      store.set(sessionsAtom, response.rows);
      store.set(errorAtom, undefined);
      store.set(phaseAtom, "ready");
    } catch (error) {
      if (version === refreshVersion) setError(error);
    }
  }

  async function addProject(rootPath: string): Promise<AgentProjectInfo> {
    const project = await createAgentProject(
      await rpcSession.webTransport(),
      { rootPath },
    );
    store.set(
      projectsAtom,
      (current) => upsertById(current, project, (item) => item.projectId),
    );
    return project;
  }

  async function addSession(
    providerId: string,
    workspace: CreateAgentWorkspace,
  ): Promise<AgentSessionInfo> {
    const session = await createAgentSession(
      await rpcSession.webTransport(),
      {
        creationRequestId: crypto.randomUUID(),
        providerId,
        workspace,
      },
    );
    store.set(
      sessionsAtom,
      (current) =>
        upsertById(current, session.summary, (item) => item.sessionId),
    );
    return session;
  }

  async function archiveSession(sessionId: string): Promise<AgentSessionInfo> {
    const session = await updateAgentSession(
      await rpcSession.webTransport(),
      {
        archived: true,
        sessionId,
      },
    );
    store.set(
      sessionsAtom,
      (current) => current.filter((item) => item.sessionId !== sessionId),
    );
    return session;
  }

  function setError(error: unknown) {
    store.set(
      errorAtom,
      error instanceof Error ? error.message : String(error),
    );
    store.set(phaseAtom, "error");
  }

  return {
    addProject,
    addSession,
    archiveSession,
    availableProvidersAtom,
    errorAtom,
    phaseAtom,
    projectsAtom,
    providersAtom,
    refreshSessions,
    sessionsAtom,
  };
});

function agentSubscriptionKey(
  machine: Machine | undefined,
  selectedIsPaired: boolean,
  daemonInstanceId: string | undefined,
): string {
  if (!machine || !selectedIsPaired || !daemonInstanceId) return "idle";
  return [
    machine.id,
    machine.baseUrl,
    machine.clientId ?? "",
    daemonInstanceId,
  ].join("\n");
}

function applyProviderEvent(
  current: AgentProviderInfo[],
  event: AgentProvidersTableEvent,
): AgentProviderInfo[] {
  return event.type === "snapshot"
    ? event.rows
    : applyPatch(current, event.removes, event.upserts, (item) =>
      item.providerId);
}

function applyProjectEvent(
  current: AgentProjectInfo[],
  event: AgentProjectsTableEvent,
): AgentProjectInfo[] {
  return event.type === "snapshot"
    ? event.rows
    : applyPatch(current, event.removes, event.upserts, (item) =>
      item.projectId);
}

function applyPatch<T>(
  current: T[],
  removes: string[],
  upserts: T[],
  id: (item: T) => string,
): T[] {
  const removed = new Set(removes);
  const next = current.filter((item) => !removed.has(id(item)));
  for (const item of upserts) {
    const index = next.findIndex((currentItem) => id(currentItem) === id(item));
    if (index < 0) next.push(item);
    else next[index] = item;
  }
  return next;
}

function upsertById<T>(
  current: T[],
  item: T,
  id: (item: T) => string,
): T[] {
  const itemId = id(item);
  const index = current.findIndex((currentItem) => id(currentItem) === itemId);
  if (index < 0) return [item, ...current];
  return current.map((currentItem, currentIndex) =>
    currentIndex === index ? item : currentItem
  );
}
