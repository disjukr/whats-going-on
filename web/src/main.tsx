import React, {
  createContext,
  FormEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import {
  createStore,
  Provider as JotaiProvider,
  useAtomValue,
  useSetAtom,
} from "jotai";
import { bindScope, BunjaStoreProvider, useBunja } from "bunja/react";
import {
  type DividerRenderProps,
  Handle,
  type LayoutState,
  Pane,
  Root as PaneRoot,
  useLayout,
} from "panecake";
import {
  Activity,
  ArrowLeft,
  ArrowUp,
  ChevronDown,
  ChevronRight,
  Columns2,
  FileQuestion,
  FileText,
  Folder,
  GripVertical,
  HardDrive,
  Info,
  KeyRound,
  Link2,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RefreshCw,
  Rows2,
  Settings,
  Terminal,
  Trash2,
  WifiOff,
  X,
} from "lucide-react";
import "./styles.css";
import { FsEntry, FsEntryKind } from "./protocol/rpc.ts";
import { connectionBunja } from "./state/connection.ts";
import {
  displayName,
  explorerBunja,
  ExplorerMachineScope,
  explorerNavigationBunja,
  ExplorerPaneScope,
  formatDate,
  formatSize,
  kindLabel,
  pathCrumbs,
} from "./state/explorer.ts";
import { type JotaiStore, JotaiStoreScope } from "./state/jotai-store.ts";
import { machineMenuBunja } from "./state/machine-menu.ts";
import { machineModalBunja } from "./state/machine-modal.ts";
import { machineStoreBunja } from "./state/machine-store.ts";
import { Machine } from "./state/machines.ts";
import type { ConnectionState } from "./state/types.ts";
import {
  workbenchBunja,
  type WorkbenchFeature,
  type WorkbenchPane,
  type WorkbenchTab,
} from "./state/workbench.ts";

const jotaiStore = createStore();
const JotaiStoreContext = createContext<JotaiStore>(jotaiStore);
bindScope(JotaiStoreScope, JotaiStoreContext);
const projectLogoUrl = new URL("./assets/wgo.svg", import.meta.url).href;
const machinePanelMinWidth = 212;
const machinePanelMaxWidth = 420;
const minimumWorkbenchWidth = 360;
const machineRailWidth = 64;

type EntryMenuState = {
  entry: FsEntry;
  x: number;
  y: number;
};

function App() {
  const machineStore = useBunja(machineStoreBunja);
  const machineMenuState = useBunja(machineMenuBunja);
  const machineModal = useBunja(machineModalBunja);
  const connectionState = useBunja(connectionBunja);
  const workbench = useBunja(workbenchBunja);

  const machines = useAtomValue(machineStore.machinesAtom);
  const selected = useAtomValue(machineStore.selectedAtom);
  const selectedId = useAtomValue(machineStore.selectedIdAtom);
  const selectedIsPaired = useAtomValue(machineStore.selectedIsPairedAtom);
  const machineName = useAtomValue(machineModal.machineNameAtom);
  const baseUrl = useAtomValue(machineModal.baseUrlAtom);
  const configNameDraft = useAtomValue(machineModal.configNameDraftAtom);
  const configUrlDraft = useAtomValue(machineModal.configUrlDraftAtom);
  const machineModalMode = useAtomValue(machineModal.machineModalModeAtom);
  const machineFormError = useAtomValue(machineModal.machineFormErrorAtom);
  const pairingCode = useAtomValue(machineModal.pairingCodeAtom);
  const isPairing = useAtomValue(machineModal.isPairingAtom);
  const machineMenu = useAtomValue(machineMenuState.machineMenuAtom);
  const menuMachine = useAtomValue(machineMenuState.menuMachineAtom);
  const railTooltip = useAtomValue(machineMenuState.railTooltipAtom);
  const connection = useAtomValue(connectionState.connectionAtom);
  const connectionEpoch = useAtomValue(connectionState.connectionEpochAtom);
  const modalTitle = useAtomValue(machineModal.modalTitleAtom);
  const activeFeature = useAtomValue(workbench.activeFeatureAtom);
  const workbenchLayout = useAtomValue(workbench.layoutAtom);
  const workbenchPanes = useAtomValue(workbench.panesAtom);
  const setConfigNameDraft = useSetAtom(machineModal.configNameDraftAtom);
  const setConfigUrlDraft = useSetAtom(machineModal.configUrlDraftAtom);
  const setPairingCode = useSetAtom(machineModal.pairingCodeAtom);
  const machineNameInputRef = useRef<HTMLInputElement>(null);
  const configNameInputRef = useRef<HTMLInputElement>(null);
  const pairingCodeInputRef = useRef<HTMLInputElement>(null);
  const [machinePanelWidth, setMachinePanelWidth] = useState(264);
  const [machinePanelCollapsed, setMachinePanelCollapsed] = useState(false);

  useEffect(() => {
    if (machines.length === 0 && !machineModalMode) {
      machineNameInputRef.current?.focus();
    }
  }, [machineModalMode, machines.length]);

  useEffect(() => {
    if (machineModalMode === "add") {
      machineNameInputRef.current?.focus();
      return;
    }
    if (machineModalMode === "pair") {
      pairingCodeInputRef.current?.focus();
      return;
    }
    if (machineModalMode === "config") {
      configNameInputRef.current?.focus();
      configNameInputRef.current?.select();
    }
  }, [machineModalMode]);

  useEffect(() => {
    if (!machineModalMode || machines.length === 0) return;

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        closeMachineModal();
      }
    }

    globalThis.addEventListener("keydown", closeOnEscape);
    return () => globalThis.removeEventListener("keydown", closeOnEscape);
  }, [machineModalMode, machines.length]);

  useEffect(() => {
    if (!machineMenu) return;

    function closeMenu() {
      machineMenuState.closeMachineMenu();
    }

    function closeMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }

    globalThis.addEventListener("mousedown", closeMenu);
    globalThis.addEventListener("keydown", closeMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeMenu);
      globalThis.removeEventListener("keydown", closeMenuOnEscape);
    };
  }, [machineMenuState, machineMenu]);

  function addMachine(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    machineModal.addMachine();
  }

  function closeMachineModal() {
    machineModal.closeMachineModal();
  }

  function openAddMachineModal() {
    machineModal.openAddMachineModal();
    if (machines.length === 0) {
      machineNameInputRef.current?.focus();
    }
  }

  function openMachineContextMenu(
    event: React.MouseEvent<HTMLButtonElement>,
    machine: Machine,
  ) {
    event.preventDefault();
    event.stopPropagation();
    machineMenuState.openMachineMenu(machine.id, event.clientX, event.clientY);
  }

  function openMachineTitleMenu(
    event: React.MouseEvent<HTMLButtonElement>,
    machine: Machine,
  ) {
    event.preventDefault();
    event.stopPropagation();
    if (machineMenu?.machineId === machine.id) {
      machineMenuState.closeMachineMenu();
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    machineMenuState.openMachineMenu(machine.id, rect.left, rect.bottom + 8);
  }

  function showRailTooltip(target: HTMLElement, name: string) {
    const rect = target.getBoundingClientRect();
    machineMenuState.showRailTooltip(
      name,
      rect.right + 12,
      rect.top + rect.height / 2,
    );
  }

  function openConfigMachineModal(machine: Machine) {
    machineModal.openConfigMachineModal(machine.id);
  }

  function openPairMachineModal(machine: Machine) {
    machineModal.openPairMachineModal(machine.id);
  }

  function openDeleteMachineModal(machine: Machine) {
    machineModal.openDeleteMachineModal(machine.id);
  }

  function reconnectSelectedMachine() {
    machineMenuState.closeMachineMenu();
    void checkSelected();
  }

  function saveMachineConfig(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    machineModal.saveMachineConfig();
  }

  function deleteSelectedMachine() {
    machineModal.deleteSelectedMachine();
  }

  async function checkSelected() {
    await connectionState.checkSelected();
  }

  async function pairSelected(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    await machineModal.pairSelected(
      `web:${globalThis.location.host || "local"}`,
    );
  }

  function updateMachineNameDraft(value: string) {
    machineModal.updateMachineNameDraft(value);
  }

  function updateBaseUrlDraft(value: string) {
    machineModal.updateBaseUrlDraft(value);
  }

  function clampMachinePanelWidth(width: number) {
    const maxByViewport = Math.max(
      machinePanelMinWidth,
      globalThis.innerWidth - machineRailWidth - minimumWorkbenchWidth,
    );
    return Math.round(
      Math.min(
        Math.max(width, machinePanelMinWidth),
        Math.min(machinePanelMaxWidth, maxByViewport),
      ),
    );
  }

  function startMachinePanelResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const initialX = event.clientX;
    const initialWidth = machinePanelWidth;
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;

    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    function resize(moveEvent: PointerEvent) {
      setMachinePanelWidth(
        clampMachinePanelWidth(
          initialWidth + moveEvent.clientX - initialX,
        ),
      );
    }

    function stopResize() {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
      globalThis.removeEventListener("pointermove", resize);
      globalThis.removeEventListener("pointerup", stopResize);
      globalThis.removeEventListener("pointercancel", stopResize);
    }

    globalThis.addEventListener("pointermove", resize);
    globalThis.addEventListener("pointerup", stopResize);
    globalThis.addEventListener("pointercancel", stopResize);
  }

  function resizeMachinePanelWithKeyboard(
    event: React.KeyboardEvent<HTMLDivElement>,
  ) {
    const step = event.shiftKey ? 40 : 16;
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setMachinePanelWidth((width) => clampMachinePanelWidth(width - step));
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      setMachinePanelWidth((width) => clampMachinePanelWidth(width + step));
    }
    if (event.key === "Home") {
      event.preventDefault();
      setMachinePanelWidth(machinePanelMinWidth);
    }
    if (event.key === "End") {
      event.preventDefault();
      setMachinePanelWidth((width) =>
        clampMachinePanelWidth(Math.max(width, machinePanelMaxWidth))
      );
    }
  }

  function renderAddMachineForm(showCancel: boolean) {
    return (
      <form className="machine-modal-form" onSubmit={addMachine}>
        <label>
          <span>Name</span>
          <input
            ref={machineNameInputRef}
            value={machineName}
            onChange={(event) => updateMachineNameDraft(event.target.value)}
            placeholder="Local daemon"
            aria-label="Machine name"
          />
        </label>
        <label>
          <span>URL</span>
          <input
            value={baseUrl}
            onChange={(event) => updateBaseUrlDraft(event.target.value)}
            placeholder="https://host:8765"
            aria-label="Machine URL"
          />
        </label>
        {machineFormError
          ? <div className="field-error">{machineFormError}</div>
          : null}
        <div className="modal-actions">
          {showCancel
            ? (
              <button type="button" onClick={closeMachineModal}>
                Cancel
              </button>
            )
            : null}
          <button type="submit">
            <Plus size={16} />
            Continue
          </button>
        </div>
      </form>
    );
  }

  return (
    <main
      className={machinePanelCollapsed
        ? "app-shell machine-panel-collapsed"
        : "app-shell"}
      style={{
        "--machine-panel-width": `${machinePanelWidth}px`,
      } as React.CSSProperties}
    >
      <aside className="machine-rail" aria-label="Machine switcher">
        <div className="rail-brand" title="wgo">
          <img src={projectLogoUrl} alt="wgo" />
        </div>

        <nav className="rail-list" aria-label="Machines">
          {machines.map((machine) => (
            <button
              type="button"
              key={machine.id}
              className={machine.id === selectedId
                ? "rail-machine active"
                : "rail-machine"}
              onClick={() => {
                machineMenuState.selectMachine(machine.id);
              }}
              onMouseEnter={(event) =>
                showRailTooltip(event.currentTarget, machine.name)}
              onMouseLeave={machineMenuState.hideRailTooltip}
              onFocus={(event) =>
                showRailTooltip(event.currentTarget, machine.name)}
              onBlur={machineMenuState.hideRailTooltip}
              onContextMenu={(event) => openMachineContextMenu(event, machine)}
              aria-label={machine.name}
            >
              <span className="rail-indicator" />
              <span className="machine-avatar">
                {machineInitials(machine.name)}
              </span>
            </button>
          ))}
        </nav>

        <button
          type="button"
          className="rail-action"
          onClick={openAddMachineModal}
          title="Add machine"
          aria-label="Add machine"
        >
          <Plus size={22} />
        </button>
      </aside>

      {railTooltip
        ? (
          <div
            className="rail-tooltip"
            style={{ left: railTooltip.x, top: railTooltip.y }}
            role="tooltip"
          >
            {railTooltip.name}
          </div>
        )
        : null}

      <header className="global-topbar">
        <div className="global-topbar-left">
          <button
            type="button"
            className="global-icon-button"
            onClick={() => setMachinePanelCollapsed((collapsed) => !collapsed)}
            title={machinePanelCollapsed
              ? "Expand machine panel"
              : "Collapse machine panel"}
            aria-label={machinePanelCollapsed
              ? "Expand machine panel"
              : "Collapse machine panel"}
            aria-pressed={machinePanelCollapsed}
          >
            {machinePanelCollapsed
              ? <PanelLeftOpen size={14} />
              : <PanelLeftClose size={14} />}
          </button>
        </div>
        <div className="global-machine-title">
          <span>{selected?.name ?? "No machine"}</span>
        </div>
        <ConnectionPill
          machine={selected}
          connection={connection}
          onRefresh={() => void checkSelected()}
        />
      </header>

      <aside
        className="machine-panel"
        aria-label="Machine workspace"
        aria-hidden={machinePanelCollapsed}
      >
        {!machinePanelCollapsed
          ? (
            <>
              <section className="machine-panel-summary">
                <div className="machine-title">
                  <h1>
                    {selected
                      ? (
                        <button
                          type="button"
                          className={[
                            "machine-title-button",
                            connection.phase === "checking" ? "checking" : "",
                          ].filter(Boolean).join(" ")}
                          onMouseDown={(event) => event.stopPropagation()}
                          onClick={(event) =>
                            openMachineTitleMenu(event, selected)}
                          title="Machine actions"
                          aria-label={`${selected.name} machine actions`}
                        >
                          <span className="machine-title-text">
                            {selected.name}
                          </span>
                          {connection.phase === "offline"
                            ? (
                              <WifiOff
                                size={14}
                                className="machine-title-connection-indicator"
                                aria-hidden="true"
                              />
                            )
                            : null}
                          <ChevronDown size={16} />
                        </button>
                      )
                      : "No machine"}
                  </h1>
                </div>
              </section>

              <FeatureMenu
                activeFeature={activeFeature}
                onSelect={workbench.selectFeature}
              />
              <div
                className="machine-panel-resizer"
                role="separator"
                aria-label="Resize machine panel"
                aria-orientation="vertical"
                aria-valuemin={machinePanelMinWidth}
                aria-valuemax={machinePanelMaxWidth}
                aria-valuenow={machinePanelWidth}
                tabIndex={0}
                onPointerDown={startMachinePanelResize}
                onKeyDown={resizeMachinePanelWithKeyboard}
              />
            </>
          )
          : null}
      </aside>

      <section
        className={machines.length === 0 ? "workbench no-machine" : "workbench"}
      >
        {machines.length === 0
          ? (
            <section className="inline-machine-setup">
              <div className="inline-machine-card">
                <header className="modal-head">
                  <div>
                    <span>Machine</span>
                    <h2>Add machine</h2>
                  </div>
                </header>
                {renderAddMachineForm(false)}
              </div>
            </section>
          )
          : (
            <Workbench
              layout={workbenchLayout}
              panes={workbenchPanes}
              setLayout={workbench.setLayout}
              addPane={workbench.addPane}
              removePane={workbench.removePane}
              addFilesTab={workbench.addFilesTab}
              selectTab={workbench.selectTab}
              closeTab={workbench.closeTab}
              machine={selected}
              isPaired={selectedIsPaired}
              connectionEpoch={connectionEpoch}
              onPair={() => selected && openPairMachineModal(selected)}
            />
          )}
      </section>

      {machineMenu && menuMachine
        ? (
          <div
            className="machine-context-menu"
            style={{ left: machineMenu.x, top: machineMenu.y }}
            role="menu"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              onClick={reconnectSelectedMachine}
            >
              <RefreshCw size={15} />
              Reconnect
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => openConfigMachineModal(menuMachine)}
            >
              <Settings size={15} />
              Configure
            </button>
            {!(menuMachine.clientId && menuMachine.clientSecret)
              ? (
                <button
                  type="button"
                  role="menuitem"
                  onClick={() => openPairMachineModal(menuMachine)}
                >
                  <KeyRound size={15} />
                  Pair
                </button>
              )
              : null}
            <button
              type="button"
              role="menuitem"
              className="danger"
              onClick={() => openDeleteMachineModal(menuMachine)}
            >
              <Trash2 size={15} />
              Delete
            </button>
          </div>
        )
        : null}

      {machineModalMode
        ? (
          <div
            className="modal-backdrop"
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) {
                closeMachineModal();
              }
            }}
          >
            <section
              className="machine-modal"
              role="dialog"
              aria-modal="true"
              aria-labelledby="machine-modal-title"
            >
              <header className="modal-head">
                <div>
                  <span>Machine</span>
                  <h2 id="machine-modal-title">{modalTitle}</h2>
                </div>
                {machines.length > 0
                  ? (
                    <button
                      type="button"
                      onClick={closeMachineModal}
                      title="Close"
                      aria-label="Close machine modal"
                      className="icon-button"
                    >
                      <X size={16} />
                    </button>
                  )
                  : null}
              </header>

              {machineModalMode === "pair" && selected
                ? (
                  <form className="machine-modal-form" onSubmit={pairSelected}>
                    <div className="modal-machine-summary">
                      <strong>{selected.name}</strong>
                      <span>{selected.baseUrl}</span>
                    </div>
                    <label>
                      <span>Pairing code</span>
                      <input
                        ref={pairingCodeInputRef}
                        value={pairingCode}
                        onChange={(event) =>
                          setPairingCode(
                            event.target.value.replace(/\D/g, "").slice(0, 6),
                          )}
                        inputMode="numeric"
                        autoComplete="one-time-code"
                        maxLength={6}
                        placeholder="000000"
                        aria-label="Pairing code"
                      />
                    </label>
                    {connection.phase === "offline"
                      ? <div className="field-error">{connection.message}</div>
                      : null}
                    <div className="modal-actions">
                      <button type="button" onClick={closeMachineModal}>
                        Skip
                      </button>
                      <button
                        type="submit"
                        disabled={isPairing || pairingCode.length === 0}
                      >
                        {isPairing
                          ? <Loader2 size={16} className="spin" />
                          : <KeyRound size={16} />}
                        Pair
                      </button>
                    </div>
                  </form>
                )
                : machineModalMode === "config" && selected
                ? (
                  <form
                    className="machine-modal-form"
                    onSubmit={saveMachineConfig}
                  >
                    <div className="modal-machine-summary">
                      <strong>{selected.name}</strong>
                      <span>{selected.baseUrl}</span>
                    </div>
                    <label>
                      <span>Name</span>
                      <input
                        ref={configNameInputRef}
                        value={configNameDraft}
                        onChange={(event) =>
                          setConfigNameDraft(event.target.value)}
                        placeholder="Machine name"
                        aria-label="Machine name"
                      />
                    </label>
                    <label>
                      <span>URL</span>
                      <input
                        value={configUrlDraft}
                        onChange={(event) =>
                          setConfigUrlDraft(event.target.value)}
                        placeholder="https://host:8765"
                        aria-label="Machine URL"
                      />
                    </label>
                    {machineFormError
                      ? <div className="field-error">{machineFormError}</div>
                      : null}
                    <div className="modal-actions">
                      <button type="button" onClick={closeMachineModal}>
                        Cancel
                      </button>
                      <button type="submit">
                        <Settings size={16} />
                        Save
                      </button>
                    </div>
                  </form>
                )
                : machineModalMode === "delete" && selected
                ? (
                  <div className="machine-modal-form">
                    <div className="modal-machine-summary">
                      <strong>{selected.name}</strong>
                      <span>{selected.baseUrl}</span>
                    </div>
                    <p className="modal-warning">
                      This removes the machine from this browser.
                    </p>
                    <div className="modal-actions">
                      <button type="button" onClick={closeMachineModal}>
                        Cancel
                      </button>
                      <button
                        type="button"
                        className="danger-action"
                        onClick={deleteSelectedMachine}
                      >
                        <Trash2 size={16} />
                        Delete
                      </button>
                    </div>
                  </div>
                )
                : (
                  renderAddMachineForm(true)
                )}
            </section>
          </div>
        )
        : null}
    </main>
  );
}

