import { useContext } from "react";
import { useAtomValue } from "jotai";
import { formatSize } from "../../../../../state/explorer.ts";
import { FilesExplorerContext, requireFilesExplorer } from "../context.tsx";

export function FilesFooter() {
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const fileOpenPrompt = useAtomValue(explorer.fileOpenPromptAtom);
  const openedFile = useAtomValue(explorer.openedFileAtom);
  const rows = useAtomValue(explorer.visibleRowsAtom);
  const label = fileOpenPrompt
    ? formatSize(fileOpenPrompt.size)
    : openedFile
    ? formatSize(openedFile.size)
    : `${rows.length} items`;

  return (
    <div className="explorer-footer">
      <span>{label}</span>
    </div>
  );
}
