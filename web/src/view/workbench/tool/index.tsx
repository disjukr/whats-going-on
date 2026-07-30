import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { workbenchTabBunja } from "../../../state/workbench.ts";
import { AgentTool } from "./agent/index.tsx";
import { DaemonTool } from "./daemon/index.tsx";
import { FilesTool } from "./files/index.tsx";
import { ProcessesTool } from "./processes/index.tsx";
import { TerminalTool } from "./terminal/index.tsx";
import { WindowsTool } from "./windows/index.tsx";

export function WorkbenchToolContent() {
  const tabState = useBunja(workbenchTabBunja);
  const tab = useAtomValue(tabState.tabAtom);

  if (!tab) return null;
  if (tab.tool === "agent") {
    return <AgentTool />;
  }
  if (tab.tool === "daemon") {
    return <DaemonTool />;
  }
  if (tab.tool === "files") {
    return <FilesTool />;
  }
  if (tab.tool === "processes") {
    return <ProcessesTool />;
  }
  if (tab.tool === "terminal") {
    return <TerminalTool />;
  }
  if (tab.tool === "windows") {
    return <WindowsTool />;
  }
  return null;
}
