import { lazy, Suspense, useContext, useEffect, useRef, useState } from "react";
import { useBunja } from "bunja/react";
import { useAtomValue } from "jotai";
import {
  type FsEntry,
  readFile,
  writeFile,
  WriteFileMode,
} from "../../../../../../../../protocol/rpc.ts";
import { workbenchTabBunja } from "../../../../../../../../state/workbench.ts";
import {
  FilesActionsContext,
  requireFilesActions,
} from "../../../../context.tsx";
import { BigFileWarning } from "../../big-file-warning.tsx";
import {
  fileViewerBunja,
  FsEntryContext,
  requireFsEntry,
} from "../../state.tsx";

const MonacoTextViewer = lazy(async () => {
  const module = await import("./monaco-text-viewer.tsx");
  return { default: module.MonacoTextViewer };
});

const inlineOpenLimitBytes = 1024 * 1024;
const fileViewerStatusClassName = [
  "flex items-center justify-center gap-[8px] min-w-0 min-h-0",
  "text-[#667085] text-[13px]",
  "[&.error]:items-start [&.error]:justify-start [&.error]:overflow-auto",
  "[&.error]:text-[#b42318] [&.error]:p-[14px]",
].join(" ");

type FileReadState =
  | { phase: "loading" }
  | {
    draftText: string;
    phase: "ready";
    saveError?: string;
    saving: boolean;
  }
  | { phase: "error"; message: string };

const textViewerName = "text viewer";

export default function TextFileViewer() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const viewer = useBunja(fileViewerBunja);
  const tabState = useBunja(workbenchTabBunja);
  const fsEntry = requireFsEntry(useContext(FsEntryContext));
  const viewerState = useAtomValue(viewer.stateAtom);
  const machine = useAtomValue(viewer.machineAtom);
  const dirty = useAtomValue(tabState.dirtyAtom);
  const webTransport = viewer.webTransport;
  const requiresConfirmation = fsEntry.size === undefined ||
    fsEntry.size > inlineOpenLimitBytes;
  const [confirmedFsEntryPath, setConfirmedFsEntryPath] = useState<
    string | undefined
  >();
  const confirmed = !requiresConfirmation ||
    confirmedFsEntryPath === fsEntry.path;
  const fileVersionKey = textFileVersionKey(
    fsEntry.path,
    fsEntry.size,
    fsEntry.modifiedAtMs,
  );
  const [state, setState] = useState<FileReadState>({ phase: "loading" });
  const loadedFileVersionKeyRef = useRef<string | undefined>(undefined);
  const ownSavedFileVersionRef = useRef<
    { modifiedAtMs?: number; path: string; size: number } | undefined
  >(undefined);
  const savedTextRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (!confirmed || !machine || viewerState.phase !== "ready") return;
    if (dirty) return;
    if (loadedFileVersionKeyRef.current === fileVersionKey) return;
    if (
      ownSavedFileVersionRef.current &&
      isOwnSavedFileVersion(ownSavedFileVersionRef.current, fsEntry)
    ) {
      loadedFileVersionKeyRef.current = fileVersionKey;
      ownSavedFileVersionRef.current = undefined;
      return;
    }

    let cancelled = false;
    savedTextRef.current = undefined;
    tabState.setDirty(false);
    setState({ phase: "loading" });
    void (async () => {
      try {
        const bytes = await readFile(await webTransport(), fsEntry.path);
        if (cancelled) return;
        const text = decodeTextFile(bytes);
        loadedFileVersionKeyRef.current = fileVersionKey;
        savedTextRef.current = text;
        setState({
          draftText: text,
          phase: "ready",
          saving: false,
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
  }, [
    confirmed,
    dirty,
    fileVersionKey,
    fsEntry.path,
    machine,
    tabState,
    webTransport,
    viewerState,
  ]);

  function changeDraftText(nextText: string) {
    tabState.setDirty(
      savedTextRef.current !== undefined && nextText !== savedTextRef.current,
    );
    setState((current) => {
      if (current.phase !== "ready") return current;
      return {
        ...current,
        draftText: nextText,
        saveError: undefined,
      };
    });
  }

  function saveDraftText(nextText: string) {
    setState((current) =>
      current.phase === "ready"
        ? {
          ...current,
          draftText: nextText,
          saveError: undefined,
          saving: true,
        }
        : current
    );

    void (async () => {
      try {
        const bytes = new TextEncoder().encode(nextText);
        const result = await writeFile(
          await webTransport(),
          fsEntry.path,
          WriteFileMode.Replace,
          bytes,
          { expectedResultSize: bytes.byteLength },
        );
        loadedFileVersionKeyRef.current = fileVersionKey;
        ownSavedFileVersionRef.current = {
          modifiedAtMs: result.modifiedAtMs,
          path: fsEntry.path,
          size: result.resultSize,
        };
        savedTextRef.current = nextText;
        setState((current) =>
          current.phase === "ready"
            ? {
              ...current,
              draftText: nextText,
              saveError: undefined,
              saving: false,
            }
            : current
        );
        tabState.setDirty(false);
      } catch (err) {
        setState((current) =>
          current.phase === "ready"
            ? {
              ...current,
              saveError: err instanceof Error ? err.message : String(err),
              saving: false,
            }
            : current
        );
      }
    })();
  }

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
        viewerName={textViewerName}
      />
    );
  }

  if (state.phase === "loading") {
    return (
      <div className={fileViewerStatusClassName}>
        <span>Loading text</span>
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

  return (
    <Suspense
      fallback={
        <div className={fileViewerStatusClassName}>
          <span>Loading editor</span>
        </div>
      }
    >
      <div className="relative w-full h-full min-w-0 min-h-0">
        <MonacoTextViewer
          key={fsEntry.path}
          path={fsEntry.path}
          text={state.draftText}
          onChange={changeDraftText}
          onSave={saveDraftText}
        />
        {state.saving || state.saveError
          ? (
            <div className="absolute right-[8px] bottom-[8px] z-[2] rounded-[4px] border border-[#d8dde7] bg-white px-[8px] py-[4px] text-[#344054] shadow-[0_8px_24px_rgb(16_24_40_/_12%)]">
              {state.saving ? "Saving" : state.saveError}
            </div>
          )
          : null}
      </div>
    </Suspense>
  );
}

function decodeTextFile(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

function textFileVersionKey(
  path: string,
  size: number | undefined,
  modifiedAtMs: number | undefined,
): string {
  return [
    path,
    size ?? "",
    modifiedAtMs ?? "",
  ].join("\n");
}

function isOwnSavedFileVersion(
  saved: { modifiedAtMs?: number; path: string; size: number },
  entry: FsEntry,
): boolean {
  if (entry.path !== saved.path || entry.size !== saved.size) return false;
  return saved.modifiedAtMs === undefined ||
    entry.modifiedAtMs === saved.modifiedAtMs;
}
