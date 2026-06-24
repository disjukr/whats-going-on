import { useEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { nowBunja } from "unsaturated/now";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronRight,
  Info,
  Loader2,
  SquareTerminal,
  WifiOff,
} from "lucide-react";
import {
  type ClientInfo,
  type ClientsTableEvent,
  closeTerminalSession,
  renewClientCredential,
  subscribeClients,
  subscribeTerminalSessions,
  type TerminalSessionInfo,
  type TerminalSessionsTableEvent,
} from "../../../../protocol/rpc.ts";
import { machineStoreBunja } from "../../../../state/machine-store.ts";
import { rpcSessionBunja } from "../../../../state/rpc-session.ts";
import type { ConnectionState } from "../../../../state/types.ts";
import {
  workbenchBunja,
  workbenchTabBunja,
} from "../../../../state/workbench.ts";
import { Button } from "../../../ui/button.tsx";

const procNames = new Map<number, string>([
  [1, "GetDaemonInfo"],
  [2, "StartPairing"],
  [3, "CompletePairing"],
  [4, "RenewClientCredential"],
  [5, "SubscribeRoots"],
  [6, "SubscribeDirectory"],
  [7, "ReadFile"],
  [8, "WriteFile"],
  [9, "CreateNodes"],
  [10, "RenamePaths"],
  [11, "DeletePaths"],
  [12, "CreateTerminalSession"],
  [13, "SubscribeTerminalSessions"],
  [14, "SubscribeAvailableShells"],
  [15, "AttachTerminalSession"],
  [16, "TakeTerminalControl"],
  [17, "WriteTerminalInput"],
  [18, "CloseTerminalSession"],
  [19, "SubscribeClients"],
]);

const daemonToolClassName = [
  "h-full min-h-0 overflow-auto bg-white px-[18px] py-[16px]",
  "text-[#20242d]",
].join(" ");
const statusPillClassName = [
  "inline-flex flex-[0_0_auto] items-center gap-[6px] min-w-0 rounded-[999px]",
  "bg-[#eef3fb] px-[9px] py-[4px] text-[12px] font-700 text-[#344054]",
  "[&_svg]:flex-[0_0_auto]",
  "[&.reachable]:bg-[#ecfdf3] [&.reachable]:text-[#027a48]",
  "[&.checking]:bg-[#fff7e6] [&.checking]:text-[#945800]",
  "[&.offline]:bg-[#fff1f3] [&.offline]:text-[#b42318]",
].join(" ");
const daemonHeaderClassName = [
  "mb-[14px] flex min-w-0 items-center justify-between gap-[12px]",
].join(" ");
const daemonBreadcrumbClassName = [
  "flex h-[28px] min-h-[28px] flex-[1_1_auto] min-w-0 items-center gap-[4px] overflow-hidden",
  "[&_svg]:flex-[0_0_auto] [&_svg]:text-[#98a2b3]",
  "[&_button]:inline-flex [&_button]:appearance-none [&_button]:cursor-pointer",
  "[&_button]:items-center [&_button]:[font-family:inherit]",
  "[&_button]:h-[28px] [&_button]:min-w-0 [&_button]:max-w-[180px]",
  "[&_button]:overflow-hidden [&_button]:text-ellipsis",
  "[&_button]:whitespace-nowrap [&_button]:rounded-[6px]",
  "[&_button]:border-transparent [&_button]:bg-transparent",
  "[&_button]:px-[8px] [&_button]:text-[13px] [&_button]:font-750",
  "[&_button]:text-[#344054] [&_button:hover]:bg-[#eef3fb]",
  "[&_span]:inline-flex [&_span]:h-[28px] [&_span]:min-w-0",
  "[&_span]:max-w-[240px] [&_span]:items-center [&_span]:overflow-hidden",
  "[&_span]:text-ellipsis [&_span]:whitespace-nowrap",
  "[&_span]:px-[8px] [&_span]:text-[13px] [&_span]:font-750",
  "[&_span]:text-[#20242d]",
].join(" ");
const daemonBreadcrumbMutedClassName = "text-[#667085]";
const summaryGridClassName = [
  "grid grid-cols-1 gap-[1px] overflow-hidden",
  "@[640px]:grid-cols-2 @[960px]:grid-cols-3",
  "border border-[#d8dde7] rounded-[8px] bg-[#d8dde7]",
].join(" ");
const summaryItemClassName = [
  "grid min-w-0 gap-[8px] bg-[#fbfcfe] px-[16px] py-[14px]",
  "[&_dt]:m-0 [&_dt]:text-[12px] [&_dt]:font-700 [&_dt]:text-[#667085]",
  "[&_dd]:m-0 [&_dd]:min-w-0 [&_dd]:text-[14px]",
  "[&_dd]:text-[#20242d] [&_dd]:[overflow-wrap:anywhere]",
].join(" ");
const infoValueClassName = [
  "inline-block max-w-full rounded-[4px] border border-[#d8dde7]",
  "bg-[#f4f6fa] px-[4px] py-[1px] font-mono text-[0.92em]",
  "leading-[1.4] text-[#20242d] [overflow-wrap:anywhere]",
].join(" ");
const summaryValueWithNoteClassName = "grid justify-items-start gap-[7px]";
const summaryNoteClassName = "text-[12px] text-[#667085]";
const procSectionClassName = "mt-[18px] min-w-0";
const procSectionTitleClassName =
  "mb-[8px] text-[12px] font-800 uppercase text-[#667085]";
