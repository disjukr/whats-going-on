import { useContext } from "react";
import { useAtomValue } from "jotai";
import { FilesExplorerContext, requireFilesExplorer } from "../context.tsx";
import { DirectoryContent } from "./directory/index.tsx";
import { FileViewer } from "./file-viewer/index.tsx";
import { FsEntryContext } from "./file-viewer/state.tsx";

export function FilesContent() {
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const openedFile = useAtomValue(explorer.openedFileAtom);

  if (openedFile) {
    return (
      <FsEntryContext value={openedFile}>
        <FileViewer />
      </FsEntryContext>
    );
  }

  return <DirectoryContent />;
}