function machineInitials(name: string): string {
  const letters = name
    .split(/[\s._-]+/)
    .map((part) => part.trim()[0])
    .filter(Boolean)
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return letters || "PC";
}

function ConnectionPill(
  { machine, connection, onRefresh }: {
    machine?: Machine;
    connection: ConnectionState;
    onRefresh: () => void;
  },
) {
  if (!machine) return null;

  const checking = connection.phase === "checking";
  const connected = connection.phase === "reachable";
  const label = checking
    ? "Connecting"
    : connected
    ? "Connected"
    : "Unconnected";
  const className = [
    "global-connection-pill",
    connected ? "connected" : "",
    checking ? "checking" : "",
  ].filter(Boolean).join(" ");

  return (
    <button
      type="button"
      className={className}
      onClick={onRefresh}
      title={checking ? "Connecting" : connection.message}
      aria-label={checking ? "Connecting" : `Connection status: ${label}`}
      aria-busy={checking}
    >
      <span className="global-connection-status-icon" aria-hidden="true">
        <span className="global-connection-dot" />
        <RefreshCw
          size={13}
          className={checking
            ? "global-connection-refresh spin"
            : "global-connection-refresh"}
        />
      </span>
      <span>{label}</span>
    </button>
  );
}

const features: {
  id: WorkbenchFeature;
  label: string;
  disabled?: boolean;
  Icon: typeof Folder;
}[] = [
  {
    id: "files",
    label: "Files",
    Icon: Folder,
  },
  {
    id: "processes",
    label: "Processes",
    Icon: Activity,
    disabled: true,
  },
  {
    id: "terminal",
    label: "Terminal",
    Icon: Terminal,
    disabled: true,
  },
];