const sectionTitleButtonClassName = [
  "mb-[8px] inline-flex h-[24px] appearance-none items-center gap-[3px] rounded-[6px]",
  "cursor-pointer border-transparent bg-transparent px-[4px] text-[12px]",
  "font-800 uppercase text-[#667085] [font-family:inherit]",
  "hover:bg-[#eef3fb] hover:text-[#344054]",
  "[&_svg]:flex-[0_0_auto]",
].join(" ");
const clientsSectionClassName = "mt-[18px] min-w-0";
const clientOverviewCardClassName = [
  "min-w-0 rounded-[8px] border border-[#d8dde7] bg-[#fbfcfe]",
  "grid gap-[8px] px-[12px] py-[10px] text-[13px] text-[#667085]",
].join(" ");
const clientOverviewCurrentButtonClassName = [
  "grid min-w-0 appearance-none gap-[3px] rounded-[6px] px-[8px] py-[7px] text-left",
  "cursor-pointer text-[#344054] hover:bg-[#eef3fb]",
  "[font-family:inherit]",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:text-[13px] [&_strong]:font-750",
  "[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis",
  "[&_span]:whitespace-nowrap [&_span]:text-[11px] [&_span]:text-[#667085]",
].join(" ");
const clientOverviewMetaClassName =
  "px-[8px] text-[12px] font-650 text-[#667085]";
const clientOverviewMetaButtonClassName = [
  "inline min-h-0 appearance-none justify-self-start gap-0 border-0 rounded-0",
  "cursor-pointer bg-transparent p-0 text-left text-[12px] font-650 text-[#667085]",
  "[font-family:inherit]",
  "hover:border-transparent hover:bg-transparent hover:text-[#344054]",
].join(" ");
const clientListClassName = [
  "grid content-start gap-[6px] min-w-0 rounded-[8px]",
  "border border-[#d8dde7] bg-[#fbfcfe] p-[6px]",
].join(" ");
const clientButtonClassName = [
  "grid min-w-0 appearance-none gap-[3px] rounded-[6px] px-[10px] py-[8px] text-left",
  "cursor-pointer text-[#344054] hover:bg-[#eef3fb]",
  "[font-family:inherit]",
  "[&.selected]:bg-[#e8eef7] [&.selected]:text-[#20242d]",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:text-[13px] [&_strong]:font-750",
  "[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis",
  "[&_span]:whitespace-nowrap [&_span]:text-[11px] [&_span]:text-[#667085]",
].join(" ");
const clientNameRowClassName = "flex min-w-0 items-center gap-[6px]";
const currentClientBadgeClassName = [
  "flex-[0_0_auto] rounded-[999px] bg-[#dff6e7] px-[6px] py-[1px]",
  "text-[10px] not-italic font-800 text-[#027a48]",
].join(" ");
const clientInfoCurrentClientBadgeClassName = [
  "flex-[0_0_auto] rounded-[999px] bg-[#dff6e7] px-[6px] py-[1px]",
  "text-[10px] not-italic text-[#027a48]",
].join(" ");
const clientDetailPageClassName = "grid min-w-0 gap-[14px]";
const clientDetailSectionClassName = [
  "min-w-0 overflow-hidden rounded-[8px] border border-[#d8dde7] bg-white",
].join(" ");
const clientDetailSectionHeaderClassName = [
  "flex min-h-[38px] min-w-0 items-center justify-between gap-[10px]",
  "border-b border-b-[#edf0f5] bg-[#fbfcfe] px-[12px] py-[9px]",
  "[&_h2]:m-0 [&_h2]:min-w-0 [&_h2]:overflow-hidden",
  "[&_h2]:text-ellipsis [&_h2]:whitespace-nowrap",
  "[&_h2]:text-[12px] [&_h2]:font-800 [&_h2]:uppercase",
  "[&_h2]:text-[#667085]",
  "[&_span]:flex-[0_0_auto] [&_span]:text-[12px]",
  "[&_span]:font-650 [&_span]:text-[#667085]",
].join(" ");
const clientInfoGridClassName = [
  "m-0 grid min-w-0 grid-cols-1 gap-[1px] bg-[#edf0f5]",
].join(" ");
const clientInfoItemClassName = [
  "grid min-w-0 gap-[5px] bg-white px-[12px] py-[10px]",
  "[&_dt]:m-0 [&_dt]:text-[12px] [&_dt]:font-700 [&_dt]:text-[#667085]",
  "[&_dd]:m-0 [&_dd]:min-w-0 [&_dd]:text-[13px]",
  "[&_dd]:text-[#20242d] [&_dd]:[overflow-wrap:anywhere]",
].join(" ");
const clientIdValueClassName = "flex min-w-0 flex-wrap items-center gap-[7px]";
const credentialExpiryValueClassName = "grid justify-items-start gap-[7px]";
const credentialExpiryRemainingClassName = "text-[12px] text-[#667085]";
const credentialCreatedValueClassName = credentialExpiryValueClassName;
const credentialCreatedAgeClassName = credentialExpiryRemainingClassName;
const renewCredentialButtonClassName = "!min-h-[28px] !px-[9px] !text-[12px]";
const renewCredentialMessageClassName = "text-[12px] text-[#667085]";
const renewCredentialErrorClassName = "text-[12px] text-[#b42318]";
const terminalSessionListClassName = [
  "grid min-w-0",
  "[&_article]:grid [&_article]:min-w-0",
  "[&_article]:grid-cols-[minmax(0,1fr)_auto_auto]",
  "[&_article]:gap-x-[12px] [&_article]:gap-y-[4px]",
  "[&_article]:items-center",
  "[&_article]:border-b [&_article]:border-b-[#edf0f5]",
  "[&_article]:px-[12px] [&_article]:py-[10px]",
  "[&_article:last-child]:border-b-0",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:text-[13px] [&_strong]:font-750",
  "[&_span]:text-[12px] [&_span]:text-[#667085]",
  "[&_small]:col-span-3 [&_small]:min-w-0 [&_small]:[overflow-wrap:anywhere]",
  "[&_small]:text-[12px] [&_small]:text-[#667085]",
].join(" ");
const terminalSessionActionsClassName =
  "flex min-w-0 items-center justify-end gap-[6px]";
