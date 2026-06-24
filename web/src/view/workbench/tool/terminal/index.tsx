import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { HardDrive, KeyRound } from "lucide-react";
import {
  attachTerminalSession,
  closeTerminalSession,
  createTerminalSession,
  RpcError,
  takeTerminalControl,
  type TerminalEvent,
  type TerminalExit,
  type TerminalLaunchSpec,
  type TerminalSessionInfo,
  writeTerminalInput,
} from "../../../../protocol/rpc.ts";
import { machineModalBunja } from "../../../../state/machine-modal.ts";
import { machineStoreBunja } from "../../../../state/machine-store.ts";
import type { Machine } from "../../../../state/machines.ts";
import { rpcSessionBunja } from "../../../../state/rpc-session.ts";
import {
  workbenchTabBunja,
  type WorkbenchTerminalSessionSnapshot,
} from "../../../../state/workbench.ts";
import { Button } from "../../../ui/button.tsx";

const terminalToolClassName = [
  "grid [grid-template-rows:minmax(0,1fr)_auto_auto]",
  "w-full h-full min-w-0 min-h-0 overflow-hidden bg-[#0b0f16]",
].join(" ");
const terminalSurfaceClassName =
  "relative w-full h-full min-w-0 min-h-0 overflow-hidden";
const emptyWorkspaceClassName = [
  "grid content-center justify-items-center w-full h-full gap-[10px]",
  "min-h-0 bg-white text-[#667085]",
  "[&_h2]:m-0 [&_h2]:text-[#303642] [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");