function FeatureMenu(
  { activeFeature, onSelect }: {
    activeFeature: WorkbenchFeature;
    onSelect: (feature: WorkbenchFeature) => void;
  },
) {
  return (
    <nav className="feature-menu" aria-label="Workspace features">
      {features.map(({ id, label, disabled, Icon }) => (
        <button
          type="button"
          key={id}
          className={activeFeature === id
            ? "feature-item active"
            : "feature-item"}
          onClick={() => onSelect(id)}
          disabled={disabled}
          aria-current={activeFeature === id ? "page" : undefined}
        >
          <Icon size={17} />
          <span>{label}</span>
        </button>
      ))}
    </nav>
  );
}

function Workbench(
  {
    layout,
    panes,
    setLayout,
    addPane,
    removePane,
    addFilesTab,
    selectTab,
    closeTab,
    machine,
    isPaired,
    connectionEpoch,
    onPair,
  }: {
    layout: LayoutState;
    panes: WorkbenchPane[];
    setLayout: (layout: LayoutState) => void;
    addPane: () => string;
    removePane: (paneId: string) => void;
    addFilesTab: (paneId: string) => void;
    selectTab: (paneId: string, tabId: string) => void;
    closeTab: (paneId: string, tabId: string) => void;
    machine?: Machine;
    isPaired: boolean;
    connectionEpoch: number;
    onPair: () => void;
  },
) {
  return (
    <PaneRoot
      layout={layout}
      onLayoutChange={setLayout}
      className="pane-root"
      renderDivider={PaneDivider}
      emptyContent={<div className="empty-workspace">No panes</div>}
    >
      {panes.map((pane) => (
        <Pane key={pane.id} id={pane.id} minWidth={320} minHeight={220}>
          {(nodeId) => (
            <WorkbenchPaneView
              pane={pane}
              paneCount={panes.length}
              nodeId={nodeId}
              addPane={addPane}
              removePane={removePane}
              addFilesTab={addFilesTab}
              selectTab={selectTab}
              closeTab={closeTab}
              machine={machine}
              isPaired={isPaired}
              connectionEpoch={connectionEpoch}
              onPair={onPair}
            />
          )}
        </Pane>
      ))}
    </PaneRoot>
  );
}

