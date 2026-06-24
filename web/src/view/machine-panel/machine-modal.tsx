import React, { FormEvent } from "react";
import {
  KeyRound,
  Loader2,
  RefreshCw,
  Settings,
  Trash2,
  X,
} from "lucide-react";
import type { Machine } from "../../state/machines.ts";
import type { MachineModalMode } from "../../state/types.ts";
import { Button } from "../ui/button.tsx";
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
const iconButtonClassName = "!w-[36px] !min-w-[36px] !p-0";
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
const pairingControlClassName = [
  "flex items-center justify-between gap-[12px] rounded-[8px]",
  "border border-[#d8dde7] bg-[#f7f8fb] px-[10px] py-[8px]",
].join(" ");
const pairingStepClassName = [
  "grid min-h-[174px] place-items-center gap-[10px] rounded-[8px]",
  "border border-[#d8dde7] bg-white px-[16px] py-[18px] text-center",
].join(" ");
const confirmationCodeClassName = [
  "grid h-[96px] min-w-[156px] place-items-center rounded-[8px]",
  "border border-[#cfd6e3] bg-[#f7f8fb] px-[18px]",
  "font-800 text-[#20242d] text-[56px] leading-none tracking-[0]",
].join(" ");

interface MachineModalProps {
  baseUrl: string;
  configNameDraft: string;
  configNameInputRef: React.RefObject<HTMLInputElement | null>;
  configUrlDraft: string;
  isRequestingPairingCode: boolean;
  isPairing: boolean;
  machineCount: number;
  machineFormError: string;
  machineName: string;
  machineNameInputRef: React.RefObject<HTMLInputElement | null>;
  mode: MachineModalMode;
  modalTitle: string;
  pairingCode: string;
  pairingConfirmationCode?: string;
  pairingCodeExpiresInSeconds?: number;
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
  onRequestPairingCode: () => void;
  onSaveMachineConfig: (event: FormEvent<HTMLFormElement>) => void;
}

interface PairMachineFormProps {
  isRequestingPairingCode: boolean;
  isPairing: boolean;
  pairingCode: string;
  pairingConfirmationCode?: string;
  pairingCodeExpiresInSeconds?: number;
  pairingCodeInputRef: React.RefObject<HTMLInputElement | null>;
  selected: Machine;
  onClose: () => void;
  onPairingCodeChange: (value: string) => void;
  onRequestPairingCode: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}

interface PairingRestartControlProps {
  disabled: boolean;
  machineName: string;
  onRequestPairingCode: () => void;
}

interface PairingConfirmationStepProps {
  confirmationCode?: string;
  isRequestingPairingCode: boolean;
}

interface PairingCodeStepProps {
  pairingCode: string;
  pairingCodeExpiresInSeconds?: number;
  pairingCodeInputRef: React.RefObject<HTMLInputElement | null>;
  onPairingCodeChange: (value: string) => void;
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
    isRequestingPairingCode,
    isPairing,
    machineCount,
    machineFormError,
    machineName,
    machineNameInputRef,
    mode,
    modalTitle,
    pairingCode,
    pairingConfirmationCode,
    pairingCodeExpiresInSeconds,
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
    onRequestPairingCode,
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
              <Button
                onClick={onClose}
                title="Close"
                aria-label="Close machine modal"
                className={iconButtonClassName}
              >
                <X size={16} />
              </Button>
            )
            : null}
        </header>

        {mode === "pair" && selected
          ? (
            <PairMachineForm
              isRequestingPairingCode={isRequestingPairingCode}
              isPairing={isPairing}
              pairingCode={pairingCode}
              pairingConfirmationCode={pairingConfirmationCode}
              pairingCodeExpiresInSeconds={pairingCodeExpiresInSeconds}
              pairingCodeInputRef={pairingCodeInputRef}
              selected={selected}
              onClose={onClose}
              onPairingCodeChange={onPairingCodeChange}
              onRequestPairingCode={onRequestPairingCode}
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
    isRequestingPairingCode,
    isPairing,
    pairingCode,
    pairingConfirmationCode,
    pairingCodeExpiresInSeconds,
    pairingCodeInputRef,
    selected,
    onClose,
    onPairingCodeChange,
    onRequestPairingCode,
    onSubmit,
  }: PairMachineFormProps,
) {
  const hasPairingCode = pairingCodeExpiresInSeconds !== undefined;
  return (
    <form className={machineModalFormClassName} onSubmit={onSubmit}>
      <PairingRestartControl
        disabled={isPairing}
        machineName={selected.name}
        onRequestPairingCode={onRequestPairingCode}
      />
      {hasPairingCode
        ? (
          <PairingCodeStep
            pairingCode={pairingCode}
            pairingCodeExpiresInSeconds={pairingCodeExpiresInSeconds}
            pairingCodeInputRef={pairingCodeInputRef}
            onPairingCodeChange={onPairingCodeChange}
          />
        )
        : (
          <PairingConfirmationStep
            confirmationCode={pairingConfirmationCode}
            isRequestingPairingCode={isRequestingPairingCode}
          />
        )}
      <div className={modalActionsClassName}>
        <Button onClick={onClose}>
          Skip
        </Button>
        {hasPairingCode
          ? (
            <Button
              type="submit"
              disabled={isPairing || isRequestingPairingCode ||
                pairingCode.length === 0}
            >
              {isPairing
                ? <Loader2 size={16} className="animate-spin" />
                : <KeyRound size={16} />}
              Pair
            </Button>
          )
          : null}
      </div>
    </form>
  );
}

