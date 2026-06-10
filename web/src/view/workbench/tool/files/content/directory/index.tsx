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

export function DirectoryContent() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const currentPath = useAtomValue(explorer.currentPathAtom);
  const rows = useAtomValue(explorer.visibleRowsAtom);
  const selectedEntry = useAtomValue(explorer.selectedEntryAtom);
  const selectedPath = useAtomValue(explorer.selectedPathAtom);

  return (
    <div className="browser-layout">
      <FileTable
        rows={rows}
        selectedPath={selectedPath}
        onSelect={actions.selectEntry}
        onOpen={actions.openEntry}
        onContextMenu={actions.openEntryMenu}
      />
      <Inspector entry={selectedEntry} currentPath={currentPath} />
    </div>
  );
}

export { EntryPropertiesModal } from "./entry-details.tsx";