function WorkbenchPaneView(
  {
    pane,
    paneCount,
    nodeId,
    addPane,
    removePane,
    addFilesTab,
    selectTab,
    closeTab,
    machine,
    isPaired,
    connectionEpoch,
    onPair,
  }: {
    pane: WorkbenchPane;
    paneCount: number;
    nodeId: string;
    addPane: () => string;
    removePane: (paneId: string) => void;
    addFilesTab: (paneId: string) => void;
    selectTab: (paneId: string, tabId: string) => void;
    closeTab: (paneId: string, tabId: string) => void;
    machine?: Machine;
    isPaired: boolean;
    connectionEpoch: number;
    onPair: () => void;
  },
) {
  const { removePane: removeLayoutPane, split } = useLayout();
  const [newTabMenuOpen, setNewTabMenuOpen] = useState(false);
  const newTabMenuRef = useRef<HTMLDivElement>(null);
  const canClosePane = paneCount > 1;

  useEffect(() => {
    if (!newTabMenuOpen) return;

    function closeNewTabMenu(event: MouseEvent) {
      const target = event.target;
      if (target instanceof Node && newTabMenuRef.current?.contains(target)) {
        return;
      }
      setNewTabMenuOpen(false);
    }

    function closeNewTabMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setNewTabMenuOpen(false);
    }

    globalThis.addEventListener("mousedown", closeNewTabMenu);
    globalThis.addEventListener("keydown", closeNewTabMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeNewTabMenu);
      globalThis.removeEventListener("keydown", closeNewTabMenuOnEscape);
    };
  }, [newTabMenuOpen]);

  function splitPane(direction: "horizontal" | "vertical") {
    const newPaneId = addPane();
    split(nodeId, direction, newPaneId, "after");
  }

  function closePane() {
    if (!canClosePane) return;
    removeLayoutPane(nodeId);
    removePane(pane.id);
  }

  function closeWorkbenchTab(tabId: string) {
    if (pane.tabs.length > 1) {
      closeTab(pane.id, tabId);
      return;
    }
    closePane();
  }

  function openFilesTab() {
    addFilesTab(pane.id);
    setNewTabMenuOpen(false);
  }

  return (
    <section className="workbench-pane">
      <header className="workbench-pane-head">
        <Handle className="pane-handle">
          <GripVertical size={14} />
        </Handle>
        <div className="workbench-tabs" role="tablist">
          {pane.tabs.map((tab) => (
            <WorkbenchTabItem
              key={tab.id}
              tab={tab}
              machine={machine}
              active={tab.id === pane.activeTabId}
              showClose={pane.tabs.length > 1 || canClosePane}
              onSelect={() => selectTab(pane.id, tab.id)}
              onClose={() => closeWorkbenchTab(tab.id)}
            />
          ))}
        </div>
        <div className="pane-actions">
          <div className="new-tab-menu-wrap" ref={newTabMenuRef}>
            <button
              type="button"
              className="icon-button compact new-tab-trigger"
              onClick={() => setNewTabMenuOpen((open) => !open)}
              title="Open tab"
              aria-label="Open tab"
              aria-haspopup="menu"
              aria-expanded={newTabMenuOpen}
            >
              <Plus size={13} />
              <ChevronDown size={11} />
            </button>
            {newTabMenuOpen
              ? (
                <div className="new-tab-menu" role="menu">
                  <button
                    type="button"
                    role="menuitem"
                    onClick={openFilesTab}
                  >
                    <Folder size={14} />
                    Files
                  </button>
                </div>
              )
              : null}
          </div>
          <button
            type="button"
            className="icon-button compact"
            onClick={() => splitPane("horizontal")}
            title="Split right"
            aria-label="Split right"
          >
            <Columns2 size={14} />
          </button>
          <button
            type="button"
            className="icon-button compact"
            onClick={() => splitPane("vertical")}
            title="Split down"
            aria-label="Split down"
          >
            <Rows2 size={14} />
          </button>
          <button
            type="button"
            className="icon-button compact"
            onClick={closePane}
            disabled={!canClosePane}
            title="Close pane"
            aria-label="Close pane"
          >
            <X size={14} />
          </button>
        </div>
      </header>
      <div className="workbench-pane-body">
        {pane.tabs.map((tab) => (
          <section
            key={tab.id}
            className="workbench-tab-page"
            hidden={tab.id !== pane.activeTabId}
          >
            <WorkbenchTabContent
              tab={tab}
              machine={machine}
              isPaired={isPaired}
              connectionEpoch={connectionEpoch}
              onPair={onPair}
            />
          </section>
        ))}
      </div>
    </section>
  );
}

