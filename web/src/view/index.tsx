import React, { PropsWithChildren } from "react";
import { useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import {
  MachineModalHost,
  MachinePanelRegion,
} from "./machine-panel/index.tsx";
import { MachineRailRegion } from "./machine-rail/index.tsx";
import { daemonInfoBunja } from "../state/daemon-info.ts";
import { MachineIdContext } from "../state/machine-id.tsx";
import { machineStoreBunja } from "../state/machine-store.ts";
import { layoutBunja } from "./state.tsx";
import { TopBarRegion } from "./top-bar/index.tsx";
import { WorkbenchRegion } from "./workbench/index.tsx";
import { className } from "./class-name.ts";

const appShellClassName = [
  "app-shell grid h-full min-h-0 overflow-hidden bg-[#242832]",
  "[grid-template-columns:48px_var(--machine-panel-width,264px)_minmax(0,1fr)]",
  "[grid-template-rows:auto_minmax(0,1fr)]",
  "[&.machine-panel-transitioning]:[transition:grid-template-columns_180ms_ease]",
  "max-[980px]:[grid-template-columns:48px_var(--machine-panel-width,236px)_minmax(0,1fr)]",
  "max-[680px]:[grid-template-columns:48px_var(--machine-panel-width,212px)_minmax(0,1fr)]",
  "[&.machine-panel-collapsed_.machine-panel]:pointer-events-none",
  "[&.machine-panel-collapsed_.machine-panel]:border-r-0",
  "[&.machine-panel-collapsed_.machine-panel]:rounded-tl-0",
  "[&.machine-panel-collapsed_.workbench]:rounded-tl-[8px]",
].join(" ");

export default function View() {
  useBunja(daemonInfoBunja);

  return (
    <Layout>
      <MachineRailRegion />
      <TopBarRegion />
      <SelectedMachineIdProvider>
        <MachinePanelRegion />
        <WorkbenchRegion />
      </SelectedMachineIdProvider>
      <MachineModalHost />
    </Layout>
  );
}

interface LayoutProps {
  children: React.ReactNode;
}
function Layout({ children }: LayoutProps) {
  const layout = useBunja(layoutBunja);
  const machinePanelCollapsed = useAtomValue(
    layout.machinePanelCollapsedAtom,
  );
  const machinePanelTransitioning = useAtomValue(
    layout.machinePanelTransitioningAtom,
  );
  const machinePanelWidth = useAtomValue(layout.machinePanelWidthAtom);

  return (
    <main
      className={className(
        appShellClassName,
        machinePanelCollapsed && "machine-panel-collapsed",
        machinePanelTransitioning && "machine-panel-transitioning",
      )}
      style={{
        "--machine-panel-width": machinePanelCollapsed
          ? "0px"
          : `${machinePanelWidth}px`,
        "--machine-panel-open-width": `${machinePanelWidth}px`,
      } as React.CSSProperties}
    >
      {children}
    </main>
  );
}

function SelectedMachineIdProvider(
  { children }: PropsWithChildren,
) {
  const machineStore = useBunja(machineStoreBunja);
  const selectedId = useAtomValue(machineStore.selectedIdAtom);
  return (
    <MachineIdContext value={selectedId}>
      {children}
    </MachineIdContext>
  );
}
