import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { workbenchTabBunja } from "../../../state/workbench.ts";
import { FilesTool } from "./files/index.tsx";

export function WorkbenchToolContent() {
  const tabState = useBunja(workbenchTabBunja);
  const tab = useAtomValue(tabState.tabAtom);

  if (!tab) return null;
  if (tab.tool === "files") {
    return <FilesTool />;
  }
}
