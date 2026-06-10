import { X } from "lucide-react";
import { FsEntry } from "../../../../../../protocol/rpc.ts";
import {
  displayName,
  formatDate,
  formatSize,
  kindLabel,
} from "../../../../../../state/explorer.ts";

interface InspectorProps {
  entry?: FsEntry;
  currentPath?: string;
}

interface EntryPropertiesModalProps {
  entry: FsEntry;
  onClose: () => void;
}

interface EntryDetailsProps {
  entry?: FsEntry;
  currentPath?: string;
}

export function Inspector(
  { entry, currentPath }: InspectorProps,
) {
  return (
    <aside className="inspector">
      <div className="inspector-title">Selection</div>
      <EntryDetails entry={entry} currentPath={currentPath} />
    </aside>
  );
}

export function EntryPropertiesModal(
  { entry, onClose }: EntryPropertiesModalProps,
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
  { entry, currentPath }: EntryDetailsProps,
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
