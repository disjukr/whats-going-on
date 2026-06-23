import { useContext } from "react";
import { useAtomValue } from "jotai";
import {
  FilesActionsContext,
  FilesExplorerContext,
  requireFilesActions,
  requireFilesExplorer,
} from "../../context.tsx";
import { FileTable } from "./file-table.tsx";
import { Inspector } from "./entry-details.tsx";

const directoryContentClassName = [
  "grid [grid-template-rows:minmax(0,1fr)_auto]",
  "w-full h-full min-w-0 min-h-0 overflow-hidden",
].join(" ");
const browserLayoutClassName = [
  "browser-layout grid [grid-template-columns:minmax(0,1fr)_minmax(220px,28%)]",
  "[@container_workbench-tab-page_(max-width:980px)]:[grid-template-columns:minmax(0,1fr)]",
  "h-full min-h-0 overflow-hidden",
].join(" ");
const explorerFooterClassName = [
  "flex items-center justify-end h-[1.6rem] min-h-[1.6rem] box-border",
  "border-t border-t-[#d8dde7] bg-[#fbfcfe] text-[#667085]",
  "px-[8px] text-[0.8rem] leading-[1.6]",
].join(" ");

export function DirectoryContent() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const currentPath = useAtomValue(explorer.currentPathAtom);
  const rows = useAtomValue(explorer.visibleRowsAtom);
  const selectedEntry = useAtomValue(explorer.selectedEntryAtom);
  const selectedPath = useAtomValue(explorer.selectedPathAtom);

  return (
    <div className={directoryContentClassName}>
      <div className={browserLayoutClassName}>
        <FileTable
          rows={rows}
          selectedPath={selectedPath}
          onSelect={actions.selectEntry}
          onOpen={actions.openEntry}
          onContextMenu={actions.openEntryMenu}
          onFolderContextMenu={actions.openFolderMenu}
        />
        <Inspector entry={selectedEntry} currentPath={currentPath} />
      </div>
      <DirectoryFooter />
    </div>
  );
}

export { EntryPropertiesModal } from "./entry-details.tsx";

function DirectoryFooter() {
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const rows = useAtomValue(explorer.visibleRowsAtom);

  return (
    <div className={explorerFooterClassName}>
      <span>{rows.length} items</span>
    </div>
  );
}
