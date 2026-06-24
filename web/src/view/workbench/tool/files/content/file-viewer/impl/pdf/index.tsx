import { useContext, useEffect, useRef, useState } from "react";
import * as pdfjs from "pdfjs-dist";
import { useBunja } from "bunja/react";
import { useAtomValue } from "jotai";
import { readFile } from "../../../../../../../../protocol/rpc.ts";
import type { FsEntry } from "../../../../../../../../protocol/rpc.ts";
import {
  FilesActionsContext,
  requireFilesActions,
} from "../../../../context.tsx";
import { BigFileWarning } from "../../big-file-warning.tsx";
import { fileViewerBunja } from "../../state.tsx";

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "../../../../../../../../../node_modules/pdfjs-dist/build/pdf.worker.mjs",
  import.meta.url,
).toString();

const inlineOpenLimitBytes = 1024 * 1024;
const pageCssScale = 1.25;
const maxRenderPixelRatio = 2;
const fileViewerStatusClassName = [
  "flex items-center justify-center gap-[8px] min-w-0 min-h-0",
  "text-[#667085] text-[13px]",
  "[&.error]:items-start [&.error]:justify-start [&.error]:overflow-auto",
  "[&.error]:text-[#b42318] [&.error]:p-[14px]",
].join(" ");
const pdfViewerClassName = [
  "grid [grid-template-rows:minmax(0,1fr)] min-w-0 min-h-0 overflow-hidden",
  "bg-[#eef1f5]",
].join(" ");
const pdfPagesClassName = [
  "min-w-0 min-h-0 overflow-y-scroll overflow-x-auto px-[18px] py-[18px]",
  "[scrollbar-gutter:stable]",
  "[&_canvas]:block [&_canvas]:h-auto [&_canvas]:max-w-full",
].join(" ");
const pdfPageShellClassName = [
  "mx-auto mb-[18px] flex max-w-full justify-center",
  "last:mb-0",
].join(" ");
const pdfPageCanvasClassName = [
  "bg-white [box-shadow:0_2px_14px_rgb(32_36_45_/_18%)]",
].join(" ");

type PdfReadState =
  | { phase: "loading" }
  | { phase: "ready"; bytes: Uint8Array }
  | { phase: "error"; message: string };

type PdfRenderState =
  | { phase: "idle" }
  | { phase: "rendering"; pageCount?: number; renderedPages: number }
  | { phase: "ready"; pageCount: number }
  | { phase: "error"; message: string };

const pdfViewerName = "pdf viewer";

export default function PdfFileViewer() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const viewer = useBunja(fileViewerBunja);
  const fsEntry = viewer.fsEntry;
  const viewerState = useAtomValue(viewer.stateAtom);
  const machine = useAtomValue(viewer.machineAtom);
  const webTransport = viewer.webTransport;
  const requiresConfirmation = fsEntry.size === undefined ||
    fsEntry.size > inlineOpenLimitBytes;
  const [confirmedFsEntryPath, setConfirmedFsEntryPath] = useState<
    string | undefined
  >();
  const confirmed = !requiresConfirmation ||
    confirmedFsEntryPath === fsEntry.path;
  const [state, setState] = useState<PdfReadState>({ phase: "loading" });

  useEffect(() => {
    if (!confirmed || !machine || viewerState.phase !== "ready") return;

    let cancelled = false;
    setState({ phase: "loading" });
    void (async () => {
      try {
        const bytes = hasCompleteInitialBytes(fsEntry, viewerState.initialBytes)
          ? viewerState.initialBytes
          : await readFile(await webTransport(), fsEntry.path);
        if (cancelled) return;
        setState({ phase: "ready", bytes });
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
  }, [confirmed, fsEntry, fsEntry.path, machine, webTransport, viewerState]);

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
        viewerName={pdfViewerName}
      />
    );
  }

  if (state.phase === "loading") {
    return (
      <div className={fileViewerStatusClassName}>
        <span>Loading PDF</span>
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

  return <PdfCanvasDocument bytes={state.bytes} />;
}

function PdfCanvasDocument({ bytes }: { bytes: Uint8Array }) {
  const pagesRef = useRef<HTMLDivElement | null>(null);
  const [state, setState] = useState<PdfRenderState>({ phase: "idle" });

  useEffect(() => {
    let cancelled = false;
    let renderTask:
      | { cancel: () => void; promise: Promise<unknown> }
      | undefined;
    const loadingTask = pdfjs.getDocument({ data: bytes.slice() });

    pagesRef.current?.replaceChildren();
    setState({ phase: "rendering", renderedPages: 0 });
    void (async () => {
      try {
        const pdfDocument = await loadingTask.promise;
        if (cancelled) {
          await pdfDocument.destroy();
          return;
        }
        const pageCount = pdfDocument.numPages;
        setState({
          phase: "rendering",
          pageCount,
          renderedPages: 0,
        });
        for (let pageNumber = 1; pageNumber <= pageCount; pageNumber++) {
          if (cancelled) break;
          const page = await pdfDocument.getPage(pageNumber);
          if (cancelled) break;
          const host = pagesRef.current;
          if (!host) break;

          const cssViewport = page.getViewport({ scale: pageCssScale });
          const pixelRatio = Math.min(
            maxRenderPixelRatio,
            globalThis.devicePixelRatio || 1,
          );
          const renderViewport = page.getViewport({
            scale: pageCssScale * pixelRatio,
          });

          const canvas = document.createElement("canvas");
          canvas.className = pdfPageCanvasClassName;
          canvas.width = Math.floor(renderViewport.width);
          canvas.height = Math.floor(renderViewport.height);
          canvas.style.width = `${
            Math.floor(renderViewport.width / pixelRatio)
          }px`;

          const pageShell = document.createElement("div");
          pageShell.className = pdfPageShellClassName;
          pageShell.style.width = `${Math.floor(cssViewport.width)}px`;
          pageShell.append(canvas);
          host.append(pageShell);

          renderTask = page.render({ canvas, viewport: renderViewport });
          await renderTask.promise;
          page.cleanup();
          setState({
            phase: "rendering",
            pageCount,
            renderedPages: pageNumber,
          });
        }
        await pdfDocument.destroy();
        if (!cancelled) {
          setState({ phase: "ready", pageCount });
        }
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
      renderTask?.cancel();
      void loadingTask.destroy();
      pagesRef.current?.replaceChildren();
    };
  }, [bytes]);

  return (
    <section className={pdfViewerClassName}>
      <div className={pdfPagesClassName}>
        {state.phase === "rendering"
          ? (
            <PdfRenderStatus
              pageCount={state.pageCount}
              renderedPages={state.renderedPages}
            />
          )
          : state.phase === "error"
          ? (
            <div className={`${fileViewerStatusClassName} error`}>
              <span>{state.message}</span>
            </div>
          )
          : null}
        <div ref={pagesRef} />
      </div>
    </section>
  );
}

function PdfRenderStatus(
  { pageCount, renderedPages }: { pageCount?: number; renderedPages: number },
) {
  return (
    <div className="mb-[14px] flex justify-center text-[#667085] text-[12px]">
      {pageCount === undefined
        ? "Opening PDF"
        : `Rendering ${renderedPages}/${pageCount} pages`}
    </div>
  );
}

function hasCompleteInitialBytes(
  fsEntry: FsEntry,
  initialBytes: Uint8Array,
): boolean {
  return fsEntry.size !== undefined && fsEntry.size <= initialBytes.byteLength;
}