const terminalSessionCloseButtonClassName =
  "!min-h-[28px] !px-[9px] !text-[12px]";
const clientDetailStateClassName =
  "px-[12px] py-[11px] text-[13px] text-[#667085]";
const procTableClassName = [
  "grid overflow-hidden border border-[#d8dde7] rounded-[8px]",
  "bg-white",
  "[&_div]:grid [&_div]:grid-cols-[72px_minmax(0,1fr)]",
  "[&_div]:min-w-0 [&_div]:border-b [&_div]:border-b-[#edf0f5]",
  "[&_div:last-child]:border-b-0",
  "[&_span]:min-w-0 [&_span]:px-[12px] [&_span]:py-[8px]",
  "[&_span]:text-[13px] [&_span]:[overflow-wrap:anywhere]",
  "[&_span:first-child]:bg-[#fbfcfe] [&_span:first-child]:font-700",
  "[&_span:first-child]:text-[#667085]",
].join(" ");
const messageStateClassName = [
  "grid h-full min-h-[220px] place-items-center text-center",
  "[&_div]:grid [&_div]:justify-items-center [&_div]:gap-[8px]",
  "[&_svg]:text-[#98a2b3]",
  "[&_strong]:text-[14px] [&_strong]:text-[#20242d]",
  "[&_p]:m-0 [&_p]:max-w-[420px] [&_p]:text-[13px] [&_p]:text-[#667085]",
].join(" ");

interface ClientsState {
  clients: ClientInfo[];
  message?: string;
  phase: "idle" | "loading" | "ready" | "error";
}

interface TerminalSessionsState {
  message?: string;
  phase: "idle" | "loading" | "ready" | "error";
  sessions: TerminalSessionInfo[];
}

