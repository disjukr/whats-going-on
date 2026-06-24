export type ConnectionState =
  | { phase: "idle" }
  | { phase: "reachable"; rttMs?: number }
  | { phase: "offline" };

export type StreamState =
  | { phase: "idle"; message: string }
  | { phase: "connecting"; message: string }
  | { phase: "live"; message: string }
  | { phase: "closed"; message: string }
  | { phase: "error"; message: string };

export type MachineModalMode = "add" | "pair" | "config" | "delete";
export type MachineMenuState = { machineId: string; x: number; y: number };
export type RailTooltipState = { name: string; x: number; y: number };
