import React from "react";
import { FsEntry } from "../../../../../../protocol/rpc.ts";
import {
  displayName,
  formatDate,
  formatSize,
  kindLabel,
} from "../../../../../../state/explorer.ts";
import { EntryIcon } from "./entry-icon.tsx";

interface FileTableProps {
  rows: FsEntry[];
  selectedPath?: string;
  onSelect: (entry: FsEntry) => void;
  onOpen: (entry: FsEntry) => void;
  onContextMenu: (
    entry: FsEntry,
    event: React.MouseEvent<HTMLButtonElement>,
  ) => void;
}

export function FileTable(
  {
    rows,
    selectedPath,
    onSelect,
    onOpen,
    onContextMenu,
  }: FileTableProps,
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