function WorkbenchTabItem(
  { tab, machine, active, showClose, onSelect, onClose }: {
    tab: WorkbenchTab;
    machine?: Machine;
    active: boolean;
    showClose: boolean;
    onSelect: () => void;
    onClose: () => void;
  },
) {
  const label = useWorkbenchTabLabel(tab, machine);

  return (
    <div className={active ? "workbench-tab active" : "workbench-tab"}>
      <button
        type="button"
        role="tab"
        aria-selected={active}
        onClick={onSelect}
        title={label}
      >
        <Folder size={14} className="workbench-tab-icon" />
        <span className="workbench-tab-title">{label}</span>
      </button>
      {showClose
        ? (
          <button
            type="button"
            className="tab-close"
            onClick={onClose}
            title="Close tab"
            aria-label={`Close ${label}`}
          >
            <X size={13} />
          </button>
        )
        : null}
    </div>
  );
}

function useWorkbenchTabLabel(tab: WorkbenchTab, machine?: Machine): string {
  const navigation = useBunja(explorerNavigationBunja, [
    ExplorerMachineScope.bind(machine?.id),
    ExplorerPaneScope.bind(tab.id),
  ]);
  const currentPath = useAtomValue(navigation.currentPathAtom);

  if (tab.tool === "files") return folderNameFromPath(currentPath);
  return tab.title;
}

