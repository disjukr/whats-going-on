import { useContext } from "react";
import { ArrowLeft, ArrowUp } from "lucide-react";
import { useAtomValue } from "jotai";
import {
  FilesActionsContext,
  FilesExplorerContext,
  requireFilesActions,
  requireFilesExplorer,
} from "../context.tsx";
import { PathCrumbs } from "./path-crumbs.tsx";

export function FilesNavbar() {
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const currentPath = useAtomValue(explorer.currentPathAtom);
  const displayPath = useAtomValue(explorer.displayPathAtom);
  const fileOpenPrompt = useAtomValue(explorer.fileOpenPromptAtom);
  const history = useAtomValue(explorer.historyAtom);
  const path = fileOpenPrompt?.path ?? displayPath;
  const canGoBack = history.length > 0 || fileOpenPrompt !== undefined;
  const canGoUp = currentPath !== undefined;

  return (
    <div className="path-toolbar">
      <button
        type="button"
        onClick={actions.goBack}
        disabled={!canGoBack}
        title="Back"
        aria-label="Back"
        className="icon-button"
      >
        <ArrowLeft size={16} />
      </button>
      <button
        type="button"
        onClick={actions.goUp}
        disabled={!canGoUp}
        title="Up"
        aria-label="Up"
        className="icon-button"
      >
        <ArrowUp size={16} />
      </button>
      <PathCrumbs path={path} onNavigate={actions.navigate} />
    </div>
  );
}
