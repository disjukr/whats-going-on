import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import { bunja } from "bunja";
import { atom } from "jotai";
import { JotaiStoreScope } from "unsaturated/store";

const machinePanelMinWidth = 212;
const machinePanelMaxWidth = 420;
const machinePanelTransitionMs = 180;
const minimumWorkbenchWidth = 360;
const machineRailWidth = 64;

export const layoutBunja = bunja(() => {
  const store = bunja.use(JotaiStoreScope);
  const machinePanelWidthAtom = atom(264);
  const machinePanelCollapsedAtom = atom(false);
  const machinePanelTransitioningAtom = atom(false);
  let machinePanelTransitionTimeout:
    | ReturnType<typeof globalThis.setTimeout>
    | undefined;

  function clampMachinePanelWidth(width: number) {
    const maxByViewport = Math.max(
      machinePanelMinWidth,
      globalThis.innerWidth - machineRailWidth - minimumWorkbenchWidth,
    );
    return Math.round(
      Math.min(
        Math.max(width, machinePanelMinWidth),
        Math.min(machinePanelMaxWidth, maxByViewport),
      ),
    );
  }

  function startMachinePanelResize(
    event: ReactPointerEvent<HTMLDivElement>,
  ) {
    event.preventDefault();
    store.set(machinePanelTransitioningAtom, false);
    const initialX = event.clientX;
    const initialWidth = store.get(machinePanelWidthAtom);
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;

    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";

    function resize(moveEvent: PointerEvent) {
      store.set(
        machinePanelWidthAtom,
        clampMachinePanelWidth(
          initialWidth + moveEvent.clientX - initialX,
        ),
      );
    }

    function stopResize() {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
      globalThis.removeEventListener("pointermove", resize);
      globalThis.removeEventListener("pointerup", stopResize);
      globalThis.removeEventListener("pointercancel", stopResize);
    }

    globalThis.addEventListener("pointermove", resize);
    globalThis.addEventListener("pointerup", stopResize);
    globalThis.addEventListener("pointercancel", stopResize);
  }

  function resizeMachinePanelWithKeyboard(
    event: ReactKeyboardEvent<HTMLDivElement>,
  ) {
    const step = event.shiftKey ? 40 : 16;
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      store.set(machinePanelTransitioningAtom, false);
      store.set(
        machinePanelWidthAtom,
        (width) => clampMachinePanelWidth(width - step),
      );
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      store.set(machinePanelTransitioningAtom, false);
      store.set(
        machinePanelWidthAtom,
        (width) => clampMachinePanelWidth(width + step),
      );
    }
    if (event.key === "Home") {
      event.preventDefault();
      store.set(machinePanelTransitioningAtom, false);
      store.set(machinePanelWidthAtom, machinePanelMinWidth);
    }
    if (event.key === "End") {
      event.preventDefault();
      store.set(machinePanelTransitioningAtom, false);
      store.set(
        machinePanelWidthAtom,
        (width) =>
          clampMachinePanelWidth(Math.max(width, machinePanelMaxWidth)),
      );
    }
  }

  function toggleMachinePanel() {
    if (machinePanelTransitionTimeout !== undefined) {
      globalThis.clearTimeout(machinePanelTransitionTimeout);
    }
    store.set(machinePanelTransitioningAtom, true);
    store.set(machinePanelCollapsedAtom, (collapsed) => !collapsed);
    machinePanelTransitionTimeout = globalThis.setTimeout(() => {
      store.set(machinePanelTransitioningAtom, false);
      machinePanelTransitionTimeout = undefined;
    }, machinePanelTransitionMs);
  }

  return {
    machinePanelCollapsedAtom,
    machinePanelMaxWidth,
    machinePanelMinWidth,
    machinePanelTransitioningAtom,
    machinePanelWidthAtom,
    resizeMachinePanelWithKeyboard,
    startMachinePanelResize,
    toggleMachinePanel,
  };
});
