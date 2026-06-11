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
import type { FileViewerImpl } from "../index.ts";

const inlineOpenLimitBytes = 1024 * 1024;

type FileReadState =
  | { phase: "loading" }
  | { phase: "ready"; text: string }
  | { phase: "error"; message: string };

export function HexFileViewer() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const viewer = useBunja(fileViewerBunja);
  const fsEntry = viewer.fsEntry;
  const viewerState = useAtomValue(viewer.stateAtom);
  const machine = useAtomValue(viewer.machineAtom);
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
          : await readFile(machine, fsEntry.path);
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
  }, [confirmed, fsEntry, fsEntry.path, machine, viewerState]);

  if (!machine) {
    return (
      <div className="file-viewer-status error">
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
        viewerName={hexFileViewerImpl.viewerName}
      />
    );
  }

  if (state.phase === "loading") {
    return (
      <div className="file-viewer-status">
        <span>Loading bytes</span>
      </div>
    );
  }

  if (state.phase === "error") {
    return (
      <div className="file-viewer-status error">
        <span>{state.message}</span>
      </div>
    );
  }

  return <pre className="file-content binary">{state.text}</pre>;
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

export const hexFileViewerImpl = {
  id: "hex",
  label: "Hex",
  viewerName: "hex viewer",
  Component: HexFileViewer,
} as const satisfies FileViewerImpl;
