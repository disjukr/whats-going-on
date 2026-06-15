import { useContext, useEffect, useState } from "react";
import { useBunja } from "bunja/react";
import { useAtomValue } from "jotai";
import { readFile } from "../../../../../../../../protocol/rpc.ts";
import type { FsEntry } from "../../../../../../../../protocol/rpc.ts";
import { formatSize } from "../../../../../../../../state/explorer.ts";
import {
  FilesActionsContext,
  requireFilesActions,
} from "../../../../context.tsx";
import { BigFileWarning } from "../../big-file-warning.tsx";
import { fileViewerBunja } from "../../state.tsx";

const inlineOpenLimitBytes = 1024 * 1024;
const fileViewerStatusClassName = [
  "flex items-center justify-center gap-[8px] min-w-0 min-h-0",
  "text-[#667085] text-[13px]",
  "[&.error]:items-start [&.error]:justify-start [&.error]:overflow-auto",
  "[&.error]:text-[#b42318] [&.error]:p-[14px]",
].join(" ");
const fileContentClassName = [
  "min-w-0 min-h-0 m-0 overflow-auto bg-white text-[#20242d]",
  "font-mono text-[12px] leading-[1.55] p-[14px]",
  "[tab-size:2] whitespace-pre [overflow-wrap:normal]",
].join(" ");

type FileReadState =
  | { phase: "loading" }
  | { phase: "ready"; text: string }
  | { phase: "error"; message: string };

const hexViewerName = "hex viewer";

export default function HexFileViewer() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const viewer = useBunja(fileViewerBunja);
  const fsEntry = viewer.fsEntry;
  const viewerState = useAtomValue(viewer.stateAtom);
  const machine = useAtomValue(viewer.machineAtom);
  const rpcCallOptions = viewer.rpcCallOptions;
  const requiresConfirmation = fsEntry.size === undefined ||
    fsEntry.size > inlineOpenLimitBytes;
  const [confirmedFsEntryPath, setConfirmedFsEntryPath] = useState<
    string | undefined
  >();
  const confirmed = !requiresConfirmation ||
    confirmedFsEntryPath === fsEntry.path;
  const [state, setState] = useState<FileReadState>({ phase: "loading" });

  useEffect(() => {
    if (!confirmed || !machine || viewerState.phase !== "ready") return;

    let cancelled = false;
    setState({ phase: "loading" });
    void (async () => {
      try {
        const bytes = hasCompleteInitialBytes(fsEntry, viewerState.initialBytes)
          ? viewerState.initialBytes
          : await readFile(machine, fsEntry.path, {}, rpcCallOptions());
        if (cancelled) return;
        setState({
          phase: "ready",
          text: decodeHexFilePreview(bytes),
        });
      } catch (err) {
        if (!cancelled) {
          setState({
            phase: "error",
            message: err instanceof Error ? err.message : String(err),
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [confirmed, fsEntry, fsEntry.path, machine, rpcCallOptions, viewerState]);

  if (!machine) {
    return (
      <div className={`${fileViewerStatusClassName} error`}>
        <span>No machine selected</span>
      </div>
    );
  }

  if (viewerState.phase !== "ready") return null;

  if (!confirmed) {
    return (
      <BigFileWarning
        onCancel={actions.goBack}
        onConfirm={() => setConfirmedFsEntryPath(fsEntry.path)}
        viewerName={hexViewerName}
      />
    );
  }

  if (state.phase === "loading") {
    return (
      <div className={fileViewerStatusClassName}>
        <span>Loading bytes</span>
      </div>
    );
  }

  if (state.phase === "error") {
    return (
      <div className={`${fileViewerStatusClassName} error`}>
        <span>{state.message}</span>
      </div>
    );
  }

  return <pre className={fileContentClassName}>{state.text}</pre>;
}

function hasCompleteInitialBytes(
  fsEntry: FsEntry,
  initialBytes: Uint8Array,
): boolean {
  return fsEntry.size !== undefined && fsEntry.size <= initialBytes.byteLength;
}

function decodeHexFilePreview(bytes: Uint8Array): string {
  const previewLength = Math.min(bytes.length, 4096);
  const lines: string[] = [];
  for (let offset = 0; offset < previewLength; offset += 16) {
    const chunk = bytes.subarray(offset, Math.min(offset + 16, previewLength));
    const hex = Array.from(chunk)
      .map((byte) => byte.toString(16).padStart(2, "0"))
      .join(" ")
      .padEnd(47, " ");
    const ascii = Array.from(chunk)
      .map((byte) =>
        byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : "."
      )
      .join("");
    lines.push(`${offset.toString(16).padStart(8, "0")}  ${hex}  |${ascii}|`);
  }
  if (bytes.length > previewLength) {
    lines.push(`... ${formatSize(bytes.length - previewLength)} more`);
  }
  return lines.join("\n");
}