export function DaemonTool() {
  const machines = useBunja(machineStoreBunja);
  const rpcSession = useBunja(rpcSessionBunja);
  const tabState = useBunja(workbenchTabBunja);
  const workbench = useBunja(workbenchBunja);
  const daemonInfo = useAtomValue(rpcSession.daemonInfoAtom);
  const daemonServerTimeMs = useAtomValue(rpcSession.daemonServerTimeMsAtom);
  const daemonUptimeSeconds = useAtomValue(
    rpcSession.daemonUptimeSecondsAtom,
  );
  const connection = useAtomValue(rpcSession.connectionAtom);
  const connectionEpoch = useAtomValue(rpcSession.connectionEpochAtom);
  const machine = useAtomValue(machines.selectedAtom);
  const isPaired = useAtomValue(machines.selectedIsPairedAtom);
  const tab = useAtomValue(tabState.tabAtom);
  const connectionLabel = formatDaemonConnectionLabel(connection);
  const [clientsState, setClientsState] = useState<ClientsState>({
    clients: [],
    phase: "idle",
  });
  const [terminalSessionsState, setTerminalSessionsState] = useState<
    TerminalSessionsState
  >({
    phase: "idle",
    sessions: [],
  });
  const clientsRef = useRef<ClientInfo[]>([]);
  const clientDetailId = tab?.daemonClientDetailId;
  const clientsPageOpen = tab?.daemonClientsPageOpen === true ||
    clientDetailId !== undefined;
  const clientDetailIdRef = useRef<string | undefined>(undefined);
  const clientDetail = clientDetailId
    ? clientsState.clients.find((client) => client.clientId === clientDetailId)
    : undefined;

  useEffect(() => {
    clientDetailIdRef.current = clientDetailId;
  }, [clientDetailId]);

  useEffect(() => {
    if (!machine) {
      clientsRef.current = [];
      setClientsState({ clients: [], phase: "idle" });
      tabState.setDaemonClientDetailId(undefined);
      tabState.setDaemonClientsPageOpen(false);
      return;
    }
    if (!isPaired) {
      clientsRef.current = [];
      setClientsState({
        clients: [],
        message: "Pairing required to view clients",
        phase: "error",
      });
      tabState.setDaemonClientDetailId(undefined);
      tabState.setDaemonClientsPageOpen(false);
      return;
    }
    if (daemonInfo.phase !== "ready") {
      clientsRef.current = [];
      setClientsState({ clients: [], phase: "idle" });
      return;
    }

    let cancelled = false;
    const iterator = subscribeClients(
      machine,
      machines.rpcCallOptions(rpcSession.rpcCallOptions()),
    );
    setClientsState((current) => ({ ...current, phase: "loading" }));

    void (async () => {
      try {
        for await (const event of iterator) {
          if (cancelled) return;
          const clients = applyClientsEvent(clientsRef.current, event);
          clientsRef.current = clients;
          setClientsState({ clients, phase: "ready" });
          const currentClientDetailId = clientDetailIdRef.current;
          if (
            currentClientDetailId &&
            !clients.some((client) => client.clientId === currentClientDetailId)
          ) {
            tabState.setDaemonClientDetailId(undefined);
            tabState.setDaemonClientsPageOpen(true);
          }
        }
      } catch (err) {
        if (cancelled) return;
        clientsRef.current = [];
        setClientsState({
          clients: [],
          message: errorMessage(err),
          phase: "error",
        });
      }
    })();

    return () => {
      cancelled = true;
      void iterator.return(undefined);
    };
  }, [
    daemonInfo.phase,
    isPaired,
    machine?.baseUrl,
    machine?.clientId,
    machine?.clientSecret,
    machine?.id,
    connectionEpoch,
  ]);

  useEffect(() => {
    if (!machine || daemonInfo.phase !== "ready") {
      setTerminalSessionsState({ phase: "idle", sessions: [] });
      return;
    }
    if (!isPaired) {
      setTerminalSessionsState({ phase: "idle", sessions: [] });
      return;
    }

    let cancelled = false;
    const iterator = subscribeTerminalSessions(
      machine,
      machines.rpcCallOptions(rpcSession.rpcCallOptions()),
    );
    setTerminalSessionsState((current) => ({
      ...current,
      phase: "loading",
    }));

    void (async () => {
      try {
        for await (const event of iterator) {
          if (cancelled) return;
          setTerminalSessionsState((current) => ({
            phase: "ready",
            sessions: applyTerminalSessionsEvent(current.sessions, event),
          }));
        }
      } catch (err) {
        if (cancelled) return;
        setTerminalSessionsState({
          message: errorMessage(err),
          phase: "error",
          sessions: [],
        });
      }
    })();

    return () => {
      cancelled = true;
      void iterator.return(undefined);
    };
  }, [
    daemonInfo.phase,
    isPaired,
    machine?.baseUrl,
    machine?.clientId,
    machine?.clientSecret,
    machine?.id,
    connectionEpoch,
  ]);

  function openRootPage() {
    tabState.setDaemonClientDetailId(undefined);
    tabState.setDaemonClientsPageOpen(false);
  }

  function openClientsPage() {
    tabState.setDaemonClientDetailId(undefined);
    tabState.setDaemonClientsPageOpen(true);
  }

  function openClientDetail(clientId: string) {
    tabState.setDaemonClientsPageOpen(true);
    tabState.setDaemonClientDetailId(clientId);
  }

  async function renewSelectedClientCredential() {
    if (!machine) return;
    await renewClientCredential(
      machine,
      machines.rpcCallOptions(rpcSession.rpcCallOptions()),
    );
  }

  async function closeSelectedTerminalSession(terminalSessionId: string) {
    if (!machine) return;
    await closeTerminalSession(
      machine,
      terminalSessionId,
      machines.rpcCallOptions(rpcSession.rpcCallOptions()),
    );
  }

  function openTerminalSession(session: TerminalSessionInfo) {
    workbench.openTerminalTab({
      cwd: session.lastKnownCwd,
      launch: session.launch,
      terminalSessionId: session.terminalSessionId,
      terminalTitle: session.lastKnownTitle,
      title: terminalSessionTabTitle(session),
    });
  }

  return (
    <section className={daemonToolClassName}>
      <div className={daemonHeaderClassName}>
        <DaemonBreadcrumb
          clientsPageOpen={clientsPageOpen}
          clientDetailId={clientDetailId}
          onOpenClients={openClientsPage}
          onOpenRoot={openRootPage}
        />
        {clientsPageOpen
          ? null
          : (
            <span className={`${statusPillClassName} ${connection.phase}`}>
              {connection.phase === "reachable"
                ? <CheckCircle2 size={13} />
                : connection.phase === "checking"
                ? <Loader2 size={13} />
                : connection.phase === "offline"
                ? <WifiOff size={13} />
                : <Info size={13} />}
              {connectionLabel}
            </span>
          )}
      </div>
      {daemonInfo.phase === "ready"
        ? (
          <DaemonInfoView
            endpoint={machine?.baseUrl}
            instanceId={daemonInfo.daemonInfo.instanceId}
            os={daemonInfo.daemonInfo.os}
            serverTimeMs={daemonServerTimeMs}
            startedAtMs={daemonInfo.daemonInfo.startedAtMs}
            supportedProcIds={daemonInfo.daemonInfo.supportedProcIds}
            uptimeSeconds={daemonUptimeSeconds}
            version={daemonInfo.daemonInfo.version}
            clientsPageOpen={clientsPageOpen}
            clientsState={clientsState}
            clientDetailId={clientDetailId}
            currentClientId={machine?.clientId}
            terminalSessionsState={terminalSessionsState}
            onOpenClient={openClientDetail}
            onOpenClients={openClientsPage}
            onCloseTerminalSession={closeSelectedTerminalSession}
            onOpenTerminalSession={openTerminalSession}
            onRenewCurrentClientCredential={renewSelectedClientCredential}
          />
        )
        : (
          <DaemonInfoStateView
            hasMachine={machine !== undefined}
            phase={daemonInfo.phase}
            message={daemonInfo.phase === "error"
              ? daemonInfo.message
              : undefined}
          />
        )}
    </section>
  );
}

function formatDaemonConnectionLabel(connection: ConnectionState): string {
  if (connection.phase === "reachable") {
    if (connection.latencyMs === undefined) return "Connected";
    return `Connected - latency ${
      Math.max(1, Math.round(connection.latencyMs))
    } ms`;
  }
  if (connection.phase === "checking") return "Checking connection";
  if (connection.phase === "idle") return "No machine selected";
  return connection.message;
}

interface DaemonBreadcrumbProps {
  clientsPageOpen: boolean;
  clientDetailId?: string;
  onOpenClients: () => void;
  onOpenRoot: () => void;
}

