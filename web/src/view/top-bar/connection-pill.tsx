import { RefreshCw } from "lucide-react";
import type { Machine } from "../../state/machines.ts";
import type { ConnectionState } from "../../state/types.ts";
import { className } from "../class-name.ts";

const connectionPillClassName = [
  "inline-flex appearance-none justify-self-end items-center gap-[4px] h-[2em] min-h-[2em]",
  "border border-[#444b5c] rounded-full bg-transparent text-[#cbd3df]",
  "box-border cursor-pointer px-[6px] font-700 leading-[1.6] whitespace-nowrap [font-family:inherit]",
  "hover:border-[#566074] hover:bg-[#343946] hover:text-white",
  "focus-visible:border-[#566074] focus-visible:bg-[#343946] focus-visible:text-white",
  "active:translate-y-[1px]",
  "[&.connected_.connection-dot]:bg-[#22c55e]",
  "[&:hover_.connection-dot]:opacity-0",
  "[&:focus-visible_.connection-dot]:opacity-0",
  "[&:hover_.connection-refresh]:opacity-100",
  "[&:focus-visible_.connection-refresh]:opacity-100",
].join(" ");
const statusIconClassName =
  "relative inline-flex items-center justify-center w-[1em] h-[1em]";
const connectionDotClassName = [
  "connection-dot absolute w-[7px] h-[7px] rounded-full bg-[#f04438]",
  "[transition:opacity_0.12s_ease]",
].join(" ");
const connectionRefreshClassName = [
  "connection-refresh absolute opacity-0 [transition:opacity_0.12s_ease]",
].join(" ");
const connectionLabelClassName = "leading-[1.6]";

interface ConnectionPillProps {
  machine?: Machine;
  connection: ConnectionState;
  onRefresh: () => void;
}

export function ConnectionPill(
  { machine, connection, onRefresh }: ConnectionPillProps,
) {
  if (!machine) return null;

  const connected = connection.phase === "reachable";
  const label = connected ? "Connected" : "Unconnected";
  const buttonClassName = className(
    connectionPillClassName,
    connected && "connected",
  );

  return (
    <button
      type="button"
      className={buttonClassName}
      onClick={onRefresh}
      aria-label={`Connection status: ${label}`}
    >
      <span className={statusIconClassName} aria-hidden="true">
        <span className={connectionDotClassName} />
        <RefreshCw
          size={12}
          className={connectionRefreshClassName}
        />
      </span>
      <span className={connectionLabelClassName}>{label}</span>
    </button>
  );
}
