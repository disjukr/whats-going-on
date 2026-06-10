import { useEffect, useState } from "react";
import { FileText, Loader2 } from "lucide-react";
import { FsEntry, readFile } from "../../../../../../protocol/rpc.ts";
import { displayName, formatSize } from "../../../../../../state/explorer.ts";
import type { Machine } from "../../../../../../state/machines.ts";
import { decodeFilePreview } from "./file-preview.ts";
import { HexViewerContent } from "./hex/index.tsx";
import { TextViewerContent } from "./text/index.tsx";
import type { FileLoadState } from "./types.ts";

interface FileViewerProps {
  machine: Machine;
  file: FsEntry;
}

export function FileViewer({ machine, file }: FileViewerProps) {
  const [state, setState] = useState<FileLoadState>({ phase: "loading" });

  useEffect(() => {
    let cancelled = false;
    setState({ phase: "loading" });
    void (async () => {
      try {
        const bytes = await readFile(machine, file.path);
        if (cancelled) return;
        setState({
          phase: "ready",
          byteLength: bytes.byteLength,
          preview: decodeFilePreview(bytes),
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
  }, [file.path, machine]);

  return (
    <section className="file-viewer">
      <header className="file-viewer-head">
        <div className="file-viewer-title">
          <FileText size={16} />
          <span>{displayName(file)}</span>
        </div>
        <span className="file-viewer-meta">
          {state.phase === "ready"
            ? formatSize(state.byteLength)
            : formatSize(file.size)}
        </span>
      </header>
      {state.phase === "loading"
        ? (
          <div className="file-viewer-status">
            <Loader2 size={18} className="spin" />
            <span>Loading file</span>
          </div>
        )
        : state.phase === "error"
        ? (
          <div className="file-viewer-status error">
            <span>{state.message}</span>
          </div>
        )
        : (
          state.preview.kind === "binary"
            ? <HexViewerContent text={state.preview.text} />
            : <TextViewerContent text={state.preview.text} />
        )}
    </section>
  );
}