const terminalFooterClassName = [
  "flex items-center justify-end gap-[12px] h-[2rem] min-h-[2rem] box-border",
  "border-t border-t-[#d8dde7] bg-[#fbfcfe] text-[#667085]",
  "px-[8px] leading-[1.6]",
  "[@container_workbench-tab-page_(min-width:520px)]:justify-between",
].join(" ");
const terminalFooterDetailsClassName = [
  "hidden min-w-0 items-center gap-[8px]",
  "[@container_workbench-tab-page_(min-width:520px)]:flex",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:font-650 [&_strong]:text-[#344054]",
  "[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis",
  "[&_span]:whitespace-nowrap",
].join(" ");
const terminalFooterSizeClassName = "flex-[0_0_auto]";
const terminalHostClassName =
  "w-full h-full min-w-0 min-h-0 overflow-hidden p-[8px]";
const terminalOverlayNoticeClassName = [
  "absolute inset-0 z-[2] grid place-items-center bg-[#0b0f16]",
  "px-[18px] text-center text-[#d0d5dd]",
  "[&_div]:grid [&_div]:max-w-[360px] [&_div]:justify-items-center",
  "[&_div]:gap-[10px]",
  "[&_h2]:m-0 [&_h2]:text-[18px] [&_h2]:font-750 [&_h2]:text-[#f2f4f7]",
  "[&_p]:m-0 [&_p]:text-[13px] [&_p]:leading-[1.45]",
].join(" ");
const terminalStatusNoticeClassName = [
  "flex min-h-[2rem] items-center gap-[8px] border-t border-t-[#263244]",
  "bg-[#0f1520] px-[8px] leading-[1.6] text-[#98a2b3]",
  "[&_strong]:flex-[0_0_auto] [&_strong]:font-650 [&_strong]:text-[#d0d5dd]",
  "[&_span]:min-w-0 [&_span]:flex-[1_1_auto] [&_span]:overflow-hidden",
  "[&_span]:text-ellipsis [&_span]:whitespace-nowrap",
].join(" ");
const terminalNoticeButtonClassName =
  "!h-[1.5rem] !min-h-[1.5rem] !rounded-[4px] !border-[#38465a] !bg-[#111827] !px-[6px] !leading-none !text-[#d0d5dd] hover:!bg-[#172033]";

interface TerminalRuntime {
  terminal: XTerm;
  fitAddon: FitAddon;
  inputDisposable: { dispose: () => void };
  replayQueryDisposables: Array<{ dispose: () => void }>;
  resizeObserver: ResizeObserver;
}

interface TerminalStatus {
  phase:
    | "idle"
    | "opening"
    | "attached"
    | "viewing"
    | "closed"
    | "missing"
    | "error";
  message: string;
}

interface TerminalNotice {
  actionLabel: string;
  presentation: "banner" | "overlay";
  message: string;
  title: string;
}

interface OpenTerminalOptions {
  cwd?: string;
  existingSessionId?: string;
  launch?: TerminalLaunchSpec;
  title?: string;
}

export function TerminalTool() {
  const machineStore = useBunja(machineStoreBunja);
  const machineModal = useBunja(machineModalBunja);
  const rpcSession = useBunja(rpcSessionBunja);
  const tabState = useBunja(workbenchTabBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const isPaired = useAtomValue(machineStore.selectedIsPairedAtom);
  const daemonInstanceId = useAtomValue(rpcSession.daemonInstanceIdAtom);
  const tab = useAtomValue(tabState.tabAtom);
  const active = useAtomValue(tabState.activeAtom);
  const hostRef = useRef<HTMLDivElement>(null);
  const machineRef = useRef<Machine | undefined>(undefined);
  const activeRef = useRef(false);
  const runtimeRef = useRef<TerminalRuntime | undefined>(undefined);
  const terminalSessionIdRef = useRef<string | undefined>(undefined);
  const attachIdRef = useRef<string | undefined>(undefined);
  const primaryAttachIdRef = useRef<string | undefined>(undefined);
  const latestSeqRef = useRef<number | undefined>(undefined);
  const stopAttachRef = useRef<(() => void) | undefined>(undefined);
  const generationRef = useRef(0);
  const inputQueueRef = useRef<Promise<void>>(Promise.resolve());
  const takeControlPromiseRef = useRef<Promise<void> | undefined>(undefined);
  const replayOutputUntilSeqRef = useRef<number | undefined>(undefined);
  const suppressReplayGeneratedInputRef = useRef(false);
  const replayWriteDepthRef = useRef(0);
  const persistedSessionSnapshotKeyRef = useRef<string | undefined>(undefined);
  const terminalSessionFinishedRef = useRef(false);
  const lastSizeRef = useRef<{ cols: number; rows: number } | undefined>(
    undefined,
  );
  const [terminalReady, setTerminalReady] = useState(false);
  const [status, setStatus] = useState<TerminalStatus>({
    phase: "idle",
    message: "Terminal idle",
  });
  const [terminalDimensions, setTerminalDimensions] = useState({
    cols: 80,
    rows: 24,
  });
  const [sessionInfo, setSessionInfo] = useState<TerminalSessionInfo>();

  useEffect(() => {
    machineRef.current = machine;
  }, [machine]);

  useLayoutEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    if (!active) return;
    fitTerminal();
    void takeCurrentControl();
  }, [active]);

  useEffect(() => {
    if (!sessionInfo) {
      persistedSessionSnapshotKeyRef.current = undefined;
      return;
    }

    const snapshot: WorkbenchTerminalSessionSnapshot = {
      lastKnownCwd: sessionInfo.lastKnownCwd,
      lastKnownTitle: sessionInfo.lastKnownTitle,
      launch: sessionInfo.launch,
    };
    const snapshotKey = terminalSessionSnapshotKey(snapshot);
    if (persistedSessionSnapshotKeyRef.current === snapshotKey) return;
    persistedSessionSnapshotKeyRef.current = snapshotKey;
    tabState.setTerminalSessionSnapshot(snapshot);
  }, [sessionInfo, tabState]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const terminal = new XTerm({
      cursorBlink: true,
      fontFamily: "Cascadia Mono, CaskaydiaCove Nerd Font, Consolas, monospace",
      fontSize: 13,
      lineHeight: 1.15,
      scrollback: 5000,
      theme: {
        background: "#0b0f16",
        foreground: "#e5edf7",
        cursor: "#f8fafc",
        selectionBackground: "#334155",
      },
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(host);
    fitAddon.fit();
    const size = terminalSize(terminal);
    lastSizeRef.current = size;
    setTerminalDimensions(size);

    const inputDisposable = terminal.onData((data) => queueTerminalInput(data));
    const replayQueryDisposables = installReplayQueryHandlers(
      terminal,
      () => replayWriteDepthRef.current > 0,
    );
    installTerminalClipboardShortcuts(terminal);
    const resizeObserver = new ResizeObserver(() => {
      fitTerminal();
      void takeCurrentControl();
    });
    resizeObserver.observe(host);
    runtimeRef.current = {
      terminal,
      fitAddon,
      inputDisposable,
      replayQueryDisposables,
      resizeObserver,
    };
    setTerminalReady(true);

    return () => {
      setTerminalReady(false);
      stopCurrentAttach();
      inputDisposable.dispose();
      for (const disposable of replayQueryDisposables) disposable.dispose();
      resizeObserver.disconnect();
      terminal.dispose();
      runtimeRef.current = undefined;
    };
  }, []);

  useEffect(() => {
    if (!terminalReady || !machine || !isPaired || !daemonInstanceId) {
      stopCurrentAttach();
      terminalSessionIdRef.current = undefined;
      attachIdRef.current = undefined;
      primaryAttachIdRef.current = undefined;
      takeControlPromiseRef.current = undefined;
      replayOutputUntilSeqRef.current = undefined;
      suppressReplayGeneratedInputRef.current = false;
      replayWriteDepthRef.current = 0;
      latestSeqRef.current = undefined;
      setSessionInfo(undefined);
      setStatus({ phase: "idle", message: "Terminal idle" });
      return;
    }

    void openTerminal({
      cwd: tab?.terminalLastKnownCwd,
      existingSessionId: tab?.terminalSessionId,
      launch: tab?.terminalLaunch,
      title: tab?.terminalLastKnownTitle ?? tab?.title,
    });
    return () => stopCurrentAttach();
  }, [
    terminalReady,
    machine?.id,
    machine?.baseUrl,
    machine?.clientId,
    machine?.clientSecret,
    isPaired,
    daemonInstanceId,
    tab?.id,
  ]);

  async function openTerminal(options: OpenTerminalOptions) {
    const currentMachine = machineRef.current;
    const runtime = runtimeRef.current;
    if (!currentMachine || !runtime) return;

    const generation = generationRef.current + 1;
    generationRef.current = generation;
    stopCurrentAttach();
    terminalSessionIdRef.current = undefined;
    attachIdRef.current = undefined;
    primaryAttachIdRef.current = undefined;
    takeControlPromiseRef.current = undefined;
    replayOutputUntilSeqRef.current = undefined;
    suppressReplayGeneratedInputRef.current = options.existingSessionId !==
      undefined;
    replayWriteDepthRef.current = 0;
    latestSeqRef.current = undefined;
    terminalSessionFinishedRef.current = false;
    persistedSessionSnapshotKeyRef.current = undefined;
    setSessionInfo(undefined);
    runtime.terminal.reset();

    try {
      if (options.existingSessionId) {
        terminalSessionIdRef.current = options.existingSessionId;
        setStatus({ phase: "opening", message: "Attaching terminal" });
        await attachTerminal(
          currentMachine,
          options.existingSessionId,
          generation,
        );
        return;
      }

      setStatus({ phase: "opening", message: "Opening terminal" });

      if (!options.launch?.command) {
        setStatus({
          phase: "error",
          message: "Terminal launch command is missing",
        });
        runtime.terminal.writeln("Terminal launch command is missing.");
        runtime.terminal.writeln("Open a terminal from the machine panel.");
        return;
      }

      fitTerminal();
      const size = terminalSize(runtime.terminal);
      const transport = await rpcSession.webTransport();
      const session = await createTerminalSession(
        transport,
        {
          cols: size.cols,
          cwd: options.cwd,
          rows: size.rows,
          launch: options.launch,
          title: options.title,
        },
      );
      if (generationRef.current !== generation) {
        void closeTerminalSession(
          transport,
          session.terminalSessionId,
        ).catch(() => {});
        return;
      }
      terminalSessionIdRef.current = session.terminalSessionId;
      tabState.setTerminalSessionId(session.terminalSessionId);
      setSessionInfo(session);
      await attachTerminal(
        currentMachine,
        session.terminalSessionId,
        generation,
      );
    } catch (err) {
      if (generationRef.current !== generation) return;
      if (options.existingSessionId && isTerminalSessionNotFoundError(err)) {
        clearTerminalSessionReference();
        setStatus({
          phase: "missing",
          message: "The daemon no longer has this terminal session.",
        });
        return;
      }
      setStatus({ phase: "error", message: errorMessage(err) });
    }
  }

  async function attachTerminal(
    currentMachine: Machine,
    terminalSessionId: string,
    generation: number,
  ) {
    const runtime = runtimeRef.current;
    if (!runtime) return;
    const size = activeRef.current
      ? terminalSize(runtime.terminal)
      : sessionInfo ?? terminalDimensions;
    let cancelled = false;
    let iterator: AsyncGenerator<TerminalEvent> | undefined;
    stopAttachRef.current = () => {
      cancelled = true;
      void iterator?.return(undefined);
    };

    const transport = await rpcSession.webTransport();
    if (cancelled) return;
    iterator = attachTerminalSession(
      transport,
      {
        terminalSessionId,
        afterSeq: latestSeqRef.current,
        viewportCols: size.cols,
        viewportRows: size.rows,
      },
    );

    for await (const event of iterator) {
      if (cancelled || generationRef.current !== generation) return;
      handleTerminalEvent(event);
    }
    if (
      !cancelled &&
      generationRef.current === generation &&
      !terminalSessionFinishedRef.current
    ) {
      setStatus({
        phase: "closed",
        message: "Terminal session ended.",
      });
    }
  }

  function handleTerminalEvent(event: TerminalEvent) {
    switch (event.type) {
      case "attached":
        attachIdRef.current = event.attachId;
        primaryAttachIdRef.current = event.primaryAttachId;
        replayOutputUntilSeqRef.current =
          suppressReplayGeneratedInputRef.current
            ? event.session.latestOutputSeq
            : undefined;
        setSessionInfo(event.session);
        if (event.session.exit) {
          terminalSessionFinishedRef.current = true;
          setStatus({
            phase: "closed",
            message: terminalExitMessage(event.session.exit),
          });
          return;
        }
        setStatus({
          phase: event.primaryAttachId === event.attachId
            ? "attached"
            : "viewing",
          message: event.primaryAttachId === event.attachId
            ? "Terminal attached"
            : "Terminal attached in view mode",
        });
        if (activeRef.current) void takeCurrentControl();
        return;
      case "outputChunk":
        latestSeqRef.current = event.seq;
        writeTerminalOutput(event.bytes, isSuppressibleReplayOutput(event.seq));
        if (
          replayOutputUntilSeqRef.current !== undefined &&
          event.seq >= replayOutputUntilSeqRef.current
        ) {
          replayOutputUntilSeqRef.current = undefined;
        }
        return;
      case "historyGap":
        setStatus({
          phase: "viewing",
          message: `Output history resumes at ${event.nextSeq}`,
        });
        return;
      case "controlChanged":
        primaryAttachIdRef.current = event.primaryAttachId;
        setStatus({
          phase: event.primaryAttachId === attachIdRef.current
            ? "attached"
            : "viewing",
          message: event.primaryAttachId === attachIdRef.current
            ? "Terminal attached"
            : "Another view has control",
        });
        return;
      case "pseudoTerminalResized":
        setTerminalDimensions({ cols: event.cols, rows: event.rows });
        setSessionInfo((current) =>
          current ? { ...current, cols: event.cols, rows: event.rows } : current
        );
        return;
      case "sessionExited":
        terminalSessionFinishedRef.current = true;
        setSessionInfo((current) =>
          current ? { ...current, exit: event.exit } : current
        );
        return;
      case "sessionClosed":
        terminalSessionFinishedRef.current = true;
        primaryAttachIdRef.current = undefined;
        takeControlPromiseRef.current = undefined;
        replayOutputUntilSeqRef.current = undefined;
        suppressReplayGeneratedInputRef.current = false;
        replayWriteDepthRef.current = 0;
        clearTerminalSessionReference();
        setStatus({
          phase: "closed",
          message: terminalSessionClosedMessage(event.reason),
        });
        return;
      case "workingDirectoryChanged":
        setSessionInfo((current) =>
          current ? { ...current, lastKnownCwd: event.cwd } : current
        );
        return;
      case "titleChanged":
        setSessionInfo((current) =>
          current ? { ...current, lastKnownTitle: event.title } : current
        );
        return;
    }
  }

  function queueTerminalInput(data: string) {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(data);
    inputQueueRef.current = inputQueueRef.current
      .catch(() => {})
      .then(async () => {
        const currentMachine = machineRef.current;
        if (
          terminalSessionFinishedRef.current ||
          !currentMachine ||
          !terminalSessionIdRef.current
        ) return;
        if (!(await ensureCurrentControl())) return;
        const terminalSessionId = terminalSessionIdRef.current;
        const attachId = attachIdRef.current;
        if (
          !terminalSessionId ||
          !attachId ||
          primaryAttachIdRef.current !== attachId
        ) return;
        await writeTerminalInput(
          await rpcSession.webTransport(),
          terminalSessionId,
          attachId,
          bytes,
        );
      })
      .catch((err) => {
        if (isStalePrimaryAttachError(err)) return;
        setStatus({ phase: "error", message: errorMessage(err) });
      });
  }

  function isSuppressibleReplayOutput(seq: number) {
    return replayOutputUntilSeqRef.current !== undefined &&
      seq <= replayOutputUntilSeqRef.current;
  }

  function writeTerminalOutput(
    bytes: Uint8Array,
    suppressGeneratedInput: boolean,
  ) {
    const terminal = runtimeRef.current?.terminal;
    if (!terminal) return;
    if (!suppressGeneratedInput) {
      terminal.write(bytes);
      return;
    }

    replayWriteDepthRef.current += 1;
    try {
      terminal.write(bytes, () => {
        replayWriteDepthRef.current = Math.max(
          0,
          replayWriteDepthRef.current - 1,
        );
      });
    } catch (err) {
      replayWriteDepthRef.current = Math.max(
        0,
        replayWriteDepthRef.current - 1,
      );
      throw err;
    }
  }

  function isLivePrimaryAttach() {
    const attachId = attachIdRef.current;
    return attachId !== undefined && primaryAttachIdRef.current === attachId;
  }

  async function ensureCurrentControl() {
    if (isLivePrimaryAttach()) return true;
    if (terminalSessionFinishedRef.current || !activeRef.current) return false;
    takeControlPromiseRef.current ??= takeCurrentControl().finally(() => {
      takeControlPromiseRef.current = undefined;
    });
    await takeControlPromiseRef.current;
    return isLivePrimaryAttach();
  }

  async function takeCurrentControl() {
    const currentMachine = machineRef.current;
    const runtime = runtimeRef.current;
    const terminalSessionId = terminalSessionIdRef.current;
    const attachId = attachIdRef.current;
    if (
      terminalSessionFinishedRef.current ||
      !activeRef.current ||
      !currentMachine ||
      !runtime ||
      !terminalSessionId ||
      !attachId
    ) return;
    const size = terminalSize(runtime.terminal);
    const previous = lastSizeRef.current;
    lastSizeRef.current = size;
    setTerminalDimensions(size);

    try {
      const response = await takeTerminalControl(
        await rpcSession.webTransport(),
        {
          terminalSessionId,
          attachId,
          viewportCols: size.cols,
          viewportRows: size.rows,
        },
      );
      primaryAttachIdRef.current = response.primaryAttachId;
      setStatus({
        phase: "attached",
        message: previous?.cols !== size.cols || previous?.rows !== size.rows
          ? `${size.cols}x${size.rows}`
          : "Terminal attached",
      });
      setSessionInfo((current) =>
        current
          ? {
            ...current,
            cols: size.cols,
            rows: size.rows,
            primaryAttachId: response.primaryAttachId,
          }
          : current
      );
    } catch (err) {
      setStatus({ phase: "error", message: errorMessage(err) });
    }
  }

  function fitTerminal() {
    const runtime = runtimeRef.current;
    if (!runtime) return;
    try {
      runtime.fitAddon.fit();
      setTerminalDimensions(terminalSize(runtime.terminal));
    } catch {
      // xterm fit can fail while the pane is hidden or not yet measurable.
    }
  }

  function stopCurrentAttach() {
    stopAttachRef.current?.();
    stopAttachRef.current = undefined;
  }

  function clearTerminalSessionReference() {
    terminalSessionIdRef.current = undefined;
    attachIdRef.current = undefined;
    primaryAttachIdRef.current = undefined;
    takeControlPromiseRef.current = undefined;
    replayOutputUntilSeqRef.current = undefined;
    suppressReplayGeneratedInputRef.current = false;
    replayWriteDepthRef.current = 0;
    latestSeqRef.current = undefined;
    tabState.setTerminalSessionId(undefined);
  }

  function openNewTerminalSession() {
    const launch = sessionInfo?.launch ?? tab?.terminalLaunch;
    const cwd = sessionInfo?.lastKnownCwd ?? tab?.terminalLastKnownCwd;
    const title = sessionInfo?.lastKnownTitle ?? tab?.terminalLastKnownTitle ??
      tab?.title;
    clearTerminalSessionReference();
    void openTerminal({
      cwd,
      launch,
      title,
    });
  }

  if (!machine) {
    return (
      <section className={emptyWorkspaceClassName}>
        <HardDrive size={28} />
        <h2>No machine selected</h2>
      </section>
    );
  }

  if (!isPaired) {
    return (
      <section className={emptyWorkspaceClassName}>
        <KeyRound size={28} />
        <h2>Pairing required</h2>
        <Button
          onClick={() => machineModal.openPairMachineModal(machine.id)}
        >
          <KeyRound size={16} />
          Pair
        </Button>
      </section>
    );
  }

  const terminalNotice = terminalNoticeFromState(status, sessionInfo);
  const canOpenNewTerminal = Boolean(
    sessionInfo?.launch.command ?? tab?.terminalLaunch?.command,
  );

  return (
    <section className={terminalToolClassName}>
      <div className={terminalSurfaceClassName}>
        <div ref={hostRef} className={terminalHostClassName} />
        {terminalNotice?.presentation === "overlay"
          ? (
            <div className={terminalOverlayNoticeClassName}>
              <div>
                <h2>{terminalNotice.title}</h2>
                <p>{terminalNotice.message}</p>
                {canOpenNewTerminal
                  ? (
                    <Button
                      className={terminalNoticeButtonClassName}
                      onClick={openNewTerminalSession}
                    >
                      {terminalNotice.actionLabel}
                    </Button>
                  )
                  : null}
              </div>
            </div>
          )
          : null}
      </div>
      {terminalNotice?.presentation === "banner"
        ? (
          <div className={terminalStatusNoticeClassName}>
            <strong>{terminalNotice.title}</strong>
            <span>{terminalNotice.message}</span>
            {canOpenNewTerminal
              ? (
                <Button
                  className={terminalNoticeButtonClassName}
                  onClick={openNewTerminalSession}
                >
                  {terminalNotice.actionLabel}
                </Button>
              )
              : null}
          </div>
        )
        : null}
      <div className={terminalFooterClassName}>
        <div className={terminalFooterDetailsClassName}>
          <strong>{terminalLaunchName(sessionInfo)}</strong>
          {sessionInfo?.lastKnownCwd
            ? <span>{sessionInfo.lastKnownCwd}</span>
            : null}
        </div>
        <span className={terminalFooterSizeClassName}>
          {sessionInfo?.cols ?? terminalDimensions.cols}
          {" x "}
          {sessionInfo?.rows ?? terminalDimensions.rows}
        </span>
      </div>
    </section>
  );
}

function terminalSize(terminal: XTerm): { cols: number; rows: number } {
  return {
    cols: Math.max(1, terminal.cols || 80),
    rows: Math.max(1, terminal.rows || 24),
  };
}

function terminalLaunchName(sessionInfo: TerminalSessionInfo | undefined) {
  if (!sessionInfo?.launch.command) return "Terminal";
  return commandName(sessionInfo.launch.command);
}

function terminalNoticeFromState(
  status: TerminalStatus,
  sessionInfo: TerminalSessionInfo | undefined,
): TerminalNotice | undefined {
  if (sessionInfo?.exit) {
    return {
      actionLabel: "Restart",
      presentation: "banner",
      title: "Exited",
      message: terminalExitMessage(sessionInfo.exit),
    };
  }
  if (status.phase === "missing") {
    return {
      actionLabel: "Start new terminal session",
      presentation: "overlay",
      title: "Terminal session closed",
      message: status.message,
    };
  }
  if (status.phase === "closed") {
    return {
      actionLabel: "Start new terminal session",
      presentation: "overlay",
      title: "Terminal session closed",
      message: status.message,
    };
  }
  if (status.phase === "error") {
    return {
      actionLabel: "Start new terminal session",
      presentation: "overlay",
      title: "Terminal error",
      message: status.message,
    };
  }
  return undefined;
}

function terminalExitMessage(exit: TerminalExit): string {
  if (exit.code !== undefined) return `Process exited with code ${exit.code}.`;
  if (exit.signal) return `Process exited after signal ${exit.signal}.`;
  return "Process exited.";
}

function terminalSessionClosedMessage(reason: string): string {
  if (reason === "ClosedByClient") {
    return "A client closed this terminal session.";
  }
  return humanizeCode(reason, "The terminal session was closed.");
}

function terminalSessionSnapshotKey(
  snapshot: WorkbenchTerminalSessionSnapshot,
): string {
  return JSON.stringify([
    snapshot.launch?.command,
    snapshot.launch?.args ?? [],
    snapshot.lastKnownCwd,
    snapshot.lastKnownTitle,
  ]);
}

function commandName(command: string): string {
  const normalized = command.replaceAll("\\", "/");
  const segments = normalized.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? command;
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function isStalePrimaryAttachError(err: unknown): boolean {
  return err instanceof RpcError && err.code === "NotPrimaryAttach";
}

function installTerminalClipboardShortcuts(terminal: XTerm) {
  terminal.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    const modifierPressed = event.ctrlKey || event.metaKey;
    if (!modifierPressed || event.altKey) return true;

    const key = event.key.toLowerCase();
    if (key === "c" && terminal.hasSelection()) {
      event.preventDefault();
      void copyTerminalSelection(terminal).catch(() => {});
      return false;
    }
    if (key === "v") {
      event.preventDefault();
      void pasteClipboardToTerminal(terminal).catch(() => {});
      return false;
    }
    return true;
  });
}

