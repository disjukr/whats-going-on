import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import {
  AlertTriangle,
  CheckCircle2,
  Info,
  Loader2,
  WifiOff,
} from "lucide-react";
import { daemonInfoBunja } from "../../../../state/daemon-info.ts";
import { connectionBunja } from "../../../../state/connection.ts";
import { machineStoreBunja } from "../../../../state/machine-store.ts";
import type { ConnectionState } from "../../../../state/types.ts";

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
]);

const daemonToolClassName = [
  "h-full min-h-0 overflow-auto bg-white px-[18px] py-[16px]",
  "text-[#20242d]",
].join(" ");
const statusPillClassName = [
  "inline-flex items-center gap-[6px] min-w-0 rounded-[999px]",
  "bg-[#eef3fb] px-[9px] py-[4px] text-[12px] font-700 text-[#344054]",
  "[&_svg]:flex-[0_0_auto]",
  "[&.reachable]:bg-[#ecfdf3] [&.reachable]:text-[#027a48]",
  "[&.checking]:bg-[#fff7e6] [&.checking]:text-[#945800]",
  "[&.offline]:bg-[#fff1f3] [&.offline]:text-[#b42318]",
].join(" ");
const daemonStatusRowClassName = "mb-[14px] flex justify-end min-w-0";
const summaryGridClassName = [
  "grid grid-cols-[repeat(3,minmax(0,1fr))] gap-[1px] overflow-hidden",
  "border border-[#d8dde7] rounded-[8px] bg-[#d8dde7]",
  "[@container_workbench-tab-page_(max-width:760px)]:grid-cols-1",
].join(" ");
const summaryItemClassName = [
  "grid min-w-0 gap-[6px] bg-[#fbfcfe] px-[14px] py-[12px]",
  "[&_dt]:m-0 [&_dt]:text-[12px] [&_dt]:font-700 [&_dt]:text-[#667085]",
  "[&_dd]:m-0 [&_dd]:min-w-0 [&_dd]:text-[14px] [&_dd]:font-700",
  "[&_dd]:text-[#20242d] [&_dd]:[overflow-wrap:anywhere]",
].join(" ");
const procSectionClassName = "mt-[18px] min-w-0";
const procSectionTitleClassName =
  "mb-[8px] text-[12px] font-800 uppercase text-[#667085]";
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

export function DaemonTool() {
  const daemonInfoState = useBunja(daemonInfoBunja);
  const connectionState = useBunja(connectionBunja);
  const machines = useBunja(machineStoreBunja);
  const daemonInfo = useAtomValue(daemonInfoState.daemonInfoAtom);
  const connection = useAtomValue(connectionState.connectionAtom);
  const machine = useAtomValue(machines.selectedAtom);
  const connectionLabel = formatDaemonConnectionLabel(connection);

  return (
    <section className={daemonToolClassName}>
      <div className={daemonStatusRowClassName}>
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
      </div>
      {daemonInfo.phase === "ready"
        ? (
          <DaemonInfoView
            endpoint={machine?.baseUrl}
            os={daemonInfo.daemonInfo.os}
            supportedProcIds={daemonInfo.daemonInfo.supportedProcIds}
            version={daemonInfo.daemonInfo.version}
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
    return `Connected · latency ${
      Math.max(1, Math.round(connection.latencyMs))
    } ms`;
  }
  if (connection.phase === "checking") return "Checking connection";
  if (connection.phase === "idle") return "No machine selected";
  return connection.message;
}

interface DaemonInfoViewProps {
  endpoint?: string;
  os: string;
  supportedProcIds: number[];
  version: string;
}

function DaemonInfoView(
  { endpoint, os, supportedProcIds, version }: DaemonInfoViewProps,
) {
  return (
    <>
      <dl className={summaryGridClassName}>
        <div className={summaryItemClassName}>
          <dt>Endpoint</dt>
          <dd>{endpoint ?? "Unknown"}</dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Daemon version</dt>
          <dd>{version}</dd>
        </div>
        <div className={summaryItemClassName}>
          <dt>Machine OS</dt>
          <dd>{os}</dd>
        </div>
      </dl>

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