function DaemonBreadcrumb(
  {
    clientsPageOpen,
    clientDetailId,
    onOpenClients,
    onOpenRoot,
  }: DaemonBreadcrumbProps,
) {
  const clientLabel = clientDetailId || "Client detail";
  return (
    <nav className={daemonBreadcrumbClassName} aria-label="Daemon location">
      {clientsPageOpen
        ? (
          <button type="button" onClick={onOpenRoot}>
            Daemon
          </button>
        )
        : <span>Daemon</span>}
      {clientDetailId
        ? (
          <>
            <ChevronRight size={14} />
            <button type="button" onClick={onOpenClients}>
              Clients
            </button>
            <ChevronRight size={14} />
            <span title={clientLabel}>{clientLabel}</span>
          </>
        )
        : clientsPageOpen
        ? (
          <>
            <ChevronRight size={14} />
            <span>Clients</span>
          </>
        )
        : (
          <>
            <ChevronRight size={14} />
            <span className={daemonBreadcrumbMutedClassName}>Overview</span>
          </>
        )}
    </nav>
  );
}

function applyTerminalSessionsEvent(
  current: TerminalSessionInfo[],
  event: TerminalSessionsTableEvent,
): TerminalSessionInfo[] {
  if (event.type === "snapshot") return event.rows;
  const removed = new Set(event.removes.map((row) => row.terminalSessionId));
  const next = current.filter((session) =>
    !removed.has(session.terminalSessionId)
  );
  for (const session of event.upserts) {
    const index = next.findIndex((currentSession) =>
      currentSession.terminalSessionId === session.terminalSessionId
    );
    if (index >= 0) next[index] = session;
    else next.push(session);
  }
  return next;
}

function applyClientsEvent(
  current: ClientInfo[],
  event: ClientsTableEvent,
): ClientInfo[] {
  if (event.type === "snapshot") return event.rows;
  const removed = new Set(event.removes.map((row) => row.clientId));
  const next = current.filter((client) => !removed.has(client.clientId));
  for (const client of event.upserts) {
    const index = next.findIndex((currentClient) =>
      currentClient.clientId === client.clientId
    );
    if (index >= 0) next[index] = client;
    else next.push(client);
  }
  return next;
}

interface DaemonInfoViewProps {
  clientsPageOpen: boolean;
  clientDetailId?: string;
  clientsState: ClientsState;
  currentClientId?: string;
  endpoint?: string;
  instanceId: string;
  onCloseTerminalSession: (terminalSessionId: string) => Promise<void>;
  onOpenClient: (clientId: string) => void;
  onOpenTerminalSession: (session: TerminalSessionInfo) => void;
  os: string;
  onRenewCurrentClientCredential: () => Promise<void>;
  serverTimeMs?: number;
  startedAtMs: number;
  supportedProcIds: number[];
  terminalSessionsState: TerminalSessionsState;
  onOpenClients: () => void;
  uptimeSeconds?: number;
  version: string;
}