function folderNameFromPath(path?: string): string {
  const crumbs = pathCrumbs(path);
  return crumbs[crumbs.length - 1]?.label ?? "Files";
}

function WorkbenchTabContent(
  { tab, machine, isPaired, connectionEpoch, onPair }: {
    tab: WorkbenchTab;
    machine?: Machine;
    isPaired: boolean;
    connectionEpoch: number;
    onPair: () => void;
  },
) {
  if (tab.tool === "files") {
    return (
      <Explorer
        paneScopeId={tab.id}
        machine={machine}
        isPaired={isPaired}
        connectionEpoch={connectionEpoch}
        onPair={onPair}
      />
    );
  }
}

function PaneDivider(
  { direction, onMouseDown, onKeyDown, ref }: DividerRenderProps,
) {
  return (
    <div
      ref={ref}
      className={`pane-divider ${direction}`}
      role="separator"
      tabIndex={0}
      onMouseDown={onMouseDown}
      onKeyDown={onKeyDown}
    />
  );
}

function Explorer(
  { paneScopeId, machine, isPaired, connectionEpoch, onPair }: {
    paneScopeId: string;
    machine?: Machine;
    isPaired: boolean;
    connectionEpoch: number;
    onPair: () => void;
  },
) {
  const explorer = useBunja(explorerBunja, [
    ExplorerMachineScope.bind(machine?.id),
    ExplorerPaneScope.bind(paneScopeId),
  ]);
  const currentPath = useAtomValue(explorer.currentPathAtom);
  const history = useAtomValue(explorer.historyAtom);
  const selectedPath = useAtomValue(explorer.selectedPathAtom);
  const visibleRows = useAtomValue(explorer.visibleRowsAtom);
  const selectedEntry = useAtomValue(explorer.selectedEntryAtom);
  const lastConnectionEpochRef = useRef(connectionEpoch);
  const [entryMenu, setEntryMenu] = useState<EntryMenuState>();
  const [propertiesEntry, setPropertiesEntry] = useState<FsEntry>();

  useEffect(() => {
    if (lastConnectionEpochRef.current === connectionEpoch) return;
    lastConnectionEpochRef.current = connectionEpoch;
    explorer.refresh();
  }, [connectionEpoch, explorer]);

  useEffect(() => {
    if (!entryMenu) return;

    function closeMenu() {
      setEntryMenu(undefined);
    }

    function closeMenuOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }

    globalThis.addEventListener("mousedown", closeMenu);
    globalThis.addEventListener("keydown", closeMenuOnEscape);
    return () => {
      globalThis.removeEventListener("mousedown", closeMenu);
      globalThis.removeEventListener("keydown", closeMenuOnEscape);
    };
  }, [entryMenu]);

  useEffect(() => {
    if (!propertiesEntry) return;

    function closeModalOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setPropertiesEntry(undefined);
    }

    globalThis.addEventListener("keydown", closeModalOnEscape);
    return () => globalThis.removeEventListener("keydown", closeModalOnEscape);
  }, [propertiesEntry]);

  const {
    goBack,
    goUp,
    navigate,
    openEntry,
    selectEntry,
  } = explorer;

  function openEntryMenu(
    entry: FsEntry,
    event: React.MouseEvent<HTMLButtonElement>,
  ) {
    event.preventDefault();
    event.stopPropagation();
    selectEntry(entry);
    setEntryMenu({ entry, x: event.clientX, y: event.clientY });
  }

  function openEntryProperties(entry: FsEntry) {
    setEntryMenu(undefined);
    setPropertiesEntry(entry);
  }

  if (!machine) {
    return (
      <section className="empty-workspace">
        <HardDrive size={28} />
        <h2>No machine selected</h2>
      </section>
    );
  }

  if (!isPaired) {
    return (
      <section className="empty-workspace">
        <KeyRound size={28} />
        <h2>Pairing required</h2>
        <button type="button" onClick={onPair}>
          <KeyRound size={16} />
          Pair
        </button>
      </section>
    );
  }

  return (
    <section className="explorer">
      <div className="path-toolbar">
        <button
          type="button"
          onClick={goBack}
          disabled={history.length === 0}
          title="Back"
          aria-label="Back"
          className="icon-button"
        >
          <ArrowLeft size={16} />
        </button>
        <button
          type="button"
          onClick={goUp}
          disabled={!currentPath}
          title="Up"
          aria-label="Up"
          className="icon-button"
        >
          <ArrowUp size={16} />
        </button>
        <PathCrumbs path={currentPath} onNavigate={navigate} />
      </div>

      <div className="browser-layout">
        <FileTable
          rows={visibleRows}
          selectedPath={selectedPath}
          onSelect={selectEntry}
          onOpen={openEntry}
          onContextMenu={openEntryMenu}
        />
        <Inspector entry={selectedEntry} currentPath={currentPath} />
      </div>

      <div className="explorer-footer">
        <span>{visibleRows.length} items</span>
      </div>

      {entryMenu
        ? (
          <div
            className="entry-context-menu"
            style={{ left: entryMenu.x, top: entryMenu.y }}
            role="menu"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <button
              type="button"
              role="menuitem"
              onClick={() => openEntryProperties(entryMenu.entry)}
            >
              <Info size={15} />
              Properties
            </button>
          </div>
        )
        : null}

      {propertiesEntry
        ? (
          <EntryPropertiesModal
            entry={propertiesEntry}
            onClose={() => setPropertiesEntry(undefined)}
          />
        )
        : null}
    </section>
  );
}

