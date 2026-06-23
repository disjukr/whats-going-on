import React from "react";
import { FsEntry } from "../../../../../../protocol/rpc.ts";
import {
  displayName,
  formatDate,
  formatSize,
  kindLabel,
} from "../../../../../../state/explorer.ts";
import { EntryIcon } from "./entry-icon.tsx";
import { className } from "../../../../../class-name.ts";

interface FileTableProps {
  rows: FsEntry[];
  selectedPath?: string;
  onSelect: (entry: FsEntry) => void;
  onOpen: (entry: FsEntry) => void;
  onContextMenu: (
    entry: FsEntry,
    event: React.MouseEvent<HTMLButtonElement>,
  ) => void;
  onFolderContextMenu: (event: React.MouseEvent<HTMLDivElement>) => void;
}

const fileTableClassName = [
  "file-table grid",
  "[grid-template-columns:minmax(220px,1fr)_minmax(96px,130px)_minmax(88px,120px)_minmax(140px,190px)]",
  "[@container_workbench-tab-page_(max-width:680px)]:[grid-template-columns:minmax(200px,1fr)_96px_88px]",
  "auto-rows-[1.6em] min-w-0 min-h-0 overflow-auto bg-white leading-[1.6]",
].join(" ");
const hideInNarrowContainerClassName =
  "[@container_workbench-tab-page_(max-width:680px)]:hidden";
const fileHeadClassName = [
  "file-head sticky top-0 z-[1] flex items-center h-[1.6rem] box-border",
  "border-b border-b-[#d8dde7] bg-[#f6f8fb] text-[#667085] text-[0.8rem] font-700 px-[8px]",
].join(" ");
const fileRowClassName = [
  "grid [grid-column:1/-1] [grid-template-columns:subgrid]",
  "h-[1.6em] min-h-[1.6em] box-border border-0 border-b border-b-[#eef1f5] rounded-0",
  "appearance-none cursor-pointer bg-white p-0 text-left leading-[1.6] [font-family:inherit] hover:bg-[#f7faff]",
  "[&.selected]:bg-[#eaf3ff]",
].join(" ");
const fileCellBaseClassName = [
  "file-cell flex items-center min-w-0 overflow-hidden text-[#303642]",
  "px-[8px] text-ellipsis whitespace-nowrap",
].join(" ");
const fileNameCellClassName = `${fileCellBaseClassName} name gap-[6px]`;
const fileMetaCellClassName = `${fileCellBaseClassName} text-[#667085]`;
const fileNameClassName =
  "min-w-0 overflow-hidden text-ellipsis whitespace-nowrap leading-[1.6]";
const readonlyClassName = [
  "flex-[0_0_auto] border border-[#e4c778] rounded-full bg-[#fff8df]",
  "text-[#8a6116] px-[4px] py-0 leading-[1]",
].join(" ");
const tableEmptyClassName =
  "[grid-column:1/-1] flex items-center text-[#667085] px-[12px]";
const tableBottomPaddingClassName = "[grid-column:1/-1] h-[8rem]";

export function FileTable(
  {
    rows,
    selectedPath,
    onSelect,
    onOpen,
    onContextMenu,
    onFolderContextMenu,
  }: FileTableProps,
) {
  function openFolderContextMenu(event: React.MouseEvent<HTMLDivElement>) {
    const target = event.target;
    if (!(target instanceof Element)) return;
    if (target.closest("[data-file-table-row], [data-file-table-head]")) {
      return;
    }
    onFolderContextMenu(event);
  }

  return (
    <div
      className={fileTableClassName}
      role="grid"
      aria-label="Files"
      onContextMenu={openFolderContextMenu}
    >
      <div className={`${fileHeadClassName} name`} data-file-table-head>
        Name
      </div>
      <div className={`${fileHeadClassName} kind`} data-file-table-head>
        Kind
      </div>
      <div className={`${fileHeadClassName} size`} data-file-table-head>
        Size
      </div>
      <div
        className={className(
          fileHeadClassName,
          "modified",
          hideInNarrowContainerClassName,
        )}
        data-file-table-head
      >
        Modified
      </div>
      {rows.length === 0
        ? <div className={tableEmptyClassName}>No rows</div>
        : (
          rows.map((entry) => (
            <button
              type="button"
              key={entry.path}
              className={className(
                fileRowClassName,
                entry.path === selectedPath && "selected",
              )}
              onClick={() => onSelect(entry)}
              onDoubleClick={() => onOpen(entry)}
              onContextMenu={(event) => onContextMenu(entry, event)}
              data-file-table-row
            >
              <span className={fileNameCellClassName}>
                <EntryIcon entry={entry} />
                <span className={fileNameClassName}>{displayName(entry)}</span>
                {entry.readonly
                  ? <span className={readonlyClassName}>readonly</span>
                  : null}
              </span>
              <span className={`${fileMetaCellClassName} kind`}>
                {kindLabel(entry.kind)}
              </span>
              <span className={`${fileMetaCellClassName} size`}>
                {formatSize(entry.size)}
              </span>
              <span
                className={className(
                  fileMetaCellClassName,
                  "modified",
                  hideInNarrowContainerClassName,
                )}
              >
                {formatDate(entry.modifiedAtMs)}
              </span>
            </button>
          ))
        )}
      <div className={tableBottomPaddingClassName} aria-hidden="true" />
    </div>
  );
}
