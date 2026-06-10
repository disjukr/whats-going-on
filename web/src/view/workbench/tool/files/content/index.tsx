import { useContext } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { machineStoreBunja } from "../../../../../state/machine-store.ts";
import {
  FilesActionsContext,
  FilesExplorerContext,
  requireFilesActions,
  requireFilesExplorer,
} from "../context.tsx";
import { DirectoryContent } from "./directory/index.tsx";
import { FileOpenPrompt } from "./viewer/file-open-prompt.tsx";
import { FileViewer } from "./viewer/index.tsx";

export function FilesContent() {
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const machineStore = useBunja(machineStoreBunja);
  const machine = useAtomValue(machineStore.selectedAtom);
  const fileOpenPrompt = useAtomValue(explorer.fileOpenPromptAtom);
  const openedFile = useAtomValue(explorer.openedFileAtom);

  if (fileOpenPrompt) {
    return (
      <FileOpenPrompt
        file={fileOpenPrompt}
        onCancel={actions.cancelFileOpen}
        onConfirm={actions.confirmFileOpen}
      />
    );
  }

  if (openedFile && machine) {
    return <FileViewer machine={machine} file={openedFile} />;
  }

  return <DirectoryContent />;
}