function PathCrumbs(
  { path, onNavigate }: {
    path?: string;
    onNavigate: (path?: string) => void;
  },
) {
  const [editing, setEditing] = useState(false);
  const [draftPath, setDraftPath] = useState(path ?? "");
  const inputRef = useRef<HTMLInputElement>(null);
  const crumbs = pathCrumbs(path);

  useEffect(() => {
    if (!editing) setDraftPath(path ?? "");
  }, [editing, path]);

  useEffect(() => {
    if (!editing) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [editing]);

  function beginEditing() {
    setDraftPath(path ?? "");
    setEditing(true);
  }

  function cancelEditing() {
    setDraftPath(path ?? "");
    setEditing(false);
  }

  function submitPath(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextPath = draftPath.trim();
    setEditing(false);
    onNavigate(nextPath || undefined);
  }

  if (editing) {
    return (
      <form className="path-input-form" onSubmit={submitPath}>
        <input
          ref={inputRef}
          value={draftPath}
          onChange={(event) => setDraftPath(event.target.value)}
          onBlur={cancelEditing}
          onKeyDown={(event) => {
            if (event.key === "Escape") cancelEditing();
          }}
          aria-label="Path"
          placeholder="Path"
        />
      </form>
    );
  }

  return (
    <div
      className="crumbs"
      aria-label="Path"
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        event.preventDefault();
        beginEditing();
      }}
    >
      {crumbs.map((crumb, index) => (
        <React.Fragment key={`${crumb.path ?? "roots"}:${index}`}>
          {index > 0 ? <ChevronRight size={14} /> : null}
          <button
            type="button"
            onClick={() =>
              onNavigate(crumb.path)}
          >
            {crumb.label}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
}

function FileTable(
  {
    rows,
    selectedPath,
    onSelect,
    onOpen,
    onContextMenu,
  }: {
    rows: FsEntry[];
    selectedPath?: string;
    onSelect: (entry: FsEntry) => void;
    onOpen: (entry: FsEntry) => void;
    onContextMenu: (
      entry: FsEntry,
      event: React.MouseEvent<HTMLButtonElement>,
    ) => void;
  },
) {
  return (
    <div className="file-table" role="grid" aria-label="Files">
      <div className="file-head name">Name</div>
      <div className="file-head kind">Kind</div>
      <div className="file-head size">Size</div>
      <div className="file-head modified">Modified</div>
      {rows.length === 0 ? <div className="table-empty">No rows</div> : (
        rows.map((entry) => (
          <button
            type="button"
            key={entry.path}
            className={entry.path === selectedPath
              ? "file-row selected"
              : "file-row"}
            onClick={() => onSelect(entry)}
            onDoubleClick={() => onOpen(entry)}
            onContextMenu={(event) => onContextMenu(entry, event)}
          >
            <span className="file-cell name">
              <EntryIcon entry={entry} />
              <span>{displayName(entry)}</span>
              {entry.readonly
                ? <span className="readonly">readonly</span>
                : null}
            </span>
            <span className="file-cell kind">{kindLabel(entry.kind)}</span>
            <span className="file-cell size">{formatSize(entry.size)}</span>
            <span className="file-cell modified">
              {formatDate(entry.modifiedAtMs)}
            </span>
          </button>
        ))
      )}
    </div>
  );
}

function Inspector(
  { entry, currentPath }: { entry?: FsEntry; currentPath?: string },
) {
  return (
    <aside className="inspector">
      <div className="inspector-title">Selection</div>
      <EntryDetails entry={entry} currentPath={currentPath} />
    </aside>
  );
}

function EntryPropertiesModal(
  { entry, onClose }: { entry: FsEntry; onClose: () => void },
) {
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        className="machine-modal entry-properties-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="entry-properties-title"
      >
        <header className="modal-head">
          <div>
            <span>File</span>
            <h2 id="entry-properties-title">Properties</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            title="Close"
            aria-label="Close properties modal"
            className="icon-button"
          >
            <X size={16} />
          </button>
        </header>

        <div className="entry-properties-body">
          <EntryDetails entry={entry} />
        </div>
      </section>
    </div>
  );
}

function EntryDetails(
  { entry, currentPath }: { entry?: FsEntry; currentPath?: string },
) {
  if (!entry) {
    return (
      <dl>
        <dt>Location</dt>
        <dd>{currentPath ?? "Files"}</dd>
      </dl>
    );
  }

  return (
    <dl>
      <dt>Name</dt>
      <dd>{displayName(entry)}</dd>
      <dt>Path</dt>
      <dd>{entry.path}</dd>
      <dt>Kind</dt>
      <dd>{kindLabel(entry.kind)}</dd>
      <dt>Size</dt>
      <dd>{formatSize(entry.size)}</dd>
      <dt>Modified</dt>
      <dd>{formatDate(entry.modifiedAtMs)}</dd>
      <dt>Flags</dt>
      <dd>{entry.readonly ? "Readonly" : "Writable"}</dd>
    </dl>
  );
}

function EntryIcon({ entry }: { entry: FsEntry }) {
  if (entry.kind === FsEntryKind.Directory) {
    return entry.path.endsWith("\\")
      ? <HardDrive size={16} />
      : <Folder size={16} />;
  }
  if (entry.kind === FsEntryKind.Symlink) return <Link2 size={16} />;
  if (entry.kind === FsEntryKind.File) return <FileText size={16} />;
  return <FileQuestion size={16} />;
}

createRoot(document.getElementById("root")!).render(
  <JotaiProvider store={jotaiStore}>
    <JotaiStoreContext.Provider value={jotaiStore}>
      <BunjaStoreProvider>
        <App />
      </BunjaStoreProvider>
    </JotaiStoreContext.Provider>
  </JotaiProvider>,
);
