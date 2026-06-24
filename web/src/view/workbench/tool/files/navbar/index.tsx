import { useContext } from "react";
import { ArrowLeft, ArrowUp } from "lucide-react";
import { useAtomValue } from "jotai";
import {
  FilesActionsContext,
  FilesExplorerContext,
  requireFilesActions,
  requireFilesExplorer,
} from "../context.tsx";
import { className } from "../../../../class-name.ts";
import { Button } from "../../../../ui/button.tsx";
import { PathCrumbs } from "./path-crumbs.tsx";

const pathToolbarClassName = [
  "grid [grid-template-columns:auto_minmax(0,1fr)] gap-[4px]",
  "relative items-center h-[2em] min-h-[2em] box-border overflow-visible",
  "border-b border-b-[#d8dde7] bg-white px-0 py-0 leading-[1.6]",
].join(" ");
const pathToolbarButtonGroupClassName =
  "inline-flex h-[2rem] items-center box-border p-[2px]";
const pathToolbarButtonClassName = [
  "!w-[2em] !min-w-[2em] !h-full !min-h-0 !box-border !p-0",
].join(" ");
const pathToolbarButtonFirstClassName = "!rounded-l-[4px] !rounded-r-0";
const pathToolbarButtonLastClassName = "-ml-px !rounded-l-0 !rounded-r-[4px]";

export function FilesNavbar() {
  const explorer = requireFilesExplorer(useContext(FilesExplorerContext));
  const actions = requireFilesActions(useContext(FilesActionsContext));
  const currentPath = useAtomValue(explorer.currentPathAtom);
  const displayPath = useAtomValue(explorer.displayPathAtom);
  const history = useAtomValue(explorer.historyAtom);
  const canGoBack = history.length > 0;
  const canGoUp = currentPath !== undefined;

  return (
    <div className={pathToolbarClassName}>
      <div className={pathToolbarButtonGroupClassName}>
        <Button
          onClick={actions.goBack}
          disabled={!canGoBack}
          title="Back"
          aria-label="Back"
          className={className(
            pathToolbarButtonClassName,
            pathToolbarButtonFirstClassName,
          )}
        >
          <ArrowLeft size={12} />
        </Button>
        <Button
          onClick={actions.goUp}
          disabled={!canGoUp}
          title="Up"
          aria-label="Up"
          className={className(
            pathToolbarButtonClassName,
            pathToolbarButtonLastClassName,
          )}
        >
          <ArrowUp size={12} />
        </Button>
      </div>
      <PathCrumbs path={displayPath} onNavigate={actions.navigate} />
    </div>
  );
}