function DaemonInfoView(
  {
    clientsPageOpen,
    clientDetailId,
    clientsState,
    currentClientId,
    endpoint,
    instanceId,
    onCloseTerminalSession,
    onOpenClient,
    onOpenTerminalSession,
    onRenewCurrentClientCredential,
    os,
    serverTimeMs,
    startedAtMs,
    supportedProcIds,
    terminalSessionsState,
    uptimeSeconds,
    version,
    onOpenClients,
  }: DaemonInfoViewProps,
) {
  const clientDetail = clientDetailId
    ? clientsState.clients.find((client) => client.clientId === clientDetailId)
    : undefined;
  if (clientDetail) {
    return (
      <ClientDetailPage
        client={clientDetail}
        currentClientId={currentClientId}
        onRenewCurrentClientCredential={onRenewCurrentClientCredential}
        onCloseTerminalSession={onCloseTerminalSession}
        onOpenTerminalSession={onOpenTerminalSession}
        sessions={terminalSessionsState.sessions.filter((session) =>
          session.creatorClientId === clientDetail.clientId
        )}
        state={terminalSessionsState}
      />
    );
  }
  if (clientDetailId && clientsState.phase !== "ready") {
    return (
      <ClientDetailPendingPage
        message={clientDetailPendingMessage(clientsState)}
      />
    );
  }
  if (clientsPageOpen) {
    return (
      <ClientsPage
        clientsState={clientsState}
        currentClientId={currentClientId}
        onOpenClient={onOpenClient}
      />
    );
  }

  return (
    <>
      <dl className={summaryGridClassName}>
        <div className={summaryItemClassName}>
          <dt>Endpoint</dt>
          <dd>
            <code className={infoValueClassName}>{endpoint ?? "Unknown"}</code>
          </dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Daemon version</dt>
          <dd>
            <code className={infoValueClassName}>{version}</code>
          </dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Machine OS</dt>
          <dd>
            <code className={infoValueClassName}>{os}</code>
          </dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Daemon instance ID</dt>
          <dd className={summaryValueWithNoteClassName}>
            <code className={infoValueClassName}>{instanceId}</code>
            <span className={summaryNoteClassName}>
              Changes every time the daemon starts.
            </span>
          </dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Daemon time</dt>
          <dd>
            <code className={infoValueClassName}>
              {formatDaemonTimestamp(serverTimeMs)}
            </code>
          </dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Daemon started</dt>
          <dd className={summaryValueWithNoteClassName}>
            <code className={infoValueClassName}>
              {formatDaemonTimestamp(startedAtMs)}
            </code>
            <span className={summaryNoteClassName}>
              Uptime {formatDaemonUptime(uptimeSeconds)}
            </span>
          </dd>
        </div>
      </dl>

      <ClientsOverviewSection
        clientsState={clientsState}
        currentClientId={currentClientId}
        onOpenClient={onOpenClient}
        onOpenClients={onOpenClients}
      />

      <section className={procSectionClassName}>
        <div className={procSectionTitleClassName}>Supported RPC</div>
        <div className={procTableClassName}>
          {supportedProcIds.map((procId) => (
            <div key={procId}>
              <span>{procId}</span>
              <span>{procNames.get(procId) ?? `Proc ${procId}`}</span>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

interface ClientsOverviewSectionProps {
  clientsState: ClientsState;
  currentClientId?: string;
  onOpenClient: (clientId: string) => void;
  onOpenClients: () => void;
}

function ClientsOverviewSection(
  {
    clientsState,
    currentClientId,
    onOpenClient,
    onOpenClients,
  }: ClientsOverviewSectionProps,
) {
  return (
    <section className={clientsSectionClassName}>
      <button
        type="button"
        className={sectionTitleButtonClassName}
        onClick={onOpenClients}
      >
        Clients
        <ChevronRight size={13} />
      </button>
      <div className={clientOverviewCardClassName}>
        <ClientsOverviewContent
          clientsState={clientsState}
          currentClientId={currentClientId}
          onOpenClient={onOpenClient}
          onOpenClients={onOpenClients}
        />
      </div>
    </section>
  );
}

interface ClientsOverviewContentProps {
  clientsState: ClientsState;
  currentClientId?: string;
  onOpenClient: (clientId: string) => void;
  onOpenClients: () => void;
}

function ClientsOverviewContent(
  {
    clientsState,
    currentClientId,
    onOpenClient,
    onOpenClients,
  }: ClientsOverviewContentProps,
) {
  if (clientsState.phase !== "ready") {
    return <span>{clientsOverviewLabel(clientsState)}</span>;
  }
  const currentClient = currentClientId
    ? clientsState.clients.find((client) => client.clientId === currentClientId)
    : undefined;
  if (!currentClient) {
    return (
      <>
        <span>{clientsOverviewLabel(clientsState)}</span>
        <span className={clientOverviewMetaClassName}>
          This client is not in the daemon client list.
        </span>
      </>
    );
  }
  const otherClientCount = clientsState.clients.length - 1;
  return (
    <>
      <button
        type="button"
        className={clientOverviewCurrentButtonClassName}
        onClick={() => onOpenClient(currentClient.clientId)}
      >
        <div className={clientNameRowClassName}>
          <strong>{currentClient.label || "Unnamed client"}</strong>
          <em className={currentClientBadgeClassName}>This client</em>
        </div>
        <span>{currentClient.clientId}</span>
      </button>
      <button
        type="button"
        className={clientOverviewMetaButtonClassName}
        onClick={onOpenClients}
      >
        {otherClientsLabel(otherClientCount)}
      </button>
    </>
  );
}

interface ClientsPageProps {
  clientsState: ClientsState;
  currentClientId?: string;
  onOpenClient: (clientId: string) => void;
}

function ClientsPage(
  { clientsState, currentClientId, onOpenClient }: ClientsPageProps,
) {
  return (
    <section className={clientDetailPageClassName}>
      <ClientsSection
        clientsState={clientsState}
        currentClientId={currentClientId}
        onOpenClient={onOpenClient}
      />
    </section>
  );
}

interface ClientDetailPendingPageProps {
  message: string;
}

function ClientDetailPendingPage(
  { message }: ClientDetailPendingPageProps,
) {
  return (
    <section className={clientDetailPageClassName}>
      <InlineState message={message} />
    </section>
  );
}

interface ClientsSectionProps {
  clientsState: ClientsState;
  currentClientId?: string;
  onOpenClient: (clientId: string) => void;
}

function ClientsSection(
  {
    clientsState,
    currentClientId,
    onOpenClient,
  }: ClientsSectionProps,
) {
  return (
    <section className={clientsSectionClassName}>
      <div className={procSectionTitleClassName}>Clients</div>
      {clientsState.phase === "error"
        ? <InlineState message={clientsState.message ?? "Client list failed"} />
        : clientsState.phase === "loading"
        ? <InlineState message="Loading clients" />
        : clientsState.clients.length === 0
        ? <InlineState message="No paired clients" />
        : (
          <div className={clientListClassName}>
            {clientsState.clients.map((client) => (
              <ClientButton
                key={client.clientId}
                client={client}
                currentClientId={currentClientId}
                onOpenClient={onOpenClient}
              />
            ))}
          </div>
        )}
    </section>
  );
}

interface ClientButtonProps {
  client: ClientInfo;
  currentClientId?: string;
  onOpenClient: (clientId: string) => void;
}

function ClientButton(
  { client, currentClientId, onOpenClient }: ClientButtonProps,
) {
  const isCurrent = client.clientId === currentClientId;
  return (
    <button
      type="button"
      className={clientButtonClassName}
      onClick={() => onOpenClient(client.clientId)}
    >
      <div className={clientNameRowClassName}>
        <strong>{client.label || "Unnamed client"}</strong>
        {isCurrent
          ? <em className={currentClientBadgeClassName}>This client</em>
          : null}
      </div>
      <span>{client.clientId}</span>
    </button>
  );
}

interface ClientDetailPageProps {
  client: ClientInfo;
  currentClientId?: string;
  onCloseTerminalSession: (terminalSessionId: string) => Promise<void>;
  onOpenTerminalSession: (session: TerminalSessionInfo) => void;
  onRenewCurrentClientCredential: () => Promise<void>;
  sessions: TerminalSessionInfo[];
  state: TerminalSessionsState;
}

function ClientDetailPage(
  {
    client,
    currentClientId,
    onCloseTerminalSession,
    onOpenTerminalSession,
    onRenewCurrentClientCredential,
    sessions,
    state,
  }: ClientDetailPageProps,
) {
  return (
    <section className={clientDetailPageClassName}>
      <ClientInformationSection
        client={client}
        currentClientId={currentClientId}
        onRenewCurrentClientCredential={onRenewCurrentClientCredential}
      />
      <ClientTerminalSessions
        onCloseTerminalSession={onCloseTerminalSession}
        onOpenTerminalSession={onOpenTerminalSession}
        sessions={sessions}
        state={state}
      />
    </section>
  );
}

interface ClientInformationSectionProps {
  client: ClientInfo;
  currentClientId?: string;
  onRenewCurrentClientCredential: () => Promise<void>;
}

function ClientInformationSection(
  {
    client,
    currentClientId,
    onRenewCurrentClientCredential,
  }: ClientInformationSectionProps,
) {
  const now = useBunja(nowBunja);
  const nowMs = useAtomValue(now.nowEverySecondAtom);
  const isCurrent = client.clientId === currentClientId;
  const [renewPhase, setRenewPhase] = useState<
    "idle" | "renewing" | "renewed" | "error"
  >("idle");
  const [renewError, setRenewError] = useState<string>();

  async function renewNow() {
    setRenewPhase("renewing");
    setRenewError(undefined);
    try {
      await onRenewCurrentClientCredential();
      setRenewPhase("renewed");
    } catch (err) {
      setRenewError(errorMessage(err));
      setRenewPhase("error");
    }
  }

  return (
    <section className={clientDetailSectionClassName}>
      <header className={clientDetailSectionHeaderClassName}>
        <h2>Client information</h2>
      </header>
      <dl className={clientInfoGridClassName}>
        <div className={clientInfoItemClassName}>
          <dt>Label</dt>
          <dd>
            <code className={infoValueClassName}>
              {client.label || "Unnamed client"}
            </code>
          </dd>
        </div>
        <div className={clientInfoItemClassName}>
          <dt>Client ID</dt>
          <dd className={clientIdValueClassName}>
            <code className={infoValueClassName}>{client.clientId}</code>
            {isCurrent
              ? (
                <em className={clientInfoCurrentClientBadgeClassName}>
                  This client
                </em>
              )
              : null}
          </dd>
        </div>
        <div className={clientInfoItemClassName}>
          <dt>Credential created</dt>
          <dd className={credentialCreatedValueClassName}>
            <code className={infoValueClassName}>
              {formatUnixTimestamp(client.createdAtUnix)}
            </code>
            <span className={credentialCreatedAgeClassName}>
              {formatCredentialCreatedAge(client.createdAtUnix, nowMs)}
            </span>
          </dd>
        </div>
        <div className={clientInfoItemClassName}>
          <dt>Credential expires</dt>
          <dd className={credentialExpiryValueClassName}>
            <code className={infoValueClassName}>
              {formatUnixTimestamp(client.expiresAtUnix)}
            </code>
            <span className={credentialExpiryRemainingClassName}>
              {formatCredentialExpiryRemaining(client.expiresAtUnix, nowMs)}
            </span>
            {isCurrent
              ? (
                <>
                  <Button
                    className={renewCredentialButtonClassName}
                    disabled={renewPhase === "renewing"}
                    onClick={renewNow}
                  >
                    {renewPhase === "renewing" ? "Renewing" : "Renew now"}
                  </Button>
                  {renewPhase === "renewed"
                    ? (
                      <span className={renewCredentialMessageClassName}>
                        Renewal requested
                      </span>
                    )
                    : null}
                  {renewPhase === "error"
                    ? (
                      <span className={renewCredentialErrorClassName}>
                        {renewError ?? "Renewal failed"}
                      </span>
                    )
                    : null}
                </>
              )
              : null}
          </dd>
        </div>
      </dl>
    </section>
  );
}

interface ClientTerminalSessionsProps {
  onCloseTerminalSession: (terminalSessionId: string) => Promise<void>;
  onOpenTerminalSession: (session: TerminalSessionInfo) => void;
  sessions: TerminalSessionInfo[];
  state: TerminalSessionsState;
}

function ClientTerminalSessions(
  {
    onCloseTerminalSession,
    onOpenTerminalSession,
    sessions,
    state,
  }: ClientTerminalSessionsProps,
) {
  const [closingSessionIds, setClosingSessionIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [closeError, setCloseError] = useState<string>();

  async function closeSession(terminalSessionId: string) {
    setCloseError(undefined);
    setClosingSessionIds((current) => new Set(current).add(terminalSessionId));
    try {
      await onCloseTerminalSession(terminalSessionId);
    } catch (err) {
      setCloseError(errorMessage(err));
    } finally {
      setClosingSessionIds((current) => {
        const next = new Set(current);
        next.delete(terminalSessionId);
        return next;
      });
    }
  }

  return (
    <section className={clientDetailSectionClassName}>
      <header className={clientDetailSectionHeaderClassName}>
        <h2>Terminal sessions</h2>
      </header>
      {closeError ? <ClientDetailState message={closeError} /> : null}
      {state.phase === "error"
        ? (
          <ClientDetailState
            message={state.message ?? "Terminal session list failed"}
          />
        )
        : state.phase === "loading"
        ? <ClientDetailState message="Loading terminal sessions" />
        : sessions.length === 0
        ? <ClientDetailState message="No terminal sessions for this client" />
        : (
          <div className={terminalSessionListClassName}>
            {sessions.map((session) => (
              <article key={session.terminalSessionId}>
                <strong>{commandName(session.launch.command)}</strong>
                <span>{session.cols} x {session.rows}</span>
                <div className={terminalSessionActionsClassName}>
                  <Button
                    className={terminalSessionCloseButtonClassName}
                    onClick={() => onOpenTerminalSession(session)}
                  >
                    <SquareTerminal size={13} />
                    Open
                  </Button>
                  <Button
                    className={terminalSessionCloseButtonClassName}
                    disabled={closingSessionIds.has(session.terminalSessionId)}
                    onClick={() => closeSession(session.terminalSessionId)}
                  >
                    {closingSessionIds.has(session.terminalSessionId)
                      ? "Closing"
                      : "Close"}
                  </Button>
                </div>
                <small>
                  {terminalSessionStatus(session)}
                  {" - "}
                  {session.lastKnownCwd ?? "cwd unknown"}
                </small>
              </article>
            ))}
          </div>
        )}
    </section>
  );
}

interface ClientDetailStateProps {
  message: string;
}

function ClientDetailState({ message }: ClientDetailStateProps) {
  return <div className={clientDetailStateClassName}>{message}</div>;
}

function terminalSessionStatus(session: TerminalSessionInfo): string {
  if (session.exit?.code !== undefined) {
    return `Exited with code ${session.exit.code}`;
  }
  if (session.exit?.signal) return `Exited after ${session.exit.signal}`;
  if (session.exit) return "Exited";
  return "Running";
}

function terminalSessionTabTitle(session: TerminalSessionInfo): string {
  return session.lastKnownTitle ?? commandName(session.launch.command);
}

interface InlineStateProps {
  message: string;
}

function InlineState({ message }: InlineStateProps) {
  return (
    <div className="min-w-0 rounded-[8px] border border-[#d8dde7] bg-[#fbfcfe] px-[12px] py-[10px] text-[13px] text-[#667085]">
      {message}
    </div>
  );
}

function formatDaemonTimestamp(timeMs?: number): string {
  if (timeMs === undefined || timeMs === 0) return "Unknown";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(timeMs));
}

function formatDaemonUptime(uptimeSeconds?: number): string {
  if (uptimeSeconds === undefined) return "Unknown";
  const days = Math.floor(uptimeSeconds / 86400);
  const hours = Math.floor((uptimeSeconds % 86400) / 3600);
  const minutes = Math.floor((uptimeSeconds % 3600) / 60);
  const seconds = uptimeSeconds % 60;
  if (days > 0) return `${days}d ${hours}h ${minutes}m ${seconds}s`;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

function formatUnixTimestamp(timeUnix?: number): string {
  if (timeUnix === undefined || timeUnix === 0) return "Unknown";
  return formatDaemonTimestamp(timeUnix * 1000);
}

function formatCredentialExpiryRemaining(
  expiresAtUnix: number | undefined,
  nowMs: number,
): string {
  if (expiresAtUnix === undefined || expiresAtUnix === 0) {
    return "Expiry unknown";
  }
  const remainingSeconds = Math.floor(expiresAtUnix - nowMs / 1000);
  if (remainingSeconds > 0) {
    return `${formatDurationSeconds(remainingSeconds)} remaining`;
  }
  if (remainingSeconds === 0) return "Expires now";
  return `Expired ${formatDurationSeconds(-remainingSeconds)} ago`;
}

function formatCredentialCreatedAge(
  createdAtUnix: number | undefined,
  nowMs: number,
): string {
  if (createdAtUnix === undefined || createdAtUnix === 0) {
    return "Creation time unknown";
  }
  const elapsedSeconds = Math.floor(nowMs / 1000 - createdAtUnix);
  if (elapsedSeconds > 0) {
    return `${formatDurationSeconds(elapsedSeconds)} ago`;
  }
  if (elapsedSeconds === 0) return "Now";
  return `In ${formatDurationSeconds(-elapsedSeconds)}`;
}

function formatDurationSeconds(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const restSeconds = seconds % 60;
  if (days > 0) return `${days}d ${hours}h ${minutes}m ${restSeconds}s`;
  if (hours > 0) return `${hours}h ${minutes}m ${restSeconds}s`;
  if (minutes > 0) return `${minutes}m ${restSeconds}s`;
  return `${restSeconds}s`;
}

function commandName(command: string): string {
  const normalized = command.replaceAll("\\", "/");
  return normalized.split("/").filter(Boolean).pop() ?? command;
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function clientDetailPendingMessage(clientsState: ClientsState): string {
  if (clientsState.phase === "error") {
    return clientsState.message ?? "Client detail failed";
  }
  return "Loading client detail";
}

function clientsOverviewLabel(clientsState: ClientsState): string {
  if (clientsState.phase === "error") {
    return clientsState.message ?? "Client list failed";
  }
  if (clientsState.phase === "loading") return "Loading clients";
  const count = clientsState.clients.length;
  if (count === 0) return "No paired clients";
  if (count === 1) return "1 paired client";
  return `${count} paired clients`;
}

function otherClientsLabel(count: number): string {
  if (count === 0) return "No other clients...";
  if (count === 1) return "1 other client...";
  return `${count} other clients...`;
}

interface DaemonInfoStateViewProps {
  hasMachine: boolean;
  message?: string;
  phase: "idle" | "loading" | "error";
}

function DaemonInfoStateView(
  { hasMachine, message, phase }: DaemonInfoStateViewProps,
) {
  const Icon = phase === "error"
    ? AlertTriangle
    : phase === "loading"
    ? Loader2
    : Info;
  const title = phase === "error"
    ? "Daemon info unavailable"
    : phase === "loading"
    ? "Loading daemon info"
    : hasMachine
    ? "Daemon info unavailable"
    : "No daemon selected";

  return (
    <div className={messageStateClassName}>
      <div>
        <Icon size={24} />
        <strong>{title}</strong>
        {message ? <p>{message}</p> : null}
      </div>
    </div>
  );
}