async function copyTerminalSelection(terminal: XTerm) {
  const selection = terminal.getSelection();
  if (!selection) return;
  await navigator.clipboard.writeText(selection);
}

async function pasteClipboardToTerminal(terminal: XTerm) {
  const text = await navigator.clipboard.readText();
  if (!text) return;
  terminal.paste(text);
}

function installReplayQueryHandlers(
  terminal: XTerm,
  shouldSuppress: () => boolean,
): Array<{ dispose: () => void }> {
  const consumeReplayQuery = () => shouldSuppress();
  return [
    terminal.parser.registerCsiHandler(
      { final: "n" },
      consumeReplayQuery,
    ),
    terminal.parser.registerCsiHandler(
      { prefix: "?", final: "n" },
      consumeReplayQuery,
    ),
    terminal.parser.registerCsiHandler(
      { final: "c" },
      consumeReplayQuery,
    ),
    terminal.parser.registerCsiHandler(
      { prefix: ">", final: "c" },
      consumeReplayQuery,
    ),
    terminal.parser.registerCsiHandler(
      { prefix: "=", final: "c" },
      consumeReplayQuery,
    ),
  ];
}

function isTerminalSessionNotFoundError(err: unknown): boolean {
  return err instanceof RpcError && err.code === "NotFound";
}

function humanizeCode(code: string, fallback: string): string {
  const words = code
    .replaceAll(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replaceAll(/[_-]+/g, " ")
    .trim()
    .toLowerCase();
  if (!words) return fallback;
  return `${capitalize(words)}.`;
}

function capitalize(text: string): string {
  return text.length === 0 ? text : text[0]!.toUpperCase() + text.slice(1);
}
