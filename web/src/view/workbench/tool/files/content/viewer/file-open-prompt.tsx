import { FileText } from "lucide-react";
import { FsEntry } from "../../../../../../protocol/rpc.ts";
import { displayName, formatSize } from "../../../../../../state/explorer.ts";

interface FileOpenPromptProps {
  file: FsEntry;
  onCancel: () => void;
  onConfirm: () => void;
}

export function FileOpenPrompt(
  { file, onCancel, onConfirm }: FileOpenPromptProps,
) {
  const sizeLabel = file.size === undefined
    ? "Unknown size"
    : formatSize(file.size);

  return (
    <section className="file-open-prompt">
      <div className="file-open-prompt-panel">
        <div className="file-open-prompt-icon">
          <FileText size={24} />
        </div>
        <div className="file-open-prompt-copy">
          <h2>{displayName(file)}</h2>
          <p>
            This file is <strong>{sizeLabel}</strong>. Do you want to open it?
          </p>
          <p>{file.path}</p>
        </div>
        <div className="file-open-prompt-actions">
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" onClick={onConfirm}>
            <FileText size={16} />
            Open
          </button>
        </div>
      </div>
    </section>
  );
}
