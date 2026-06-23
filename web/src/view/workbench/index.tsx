import { bunja } from "bunja";
import { atom, useAtomValue } from "jotai";
import { useBunja } from "bunja/react";
import { machineStoreBunja } from "../../state/machine-store.ts";
import { MachineAddFormContainer } from "../machine-panel/index.tsx";
import { WorkbenchPaneLayout } from "./pane-layout/index.tsx";
import { PropsWithChildren } from "react";
import { className } from "../class-name.ts";

const workbenchClassName = [
  "workbench [grid-column:3] [grid-row:2]",
  "min-w-0 min-h-0 overflow-hidden bg-white",
  "[transition:border-radius_180ms_ease]",
].join(" ");
const inlineMachineSetupClassName = [
  "grid content-center justify-items-center min-h-0 overflow-auto p-[24px]",
].join(" ");
const inlineMachineCardClassName = [
  "w-[min(460px,100%)] overflow-hidden border border-[#d8dde7]",
  "rounded-[8px] bg-white",
].join(" ");
const modalHeadClassName = [
  "flex items-center justify-between gap-[12px] border-b border-b-[#e4e8ef]",
  "px-[16px] py-[14px]",
  "[&_div]:grid [&_div]:gap-[2px] [&_div]:min-w-0",
  "[&_span]:text-[#667085] [&_span]:text-[12px] [&_span]:font-700",
  "[&_h2]:m-0 [&_h2]:text-[#20242d] [&_h2]:text-[18px] [&_h2]:tracking-[0]",
].join(" ");

const workbenchRegionBunja = bunja(() => {
  const { machinesAtom } = bunja.use(machineStoreBunja);
  const hasMachinesAtom = atom((get) => get(machinesAtom).length > 0);
  return { hasMachinesAtom };
});

export function WorkbenchRegion() {
  const { hasMachinesAtom } = useBunja(workbenchRegionBunja);
  const hasMachines = useAtomValue(hasMachinesAtom);
  return (
    <section
      className={className(workbenchClassName, !hasMachines && "grid")}
    >
      {hasMachines ? <WorkbenchPaneLayout /> : (
        <InlineMachineSetup>
          <MachineAddFormContainer showCancel={false} />
        </InlineMachineSetup>
      )}
    </section>
  );
}

function InlineMachineSetup({ children }: PropsWithChildren) {
  return (
    <section className={inlineMachineSetupClassName}>
      <div className={inlineMachineCardClassName}>
        <header className={modalHeadClassName}>
          <div>
            <span>Machine</span>
            <h2>Add machine</h2>
          </div>
        </header>
        {children}
      </div>
    </section>
  );
}
