import React, { FormEvent } from "react";
import { Plus } from "lucide-react";

const machineModalFormClassName = [
  "grid gap-[12px] p-[16px]",
  "[&_label]:grid [&_label]:gap-[6px] [&_label]:min-w-0",
  "[&_label_span]:text-[#475467] [&_label_span]:text-[12px] [&_label_span]:font-700",
].join(" ");
const fieldErrorClassName = "text-[#b42318] text-[12px]";
const modalActionsClassName = "flex justify-end gap-[8px]";

interface AddMachineFormProps {
  baseUrl: string;
  error: string;
  machineName: string;
  machineNameInputRef: React.RefObject<HTMLInputElement | null>;
  showCancel: boolean;
  onBaseUrlChange: (value: string) => void;
  onCancel: () => void;
  onMachineNameChange: (value: string) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
}

export function AddMachineForm(
  {
    baseUrl,
    error,
    machineName,
    machineNameInputRef,
    showCancel,
    onBaseUrlChange,
    onCancel,
    onMachineNameChange,
    onSubmit,
  }: AddMachineFormProps,
) {
  return (
    <form className={machineModalFormClassName} onSubmit={onSubmit}>
      <label>
        <span>Name</span>
        <input
          ref={machineNameInputRef}
          value={machineName}
          onChange={(event) => onMachineNameChange(event.target.value)}
          placeholder="Local daemon"
          aria-label="Machine name"
        />
      </label>
      <label>
        <span>URL</span>
        <input
          value={baseUrl}
          onChange={(event) => onBaseUrlChange(event.target.value)}
          placeholder="https://host:9012"
          aria-label="Machine URL"
        />
      </label>
      {error ? <div className={fieldErrorClassName}>{error}</div> : null}
      <div className={modalActionsClassName}>
        {showCancel
          ? (
            <button type="button" onClick={onCancel}>
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
