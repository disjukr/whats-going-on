import { bunja } from "bunja";
import { terminalShellsBunja } from "../../../../state/terminal-shells.ts";

export const filesToolBunja = bunja(() => {
  const terminalShells = bunja.use(terminalShellsBunja);

  return {
    defaultShellAtom: terminalShells.defaultShellAtom,
    terminalShellsAtom: terminalShells.terminalShellsAtom,
  };
});
