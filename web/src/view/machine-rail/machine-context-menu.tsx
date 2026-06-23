import { KeyRound, RefreshCw, Settings, Trash2, Unlink } from "lucide-react";
import type { Machine } from "../../state/machines.ts";
import { FloatingMenu, FloatingMenuItem } from "../ui/floating-menu.tsx";

export interface MachineMenuPosition {
  x: number;
  y: number;
}

interface MachineContextMenuProps {
  machine: Machine;
  menu: MachineMenuPosition;
  onConfigure: (machine: Machine) => void;
  onDelete: (machine: Machine) => void;
  onPair: (machine: Machine) => void;
  onReconnect: () => void;
  onUnpair: (machine: Machine) => void;
}

export function MachineContextMenu(
  {
    machine,
    menu,
    onConfigure,
    onDelete,
    onPair,
    onReconnect,
    onUnpair,
  }: MachineContextMenuProps,
) {
  const isPaired = Boolean(machine.clientId && machine.clientSecret);

  return (
    <FloatingMenu
      className="z-[30] w-[176px]"
      position={{ left: menu.x, top: menu.y }}
    >
      <FloatingMenuItem
        onClick={onReconnect}
      >
        <RefreshCw size={15} />
        Reconnect
      </FloatingMenuItem>
      <FloatingMenuItem
        onClick={() => onConfigure(machine)}
      >
        <Settings size={15} />
        Configure
      </FloatingMenuItem>
      {!isPaired
        ? (
          <FloatingMenuItem
            onClick={() => onPair(machine)}
          >
            <KeyRound size={15} />
            Pair
          </FloatingMenuItem>
        )
        : (
          <FloatingMenuItem
            onClick={() => onUnpair(machine)}
          >
            <Unlink size={15} />
            Unpair
          </FloatingMenuItem>
        )}
      <FloatingMenuItem
        danger
        onClick={() => onDelete(machine)}
      >
        <Trash2 size={15} />
        Delete
      </FloatingMenuItem>
    </FloatingMenu>
  );
}
