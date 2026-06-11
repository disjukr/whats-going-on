import { useBunja } from "bunja/react";
import { FileText } from "lucide-react";
import { displayName, formatSize } from "../../../../../../state/explorer.ts";
import { fileViewerBunja } from "./state.tsx";

interface BigFileWarningProps {
  onCancel: () => void;
  onConfirm: () => void;
  viewerName: string;
}

export function BigFileWarning(
  { onCancel, onConfirm, viewerName }: BigFileWarningProps,
) {
  const viewer = useBunja(fileViewerBunja);
  const fsEntry = viewer.fsEntry;
  const sizeLabel = fsEntry.size === undefined
    ? "Unknown size"
    : formatSize(fsEntry.size);

  return (
    <section className="big-file-warning">
      <div className="big-file-warning-panel">
        <div className="big-file-warning-icon">
          <FileText size={24} />
        </div>
        <div className="big-file-warning-copy">
          <h2>{displayName(fsEntry)}</h2>
          <p>
            This file is <strong>{sizeLabel}</strong>. Do you want to open it
            {" "}
            with the {viewerName}?
          </p>
          <p>{fsEntry.path}</p>
        </div>
        <div className="big-file-warning-actions">
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