function PairingRestartControl(
  {
    disabled,
    machineName,
    onRequestPairingCode,
  }: PairingRestartControlProps,
) {
  return (
    <div className={pairingControlClassName}>
      <span className="flex min-w-0 items-center gap-[4px] text-[#475467] text-[12px]">
        <span className="shrink-0">Pairing to</span>
        <strong className="min-w-0 truncate font-700 text-[#20242d]">
          {machineName}
        </strong>
      </span>
      <Button
        className="min-w-[36px] px-[10px]"
        disabled={disabled}
        onClick={onRequestPairingCode}
        title="Restart pairing"
      >
        <RefreshCw size={16} />
        Restart pairing
      </Button>
    </div>
  );
}

function PairingConfirmationStep(
  { confirmationCode, isRequestingPairingCode }: PairingConfirmationStepProps,
) {
  return (
    <section className={pairingStepClassName}>
      <div className="grid justify-items-center gap-[8px]">
        <span className="text-[#475467] text-[12px] font-700">
          Confirmation code
        </span>
        <div className={confirmationCodeClassName}>
          {confirmationCode ?? "--"}
        </div>
      </div>
      <p className="m-0 max-w-[320px] text-[#667085] text-[13px] leading-[1.45]">
        Select this code on the daemon to reveal the pairing code.
      </p>
      {isRequestingPairingCode
        ? (
          <div className="flex items-center gap-[6px] text-[#475467] text-[12px]">
            <Loader2 size={14} className="animate-spin" />
            Waiting for confirmation
          </div>
        )
        : null}
    </section>
  );
}

function PairingCodeStep(
  {
    pairingCode,
    pairingCodeExpiresInSeconds,
    pairingCodeInputRef,
    onPairingCodeChange,
  }: PairingCodeStepProps,
) {
  return (
    <section className={pairingStepClassName}>
      <label className="w-full">
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
      {pairingCodeExpiresInSeconds !== undefined
        ? pairingCodeExpiresInSeconds <= 0
          ? (
            <p className="m-0 text-[#b42318] text-[12px]">
              Pairing code expired. Restart pairing.
            </p>
          )
          : (
            <p className="m-0 text-[#667085] text-[12px]">
              Pairing code expires in{" "}
              {formatRemainingTime(pairingCodeExpiresInSeconds)}.
            </p>
          )
        : null}
    </section>
  );
}

function formatRemainingTime(totalSeconds: number): string {
  const seconds = Math.max(0, Math.ceil(totalSeconds));
  const minutesPart = Math.floor(seconds / 60).toString().padStart(2, "0");
  const secondsPart = (seconds % 60).toString().padStart(2, "0");
  return `${minutesPart}:${secondsPart}`;
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
        <Button onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit">
          <Settings size={16} />
          Save
        </Button>
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
        <Button onClick={onClose}>
          Cancel
        </Button>
        <Button
          className={dangerActionClassName}
          onClick={onDelete}
        >
          <Trash2 size={16} />
          Delete
        </Button>
      </div>
    </div>
  );
}
