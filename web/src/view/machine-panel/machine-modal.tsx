import React, { FormEvent } from "react";
import { KeyRound, Loader2, Settings, Trash2, X } from "lucide-react";
import type { Machine } from "../../state/machines.ts";
import type { ConnectionState, MachineModalMode } from "../../state/types.ts";
import { AddMachineForm } from "./add-machine-form.tsx";

const modalBackdropClassName =
  "fixed inset-0 z-[20] grid place-items-center bg-[rgb(32_36_45_/_42%)] p-[24px]";
const machineModalClassName = [
  "w-[min(460px,100%)] overflow-hidden border border-[#d8dde7]",
  "rounded-[8px] bg-white [box-shadow:0_24px_72px_rgb(32_36_45_/_28%)]",
].join(" ");
const modalHeadClassName = [
  "flex items-center justify-between gap-[12px] border-b border-b-[#e4e8ef]",
  "px-[16px] py-[14px]",
  "[&_div]:grid [&_div]:gap-[2px] [&_div]:min-w-0",
  "[&_span]:text-[#667085] [&_span]:text-[12px] [&_span]:font-700",
  "[&_h2]:m-0 [&_h2]:text-[#20242d] [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");
const iconButtonClassName = "w-[36px] min-w-[36px] p-0";
const machineModalFormClassName = [
  "grid gap-[12px] p-[16px]",
  "[&_label]:grid [&_label]:gap-[6px] [&_label]:min-w-0",
  "[&_label_span]:text-[#475467] [&_label_span]:text-[12px] [&_label_span]:font-700",
].join(" ");
const modalMachineSummaryClassName = [
  "grid gap-[2px] min-w-0 border border-[#d8dde7] rounded-[8px]",
  "bg-[#f7f8fb] p-[10px]",
  "[&_strong]:min-w-0 [&_strong]:overflow-hidden [&_strong]:text-ellipsis",
  "[&_strong]:whitespace-nowrap [&_strong]:text-[#20242d] [&_strong]:text-[13px]",
  "[&_span]:min-w-0 [&_span]:overflow-hidden [&_span]:text-ellipsis",
  "[&_span]:whitespace-nowrap [&_span]:text-[#667085] [&_span]:text-[12px]",
].join(" ");
const fieldErrorClassName = "text-[#b42318] text-[12px]";
const modalActionsClassName = "flex justify-end gap-[8px]";
const modalWarningClassName = "m-0 text-[#475467] text-[13px]";
const dangerActionClassName = [
  "border-[#f6c2bd] bg-[#fff4f2] text-[#b42318]",
  "hover:border-[#f04438] hover:bg-[#fff2f0] hover:text-[#912018]",
].join(" ");

interface MachineModalProps {
  baseUrl: string;
  configNameDraft: string;
  configNameInputRef: React.RefObject<HTMLInputElement | null>;
  configUrlDraft: string;
  connection: ConnectionState;
  isPairing: boolean;
  machineCount: number;
  machineFormError: string;
  machineName: string;
  machineNameInputRef: React.RefObject<HTMLInputElement | null>;
  mode: MachineModalMode;
  modalTitle: string;
  pairingCode: string;
  pairingCodeInputRef: React.RefObject<HTMLInputElement | null>;
  selected?: Machine;
  onAddMachine: (event: FormEvent<HTMLFormElement>) => void;
  onBaseUrlChange: (value: string) => void;
  onClose: () => void;
  onConfigNameChange: (value: string) => void;
  onConfigUrlChange: (value: string) => void;
  onDeleteSelectedMachine: () => void;
  onMachineNameChange: (value: string) => void;
  onPairingCodeChange: (value: string) => void;
  onPairSelected: (event: FormEvent<HTMLFormElement>) => void;
  onSaveMachineConfig: (event: FormEvent<HTMLFormElement>) => void;
}

interface PairMachineFormProps {
  connection: ConnectionState;
  isPairing: boolean;
  pairingCode: string;
  pairingCodeInputRef: React.RefObject<HTMLInputElement | null>;
  selected: Machine;
  onClose: () => void;
  onPairingCodeChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}

interface MachineConfigFormProps {
  configNameDraft: string;
  configNameInputRef: React.RefObject<HTMLInputElement | null>;
  configUrlDraft: string;
  error: string;
  selected: Machine;
  onClose: () => void;
  onConfigNameChange: (value: string) => void;
  onConfigUrlChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}

interface DeleteMachineFormProps {
  selected: Machine;
  onClose: () => void;
  onDelete: () => void;
}

export function MachineModal(
  {
    baseUrl,
    configNameDraft,
    configNameInputRef,
    configUrlDraft,
    connection,
    isPairing,
    machineCount,
    machineFormError,
    machineName,
    machineNameInputRef,
    mode,
    modalTitle,
    pairingCode,
    pairingCodeInputRef,
    selected,
    onAddMachine,
    onBaseUrlChange,
    onClose,
    onConfigNameChange,
    onConfigUrlChange,
    onDeleteSelectedMachine,
    onMachineNameChange,
    onPairingCodeChange,
    onPairSelected,
    onSaveMachineConfig,
  }: MachineModalProps,
) {
  return (
    <div
      className={modalBackdropClassName}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <section
        className={machineModalClassName}
        role="dialog"
        aria-modal="true"
        aria-labelledby="machine-modal-title"
      >
        <header className={modalHeadClassName}>
          <div>
            <span>Machine</span>
            <h2 id="machine-modal-title">{modalTitle}</h2>
          </div>
          {machineCount > 0
            ? (
              <button
                type="button"
                onClick={onClose}
                title="Close"
                aria-label="Close machine modal"
                className={iconButtonClassName}
              >
                <X size={16} />
              </button>
            )
            : null}
        </header>

        {mode === "pair" && selected
          ? (
            <PairMachineForm
              connection={connection}
              isPairing={isPairing}
              pairingCode={pairingCode}
              pairingCodeInputRef={pairingCodeInputRef}
              selected={selected}
              onClose={onClose}
              onPairingCodeChange={onPairingCodeChange}
              onSubmit={onPairSelected}
            />
          )
          : mode === "config" && selected
          ? (
            <MachineConfigForm
              configNameDraft={configNameDraft}
              configNameInputRef={configNameInputRef}
              configUrlDraft={configUrlDraft}
              error={machineFormError}
              selected={selected}
              onClose={onClose}
              onConfigNameChange={onConfigNameChange}
              onConfigUrlChange={onConfigUrlChange}
              onSubmit={onSaveMachineConfig}
            />
          )
          : mode === "delete" && selected
          ? (
            <DeleteMachineForm
              selected={selected}
              onClose={onClose}
              onDelete={onDeleteSelectedMachine}
            />
          )
          : (
            <AddMachineForm
              baseUrl={baseUrl}
              error={machineFormError}
              machineName={machineName}
              machineNameInputRef={machineNameInputRef}
              showCancel
              onBaseUrlChange={onBaseUrlChange}
              onCancel={onClose}
              onMachineNameChange={onMachineNameChange}
              onSubmit={onAddMachine}
            />
          )}
      </section>
    </div>
  );
}

function PairMachineForm(
  {
    connection,
    isPairing,
    pairingCode,
    pairingCodeInputRef,
    selected,
    onClose,
    onPairingCodeChange,
    onSubmit,
  }: PairMachineFormProps,
) {
  return (
    <form className={machineModalFormClassName} onSubmit={onSubmit}>
      <div className={modalMachineSummaryClassName}>
        <strong>{selected.name}</strong>
        <span>{selected.baseUrl}</span>
      </div>
      <label>
        <span>Pairing code</span>
        <input
          ref={pairingCodeInputRef}
          value={pairingCode}
          onChange={(event) =>
            onPairingCodeChange(
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
        ? <div className={fieldErrorClassName}>{connection.message}</div>
        : null}
      <div className={modalActionsClassName}>
        <button type="button" onClick={onClose}>
          Skip
        </button>
        <button
          type="submit"
          disabled={isPairing || pairingCode.length === 0}
        >
          {isPairing
            ? <Loader2 size={16} className="animate-spin" />
            : <KeyRound size={16} />}
          Pair
        </button>
      </div>
    </form>
  );
}

function MachineConfigForm(
  {
    configNameDraft,
    configNameInputRef,
    configUrlDraft,
    error,
    selected,
    onClose,
    onConfigNameChange,
    onConfigUrlChange,
    onSubmit,
  }: MachineConfigFormProps,
) {
  return (
    <form
      className={machineModalFormClassName}
      onSubmit={onSubmit}
    >
      <div className={modalMachineSummaryClassName}>
        <strong>{selected.name}</strong>
        <span>{selected.baseUrl}</span>
      </div>
      <label>
        <span>Name</span>
        <input
          ref={configNameInputRef}
          value={configNameDraft}
          onChange={(event) => onConfigNameChange(event.target.value)}
          placeholder="Machine name"
          aria-label="Machine name"
        />
      </label>
      <label>
        <span>URL</span>
        <input
          value={configUrlDraft}
          onChange={(event) => onConfigUrlChange(event.target.value)}
          placeholder="https://host:9012"
          aria-label="Machine URL"
        />
      </label>
      {error ? <div className={fieldErrorClassName}>{error}</div> : null}
      <div className={modalActionsClassName}>
        <button type="button" onClick={onClose}>
          Cancel
        </button>
        <button type="submit">
          <Settings size={16} />
          Save
        </button>
      </div>
    </form>
  );
}

function DeleteMachineForm(
  {
    selected,
    onClose,
    onDelete,
  }: DeleteMachineFormProps,
) {
  return (
    <div className={machineModalFormClassName}>
      <div className={modalMachineSummaryClassName}>
        <strong>{selected.name}</strong>
        <span>{selected.baseUrl}</span>
      </div>
      <p className={modalWarningClassName}>
        This removes the machine from this browser.
      </p>
      <div className={modalActionsClassName}>
        <button type="button" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className={dangerActionClassName}
          onClick={onDelete}
        >
          <Trash2 size={16} />
          Delete
        </button>
      </div>
    </div>
  );
}
