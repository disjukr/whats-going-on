import { CborValue, decodeCbor, encodeCbor } from "./cbor.ts";
import {
  decodeReqResMessageSequence,
  decodeReqResMessageSequencePrefix,
  encodePairedSecretCredential,
  encodeReqResMessageSequence,
  PAIRED_SECRET_AUTH_MECHANISM,
  type ReqResMessage,
  ReqResMessageKind,
  RpcErrorKind,
  SessionAuthErrorCode,
} from "./wire.ts";
import { Machine, normalizeMachineUrl } from "../state/machines.ts";

const PROC_GET_DAEMON_INFO = 1;
const PROC_START_PAIRING = 2;
const PROC_COMPLETE_PAIRING = 3;
const PROC_RENEW_CLIENT_CREDENTIAL = 4;
const PROC_SUBSCRIBE_ROOTS = 5;
const PROC_SUBSCRIBE_DIRECTORY = 6;
const PROC_READ_FILE = 7;
const PROC_WRITE_FILE = 8;
const PROC_CREATE_NODES = 9;
const PROC_RENAME_PATHS = 10;
const PROC_DELETE_PATHS = 11;
const PROC_CREATE_TERMINAL_SESSION = 12;
const PROC_SUBSCRIBE_TERMINAL_SESSIONS = 13;
const PROC_SUBSCRIBE_AVAILABLE_SHELLS = 14;
const PROC_ATTACH_TERMINAL_SESSION = 15;
const PROC_TAKE_TERMINAL_CONTROL = 16;
const PROC_WRITE_TERMINAL_INPUT = 17;
const PROC_CLOSE_TERMINAL_SESSION = 18;
const PROC_SUBSCRIBE_CLIENTS = 19;
const CONNECT_TIMEOUT_MS = 10_000;

export interface CompletePairingResponse {
  clientId: string;
  clientSecret: string;
  clientCredentialExpiresAtUnix: number;
}

export interface StartPairingResponse {
  pairingCodeExpiresAtUnix: number;
}

export interface RenewClientCredentialResponse {
  clientCredentialExpiresAtUnix: number;
}

export interface DaemonInfo {
  supportedProcIds: number[];
  version: string;
  os: string;
  instanceId: string;
  startedAtMs: number;
  serverTimeMs: number;
}

export interface ClientInfo {
  clientId: string;
  label: string;
  createdAtUnix: number;
  expiresAtUnix: number;
}

export type ClientsTableEvent =
  | { type: "snapshot"; rows: ClientInfo[] }
  | {
    type: "patch";
    removes: { clientId: string }[];
    upserts: ClientInfo[];
  };

export enum FsEntryKind {
  File = 1,
  Directory = 2,
  Symlink = 3,
  Other = 4,
}

export interface FsEntry {
  name: string;
  path: string;
  kind: FsEntryKind;
  size?: number;
  modifiedAtMs?: number;
  readonly: boolean;
}

export type RootsTableEvent =
  | { type: "snapshot"; rows: FsEntry[] }
  | { type: "patch"; removes: { path: string }[]; upserts: FsEntry[] }
  | { type: "closed"; reason: string };

export type DirectoryTableEvent =
  | { type: "snapshot"; rows: FsEntry[] }
  | { type: "patch"; removes: { name: string }[]; upserts: FsEntry[] }
  | { type: "closed"; reason: string; to?: string };

export enum WriteFileMode {
  Create = 1,
  Replace = 2,
  Append = 3,
  Patch = 4,
}

export interface ReadFileOptions {
  offset?: number;
  length?: number;
}

export interface ReadFileChunk {
  offset: number;
  bytes: Uint8Array;
}

export interface WriteFileStart {
  path: string;
  mode: WriteFileMode;
  expectedResultSize?: number;
  modifiedAtMs?: number;
}

export interface WriteFileChunk {
  offset?: number;
  bytes: Uint8Array;
}

export interface WriteFileResult {
  bytesWritten: number;
  resultSize: number;
  modifiedAtMs?: number;
}

export enum DeleteMode {
  Trash = 1,
  Permanent = 2,
}

export type CreateNodeSpec =
  | { type: "file" }
  | { type: "directory" }
  | { type: "symlink"; target: string }
  | { type: "hardlink"; target: string };

export interface CreateNodeOp {
  path: string;
  spec: CreateNodeSpec;
}

export interface RenamePathOp {
  from: string;
  to: string;
}

export type BulkMutationItemResult =
  | { ok: true; index: number }
  | { ok: false; index: number; code: string; message: string };

export interface BulkMutationResponse {
  results: BulkMutationItemResult[];
}

export interface TerminalLaunchSpec {
  command: string;
  args: string[];
}

export interface CreateTerminalSessionRequest {
  cols: number;
  rows: number;
  cwd?: string;
  launch: TerminalLaunchSpec;
  title?: string;
}

export interface TerminalExit {
  code?: number;
  signal?: string;
  exitedAtMs: number;
}

export interface TerminalSessionInfo {
  terminalSessionId: string;
  creatorClientId: string;
  createdAtMs: number;
  lastAttachedAtMs?: number;
  lastDetachedAtMs?: number;
  lastOutputAtMs?: number;
  cols: number;
  rows: number;
  primaryAttachId?: string;
  latestOutputSeq: number;
  lastKnownTitle?: string;
  exit?: TerminalExit;
  lastKnownCwd?: string;
  launch: TerminalLaunchSpec;
}

export interface AvailableShellInfo {
  shellId: string;
  name: string;
  command: string;
  args: string[];
  isDefault: boolean;
}

export type TerminalSessionsTableEvent =
  | { type: "snapshot"; rows: TerminalSessionInfo[] }
  | {
    type: "patch";
    removes: { terminalSessionId: string }[];
    upserts: TerminalSessionInfo[];
  };

export type AvailableShellsTableEvent =
  | { type: "snapshot"; rows: AvailableShellInfo[] }
  | {
    type: "patch";
    removes: { shellId: string }[];
    upserts: AvailableShellInfo[];
  };

export type TerminalEvent =
  | {
    type: "attached";
    attachId: string;
    primaryAttachId?: string;
    session: TerminalSessionInfo;
  }
  | { type: "outputChunk"; seq: number; bytes: Uint8Array }
  | { type: "historyGap"; nextSeq: number }
  | { type: "controlChanged"; primaryAttachId: string }
  | { type: "pseudoTerminalResized"; cols: number; rows: number }
  | { type: "sessionExited"; exit: TerminalExit }
  | { type: "sessionClosed"; reason: string }
  | { type: "workingDirectoryChanged"; cwd: string }
  | { type: "titleChanged"; title: string };

export interface TakeTerminalControlResponse {
  primaryAttachId: string;
}

export class RpcError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "RpcError";
  }
}

export function isInvalidCredentialsError(err: unknown): boolean {
  return err instanceof RpcError && err.code === "InvalidCredentials";
}

export async function completePairing(
  transport: WebTransport,
  code: string,
): Promise<CompletePairingResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([[1, code]]),
  );
  const response = await sendUnaryPayload(
    transport,
    PROC_COMPLETE_PAIRING,
    payload,
  );
  const map = decodeMap(response);
  return {
    clientId: text(map.get(1)),
    clientSecret: text(map.get(2)),
    clientCredentialExpiresAtUnix: integer(map.get(3)),
  };
}

export async function startPairing(
  transport: WebTransport,
  machine: Machine,
  confirmationCode: string,
  clientLabel: string,
): Promise<StartPairingResponse> {
  const fields: [number, CborValue][] = [
    [1, confirmationCode],
    [2, clientLabel],
  ];
  if (machine.clientId) {
    fields.push([3, machine.clientId]);
  }
  const payload = encodeCbor(
    new Map<number, CborValue>(fields),
  );
  const response = await sendUnaryPayload(
    transport,
    PROC_START_PAIRING,
    payload,
  );
  const map = decodeMap(response);
  return {
    pairingCodeExpiresAtUnix: integer(map.get(1)),
  };
}

export async function renewClientCredential(
  transport: WebTransport,
): Promise<RenewClientCredentialResponse> {
  return await renewAuthenticatedSessionCredential(transport);
}

export async function getDaemonInfoFromTransport(
  transport: WebTransport,
): Promise<DaemonInfo> {
  const response = await sendUnaryPayload(
    transport,
    PROC_GET_DAEMON_INFO,
  );
  return decodeDaemonInfoResponse(response);
}

function decodeDaemonInfoResponse(response: Uint8Array): DaemonInfo {
  const map = decodeMap(response);
  return {
    supportedProcIds: array(map.get(1)).map(integer),
    version: text(map.get(2)),
    os: text(map.get(3)),
    instanceId: text(map.get(4)),
    startedAtMs: integer(map.get(5)),
    serverTimeMs: integer(map.get(6)),
  };
}

export async function* subscribeClients(
  transport: WebTransport,
): AsyncGenerator<ClientsTableEvent> {
  yield* callServerStreamEvents(
    transport,
    PROC_SUBSCRIBE_CLIENTS,
    undefined,
    decodeClientsTableEvent,
  );
}

export async function* subscribeRoots(
  transport: WebTransport,
): AsyncGenerator<RootsTableEvent> {
  yield* callServerStreamEvents(
    transport,
    PROC_SUBSCRIBE_ROOTS,
    undefined,
    decodeRootsTableEvent,
  );
}

export async function* subscribeDirectory(
  transport: WebTransport,
  path: string,
): AsyncGenerator<DirectoryTableEvent> {
  const payload = encodeCbor(new Map<number, CborValue>([[1, path]]));
  yield* callServerStreamEvents(
    transport,
    PROC_SUBSCRIBE_DIRECTORY,
    payload,
    decodeDirectoryTableEvent,
  );
}

export async function readFile(
  transport: WebTransport,
  path: string,
  options: ReadFileOptions = {},
): Promise<Uint8Array> {
  const request = new Map<number, CborValue>([[1, path]]);
  if (options.offset !== undefined) request.set(2, options.offset);
  if (options.length !== undefined) request.set(3, options.length);

  const chunks: ReadFileChunk[] = [];
  for await (
    const chunk of callServerStreamEvents(
      transport,
      PROC_READ_FILE,
      encodeCbor(request),
      decodeReadFileChunk,
    )
  ) {
    chunks.push(chunk);
  }
  return assembleReadFileChunks(chunks, options.offset ?? 0);
}

export async function writeFile(
  transport: WebTransport,
  path: string,
  mode: WriteFileMode,
  fileBytes: Uint8Array,
  options: Omit<WriteFileStart, "path" | "mode"> & { offset?: number } = {},
): Promise<WriteFileResult> {
  return await writeFileChunks(
    transport,
    {
      path,
      mode,
      expectedResultSize: options.expectedResultSize,
      modifiedAtMs: options.modifiedAtMs,
    },
    [{ offset: options.offset, bytes: fileBytes }],
  );
}

export async function writeFileChunks(
  transport: WebTransport,
  start: WriteFileStart,
  chunks: WriteFileChunk[],
): Promise<WriteFileResult> {
  const response = await callClientStreamPayload(
    transport,
    PROC_WRITE_FILE,
    encodeWriteFileStart(start),
    chunks.map(encodeWriteFileChunk),
  );
  return decodeWriteFileResult(response);
}

export async function createNodes(
  transport: WebTransport,
  nodes: CreateNodeOp[],
): Promise<BulkMutationResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([
      [1, nodes.map(encodeCreateNodeOp)],
    ]),
  );
  const response = await callUnaryPayload(
    transport,
    PROC_CREATE_NODES,
    payload,
  );
  return decodeBulkMutationResponse(response);
}

export async function renamePaths(
  transport: WebTransport,
  ops: RenamePathOp[],
): Promise<BulkMutationResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([
      [
        1,
        ops.map((op) =>
          new Map<number, CborValue>([
            [1, op.from],
            [2, op.to],
          ])
        ),
      ],
    ]),
  );
  const response = await callUnaryPayload(
    transport,
    PROC_RENAME_PATHS,
    payload,
  );
  return decodeBulkMutationResponse(response);
}

export async function deletePaths(
  transport: WebTransport,
  paths: string[],
  mode: DeleteMode,
): Promise<BulkMutationResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([
      [1, paths],
      [2, mode],
    ]),
  );
  const response = await callUnaryPayload(
    transport,
    PROC_DELETE_PATHS,
    payload,
  );
  return decodeBulkMutationResponse(response);
}

export async function createTerminalSession(
  transport: WebTransport,
  request: CreateTerminalSessionRequest,
): Promise<TerminalSessionInfo> {
  const response = await callUnaryPayload(
    transport,
    PROC_CREATE_TERMINAL_SESSION,
    encodeCreateTerminalSessionRequest(request),
  );
  return decodeTerminalSessionInfoValue(decodeCbor(response));
}

export async function* subscribeTerminalSessions(
  transport: WebTransport,
): AsyncGenerator<TerminalSessionsTableEvent> {
  yield* callServerStreamEvents(
    transport,
    PROC_SUBSCRIBE_TERMINAL_SESSIONS,
    undefined,
    decodeTerminalSessionsTableEvent,
  );
}

export async function* subscribeAvailableShells(
  transport: WebTransport,
): AsyncGenerator<AvailableShellsTableEvent> {
  yield* callServerStreamEvents(
    transport,
    PROC_SUBSCRIBE_AVAILABLE_SHELLS,
    undefined,
    decodeAvailableShellsTableEvent,
  );
}

export async function* attachTerminalSession(
  transport: WebTransport,
  request: {
    terminalSessionId: string;
    afterSeq?: number;
    viewportCols: number;
    viewportRows: number;
  },
): AsyncGenerator<TerminalEvent> {
  yield* callServerStreamEvents(
    transport,
    PROC_ATTACH_TERMINAL_SESSION,
    encodeCbor(
      new Map<number, CborValue>([
        [1, request.terminalSessionId],
        ...(request.afterSeq === undefined
          ? []
          : [[2, request.afterSeq] as [number, CborValue]]),
        [3, request.viewportCols],
        [4, request.viewportRows],
      ]),
    ),
    decodeTerminalEvent,
  );
}

export async function takeTerminalControl(
  transport: WebTransport,
  request: {
    terminalSessionId: string;
    attachId: string;
    viewportCols: number;
    viewportRows: number;
  },
): Promise<TakeTerminalControlResponse> {
  const response = await callUnaryPayload(
    transport,
    PROC_TAKE_TERMINAL_CONTROL,
    encodeCbor(
      new Map<number, CborValue>([
        [1, request.terminalSessionId],
        [2, request.attachId],
        [3, request.viewportCols],
        [4, request.viewportRows],
      ]),
    ),
  );
  const map = decodeMap(response);
  return { primaryAttachId: text(map.get(1)) };
}

export async function writeTerminalInput(
  transport: WebTransport,
  terminalSessionId: string,
  attachId: string,
  bytes: Uint8Array,
): Promise<void> {
  await callClientStream(
    transport,
    PROC_WRITE_TERMINAL_INPUT,
    encodeCbor([
      1,
      new Map<number, CborValue>([
        [1, terminalSessionId],
        [2, attachId],
      ]),
    ]),
    [encodeCbor([2, new Map<number, CborValue>([[1, bytes]])])],
  );
}

export async function closeTerminalSession(
  transport: WebTransport,
  terminalSessionId: string,
): Promise<void> {
  await callUnary(
    transport,
    PROC_CLOSE_TERMINAL_SESSION,
    encodeCbor(new Map<number, CborValue>([[1, terminalSessionId]])),
  );
}

async function callUnaryPayload(
  transport: WebTransport,
  procId: number,
  payload: Uint8Array | undefined,
): Promise<Uint8Array> {
  const response = await callUnary(transport, procId, payload);
  if (!response) throw new Error("missing response payload");
  return response;
}

export async function sendUnaryPayload(
  transport: WebTransport,
  procId: number,
  payload?: Uint8Array,
): Promise<Uint8Array> {
  const response = await sendUnary(transport, procId, payload);
  if (!response) throw new Error("missing response payload");
  return response;
}

async function callUnary(
  transport: WebTransport,
  procId: number,
  payload: Uint8Array | undefined,
): Promise<Uint8Array | undefined> {
  return await sendUnary(transport, procId, payload);
}

async function callClientStreamPayload(
  transport: WebTransport,
  procId: number,
  startPayload: Uint8Array,
  chunkPayloads: Uint8Array[],
): Promise<Uint8Array> {
  const response = await callClientStream(
    transport,
    procId,
    startPayload,
    chunkPayloads,
  );
  if (!response) throw new Error("missing response payload");
  return response;
}

async function callClientStream(
  transport: WebTransport,
  procId: number,
  startPayload: Uint8Array,
  chunkPayloads: Uint8Array[],
): Promise<Uint8Array | undefined> {
  return await sendClientStream(
    transport,
    procId,
    startPayload,
    chunkPayloads,
  );
}

async function* callServerStreamEvents<T>(
  transport: WebTransport,
  procId: number,
  payload: Uint8Array | undefined,
  decodePayload: (bytes: Uint8Array) => T,
): AsyncGenerator<T> {
  yield* streamServerEvents(
    transport,
    procId,
    payload,
    decodePayload,
  );
}

export async function openWebTransport(
  machine: Machine,
  path = "/rpc",
): Promise<WebTransport> {
  const transport = new WebTransport(
    `${normalizeMachineUrl(machine.baseUrl)}${path}`,
  );
  try {
    await withTimeout(
      transport.ready,
      CONNECT_TIMEOUT_MS,
      "WebTransport connection",
    );
  } catch (err) {
    transport.close();
    throw err;
  }
  return transport;
}

function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timeout = setTimeout(
      () => reject(new Error(`${label} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
  });
  return Promise.race([promise, timeoutPromise]).finally(() => {
    if (timeout !== undefined) clearTimeout(timeout);
  });
}

export async function authenticateWebTransport(
  transport: WebTransport,
  clientId: string,
  clientSecret: string,
): Promise<void> {
  const stream = await transport.createBidirectionalStream();
  const request = encodeReqResMessageSequence([{
    kind: ReqResMessageKind.SessionAuthenticate,
    mechanism: PAIRED_SECRET_AUTH_MECHANISM,
    payload: encodePairedSecretCredential({
      credentialId: clientId,
      credentialSecret: clientSecret,
    }),
  }]);
  const writer = stream.writable.getWriter();
  await writer.write(request);
  await writer.close();

  const bytes = await readAll(stream.readable);
  const messages = decodeReqResMessageSequence(bytes);
  if (messages.length !== 1) {
    throw new Error("expected one session authentication response");
  }
  const response = messages[0]!;
  if (response.kind === ReqResMessageKind.SessionAuthError) {
    throw new RpcError(
      sessionAuthErrorCode(response.authErrorCode),
      response.message ?? "Session authentication failed",
    );
  }
  if (response.kind !== ReqResMessageKind.SessionAuthenticated) {
    throw new Error("expected session authentication response");
  }
}

async function renewAuthenticatedSessionCredential(
  transport: WebTransport,
): Promise<RenewClientCredentialResponse> {
  const response = await sendUnaryPayload(
    transport,
    PROC_RENEW_CLIENT_CREDENTIAL,
  );
  const map = decodeMap(response);
  return {
    clientCredentialExpiresAtUnix: integer(map.get(1)),
  };
}

export function closeWebTransport(transport: WebTransport): void {
  transport.close();
}

async function sendUnary(
  transport: WebTransport,
  procId: number,
  payload?: Uint8Array,
): Promise<Uint8Array | undefined> {
  const stream = await transport.createBidirectionalStream();

  const request = encodeReqResMessageSequence([{
    kind: ReqResMessageKind.RequestUnary,
    procId,
    payload,
  }]);
  const writer = stream.writable.getWriter();
  await writer.write(request);
  await writer.close();

  const bytes = await readAll(stream.readable);
  const messages = decodeReqResMessageSequence(bytes);
  return decodeUnaryResponse(procId, messages);
}

async function sendClientStream(
  transport: WebTransport,
  procId: number,
  startPayload: Uint8Array,
  chunkPayloads: Uint8Array[],
): Promise<Uint8Array | undefined> {
  const stream = await transport.createBidirectionalStream();
  const writer = stream.writable.getWriter();
  try {
    await writer.write(encodeReqResMessageSequence([{
      kind: ReqResMessageKind.RequestStreamStart,
      procId,
      payload: startPayload,
    }]));
    for (const payload of chunkPayloads) {
      await writer.write(encodeReqResMessageSequence([{
        kind: ReqResMessageKind.RequestStreamChunk,
        payload,
      }]));
    }
    await writer.close();
  } finally {
    try {
      writer.releaseLock();
    } catch {
      // The writer may already be detached by stream shutdown.
    }
  }

  const bytes = await readAll(stream.readable);
  const messages = decodeReqResMessageSequence(bytes);
  return decodeUnaryResponse(procId, messages);
}

function decodeUnaryResponse(
  procId: number,
  messages: ReqResMessage[],
): Uint8Array | undefined {
  if (messages.length !== 1) throw new Error("expected one response message");
  const response = messages[0]!;
  if (response.kind === ReqResMessageKind.ResponseUnaryError) {
    if (!response.error) {
      throw new RpcError("rpc_error", "RPC error response");
    }
    if (!response.errorKind) {
      throw new RpcError("rpc_error", "RPC error response without kind");
    }
    const error = decodeErrorPayload(
      procId,
      response.errorKind,
      response.error,
    );
    throw new RpcError(error.code, error.message);
  }
  if (response.kind !== ReqResMessageKind.ResponseUnaryOk) {
    throw new Error("expected unary response message");
  }
  return response.payload;
}

async function* streamServerEvents<T>(
  transport: WebTransport,
  procId: number,
  payload: Uint8Array | undefined,
  decodePayload: (bytes: Uint8Array) => T,
): AsyncGenerator<T> {
  const stream = await transport.createBidirectionalStream();

  const request = encodeReqResMessageSequence([{
    kind: ReqResMessageKind.RequestUnary,
    procId,
    payload,
  }]);
  const writer = stream.writable.getWriter();
  try {
    await writer.write(request);
    await writer.close();
  } finally {
    try {
      writer.releaseLock();
    } catch {
      // The stream may already be closing after a transport failure.
    }
  }

  const reader = stream.readable.getReader();
  let buffered: Uint8Array<ArrayBufferLike> = new Uint8Array();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffered = concatBytes(buffered, value);

      const { messages, readBytes } = decodeReqResMessageSequencePrefix(
        buffered,
      );
      if (readBytes > 0) buffered = buffered.slice(readBytes);

      for (const message of messages) {
        if (message.kind === ReqResMessageKind.ResponseStreamErrorEnd) {
          if (!message.error || !message.errorKind) {
            throw new RpcError("rpc_error", "RPC stream error response");
          }
          const error = decodeErrorPayload(
            procId,
            message.errorKind,
            message.error,
          );
          throw new RpcError(error.code, error.message);
        }
        if (
          message.kind !== ReqResMessageKind.ResponseStreamStart &&
          message.kind !== ReqResMessageKind.ResponseStreamChunk
        ) {
          throw new Error("expected stream response message");
        }
        if (!message.payload) {
          throw new Error("missing stream response payload");
        }
        yield decodePayload(message.payload);
      }
    }

    if (buffered.length > 0) {
      throw new Error("incomplete stream response message");
    }
  } finally {
    try {
      await reader.cancel();
    } catch {
      // The stream may already be closed by the server.
    }
    try {
      reader.releaseLock();
    } catch {
      // The reader may already be detached by stream shutdown.
    }
  }
}

async function readAll(
  readable: ReadableStream<Uint8Array>,
): Promise<Uint8Array> {
  const reader = readable.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    total += value.length;
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.length;
  }
  return out;
}

function concatBytes(left: Uint8Array, right: Uint8Array): Uint8Array {
  if (left.length === 0) return right;
  if (right.length === 0) return left;
  const out = new Uint8Array(left.length + right.length);
  out.set(left, 0);
  out.set(right, left.length);
  return out;
}

function decodeMap(bytes: Uint8Array): Map<number, CborValue> {
  const map = decodeCbor(bytes);
  if (!(map instanceof Map)) throw new Error("expected CBOR map");
  return map;
}

function encodeWriteFileStart(start: WriteFileStart): Uint8Array {
  const fields = new Map<number, CborValue>([
    [1, start.path],
    [2, start.mode],
  ]);
  if (start.expectedResultSize !== undefined) {
    fields.set(3, start.expectedResultSize);
  }
  if (start.modifiedAtMs !== undefined) {
    fields.set(4, start.modifiedAtMs);
  }
  return encodeCbor([1, fields]);
}

function encodeWriteFileChunk(chunk: WriteFileChunk): Uint8Array {
  const fields = new Map<number, CborValue>([[2, chunk.bytes]]);
  if (chunk.offset !== undefined) fields.set(1, chunk.offset);
  return encodeCbor([2, fields]);
}

function decodeReadFileChunk(bytes: Uint8Array): ReadFileChunk {
  const map = decodeMap(bytes);
  return {
    offset: integer(map.get(1)),
    bytes: bytesField(map.get(2)),
  };
}

function decodeWriteFileResult(bytes: Uint8Array): WriteFileResult {
  const map = decodeMap(bytes);
  return {
    bytesWritten: integer(map.get(1)),
    resultSize: integer(map.get(2)),
    modifiedAtMs: optionalInteger(map.get(3)),
  };
}

function assembleReadFileChunks(
  chunks: ReadFileChunk[],
  fallbackOffset: number,
): Uint8Array {
  if (chunks.length === 0) return new Uint8Array();
  const baseOffset = chunks[0]?.offset ?? fallbackOffset;
  const total = chunks.reduce((max, chunk) => {
    const end = chunk.offset - baseOffset + chunk.bytes.length;
    return Math.max(max, end);
  }, 0);
  const out = new Uint8Array(total);
  for (const chunk of chunks) {
    out.set(chunk.bytes, chunk.offset - baseOffset);
  }
  return out;
}

function encodeCreateNodeOp(op: CreateNodeOp): CborValue {
  return new Map<number, CborValue>([
    [1, op.path],
    [2, encodeCreateNodeSpec(op.spec)],
  ]);
}

function encodeCreateNodeSpec(spec: CreateNodeSpec): CborValue {
  switch (spec.type) {
    case "file":
      return [1, new Map<number, CborValue>()];
    case "directory":
      return [2, new Map<number, CborValue>()];
    case "symlink":
      return [3, new Map<number, CborValue>([[1, spec.target]])];
    case "hardlink":
      return [4, new Map<number, CborValue>([[1, spec.target]])];
  }
}

function encodeCreateTerminalSessionRequest(
  request: CreateTerminalSessionRequest,
): Uint8Array {
  const fields = new Map<number, CborValue>([
    [1, request.cols],
    [2, request.rows],
  ]);
  if (request.cwd !== undefined) fields.set(3, request.cwd);
  fields.set(4, encodeTerminalLaunchSpec(request.launch));
  if (request.title !== undefined) fields.set(5, request.title);
  return encodeCbor(fields);
}

function encodeTerminalLaunchSpec(launch: TerminalLaunchSpec): CborValue {
  return new Map<number, CborValue>([
    [1, launch.command],
    [2, launch.args],
  ]);
}

function decodeRootsTableEvent(bytes: Uint8Array): RootsTableEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return { type: "snapshot", rows: decodeFsEntries(fields.get(1)) };
    case 2:
      return {
        type: "patch",
        removes: array(fields.get(1)).map((value) => {
          const row = mapValue(value);
          return { path: text(row.get(1)) };
        }),
        upserts: decodeFsEntries(fields.get(2)),
      };
    case 3:
      return { type: "closed", reason: rootsCloseReason(fields.get(1)) };
    default:
      throw new Error(`unknown RootsTableEvent variant ${variantId}`);
  }
}

function decodeDirectoryTableEvent(bytes: Uint8Array): DirectoryTableEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return { type: "snapshot", rows: decodeFsEntries(fields.get(1)) };
    case 2:
      return {
        type: "patch",
        removes: array(fields.get(1)).map((value) => {
          const row = mapValue(value);
          return { name: text(row.get(1)) };
        }),
        upserts: decodeFsEntries(fields.get(2)),
      };
    case 3: {
      const [reason, reasonFields] = decodeUnionValue(fields.get(1));
      if (reason === 2) {
        return {
          type: "closed",
          reason: "Moved",
          to: optionalText(reasonFields.get(1)),
        };
      }
      return { type: "closed", reason: directoryCloseReason(reason) };
    }
    default:
      throw new Error(`unknown DirectoryTableEvent variant ${variantId}`);
  }
}

function decodeTerminalSessionsTableEvent(
  bytes: Uint8Array,
): TerminalSessionsTableEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return {
        type: "snapshot",
        rows: array(fields.get(1)).map(decodeTerminalSessionInfoValue),
      };
    case 2:
      return {
        type: "patch",
        removes: array(fields.get(1)).map((value) => {
          const row = mapValue(value);
          return { terminalSessionId: text(row.get(1)) };
        }),
        upserts: array(fields.get(2)).map(decodeTerminalSessionInfoValue),
      };
    default:
      throw new Error(
        `unknown TerminalSessionsTableEvent variant ${variantId}`,
      );
  }
}

function decodeAvailableShellsTableEvent(
  bytes: Uint8Array,
): AvailableShellsTableEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return {
        type: "snapshot",
        rows: array(fields.get(1)).map(decodeAvailableShellInfoValue),
      };
    case 2:
      return {
        type: "patch",
        removes: array(fields.get(1)).map((value) => {
          const row = mapValue(value);
          return { shellId: text(row.get(1)) };
        }),
        upserts: array(fields.get(2)).map(decodeAvailableShellInfoValue),
      };
    default:
      throw new Error(`unknown AvailableShellsTableEvent variant ${variantId}`);
  }
}

function decodeClientsTableEvent(
  bytes: Uint8Array,
): ClientsTableEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return {
        type: "snapshot",
        rows: array(fields.get(1)).map(decodeClientInfoValue),
      };
    case 2:
      return {
        type: "patch",
        removes: array(fields.get(1)).map((value) => {
          const row = mapValue(value);
          return { clientId: text(row.get(1)) };
        }),
        upserts: array(fields.get(2)).map(decodeClientInfoValue),
      };
    default:
      throw new Error(`unknown ClientsTableEvent variant ${variantId}`);
  }
}

function decodeTerminalEvent(bytes: Uint8Array): TerminalEvent {
  const [variantId, fields] = decodeUnion(decodeCbor(bytes));
  switch (variantId) {
    case 1:
      return {
        type: "attached",
        attachId: text(fields.get(1)),
        primaryAttachId: optionalText(fields.get(2)),
        session: decodeTerminalSessionInfoValue(fields.get(3)),
      };
    case 2:
      return {
        type: "outputChunk",
        seq: integer(fields.get(1)),
        bytes: bytesField(fields.get(2)),
      };
    case 3:
      return { type: "historyGap", nextSeq: integer(fields.get(1)) };
    case 4:
      return {
        type: "controlChanged",
        primaryAttachId: text(fields.get(1)),
      };
    case 5:
      return {
        type: "pseudoTerminalResized",
        cols: integer(fields.get(1)),
        rows: integer(fields.get(2)),
      };
    case 6:
      return {
        type: "sessionExited",
        exit: decodeTerminalExitValue(fields.get(1)),
      };
    case 7:
      return {
        type: "sessionClosed",
        reason: terminalCloseReason(fields.get(1)),
      };
    case 8:
      return { type: "workingDirectoryChanged", cwd: text(fields.get(1)) };
    case 9:
      return { type: "titleChanged", title: text(fields.get(1)) };
    default:
      throw new Error(`unknown TerminalEvent variant ${variantId}`);
  }
}

function decodeBulkMutationResponse(bytes: Uint8Array): BulkMutationResponse {
  const map = decodeMap(bytes);
  return {
    results: array(map.get(1)).map(decodeBulkMutationItemResult),
  };
}

function decodeBulkMutationItemResult(
  value: CborValue,
): BulkMutationItemResult {
  const [variantId, fields] = decodeUnion(value);
  switch (variantId) {
    case 0: {
      const [errorVariant, errorFields] = decodeUnionValue(fields.get(2));
      return {
        ok: false,
        index: integer(fields.get(1)),
        code: fsMutationItemErrorCode(errorVariant),
        message: text(errorFields.get(1)),
      };
    }
    case 1:
      return { ok: true, index: integer(fields.get(1)) };
    default:
      throw new Error(`unknown BulkMutationItemResult variant ${variantId}`);
  }
}

function decodeTerminalSessionInfoValue(value: unknown): TerminalSessionInfo {
  const map = mapValue(value);
  const exitValue = map.get(12);
  return {
    terminalSessionId: text(map.get(1)),
    creatorClientId: text(map.get(2)),
    createdAtMs: integer(map.get(3)),
    lastAttachedAtMs: optionalInteger(map.get(4)),
    lastDetachedAtMs: optionalInteger(map.get(5)),
    lastOutputAtMs: optionalInteger(map.get(6)),
    cols: integer(map.get(7)),
    rows: integer(map.get(8)),
    primaryAttachId: optionalText(map.get(9)),
    latestOutputSeq: integer(map.get(10)),
    lastKnownTitle: optionalText(map.get(11)),
    exit: exitValue === undefined
      ? undefined
      : decodeTerminalExitValue(exitValue),
    lastKnownCwd: optionalText(map.get(13)),
    launch: decodeTerminalLaunchSpecValue(map.get(14)),
  };
}

function decodeTerminalLaunchSpecValue(value: unknown): TerminalLaunchSpec {
  const map = mapValue(value);
  return {
    command: text(map.get(1)),
    args: array(map.get(2)).map(text),
  };
}

function decodeTerminalExitValue(value: unknown): TerminalExit {
  const map = mapValue(value);
  return {
    code: optionalInteger(map.get(1)),
    signal: optionalText(map.get(2)),
    exitedAtMs: integer(map.get(3)),
  };
}

function decodeAvailableShellInfoValue(value: unknown): AvailableShellInfo {
  const map = mapValue(value);
  return {
    shellId: text(map.get(1)),
    name: text(map.get(2)),
    command: text(map.get(3)),
    args: array(map.get(4)).map(text),
    isDefault: optionalBoolean(map.get(5)) ?? false,
  };
}

function decodeClientInfoValue(value: unknown): ClientInfo {
  const map = mapValue(value);
  return {
    clientId: text(map.get(1)),
    label: text(map.get(2)),
    createdAtUnix: integer(map.get(3)),
    expiresAtUnix: integer(map.get(4)),
  };
}

function decodeFsEntries(value: unknown): FsEntry[] {
  return array(value).map(decodeFsEntry);
}

function decodeFsEntry(value: CborValue): FsEntry {
  const map = mapValue(value);
  return {
    name: text(map.get(1)),
    path: text(map.get(2)),
    kind: fsEntryKind(map.get(3)),
    size: optionalInteger(map.get(4)),
    modifiedAtMs: optionalInteger(map.get(5)),
    readonly: optionalBoolean(map.get(6)) ?? false,
  };
}

function decodeUnion(value: CborValue): [number, Map<number, CborValue>] {
  if (!Array.isArray(value) || value.length !== 2) {
    throw new Error("expected union tuple");
  }
  return [integer(value[0]), mapValue(value[1])];
}

function decodeUnionValue(
  value: unknown,
): [number, Map<number, CborValue>] {
  if (value === undefined) return [0, new Map()];
  if (!isCborValue(value)) throw new Error("expected union value");
  return decodeUnion(value);
}

function mapValue(value: unknown): Map<number, CborValue> {
  if (value === undefined) return new Map();
  if (!(value instanceof Map)) throw new Error("expected CBOR map");
  return value;
}

function array(value: unknown): CborValue[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new Error("expected CBOR array");
  return value;
}

function bytesField(value: unknown): Uint8Array {
  if (value === undefined) return new Uint8Array();
  if (!(value instanceof Uint8Array)) throw new Error("expected bytes field");
  return value;
}

function fsEntryKind(value: unknown): FsEntryKind {
  if (value === undefined) return FsEntryKind.Other;
  const kind = integer(value);
  switch (kind) {
    case FsEntryKind.File:
    case FsEntryKind.Directory:
    case FsEntryKind.Symlink:
    case FsEntryKind.Other:
      return kind;
    default:
      throw new Error(`unknown filesystem entry kind ${kind}`);
  }
}

function rootsCloseReason(value: unknown): string {
  const [variantId] = decodeUnionValue(value);
  switch (variantId) {
    case 0:
      return "Failed";
    case 1:
      return "PermissionLost";
    case 2:
      return "Unknown";
    default:
      return `RootsSubscriptionCloseReason${variantId}`;
  }
}

function directoryCloseReason(variantId: number): string {
  switch (variantId) {
    case 0:
      return "Failed";
    case 1:
      return "Deleted";
    case 3:
      return "PermissionLost";
    case 4:
      return "ReplacedByNonDirectory";
    case 5:
      return "Unknown";
    default:
      return `DirectorySubscriptionCloseReason${variantId}`;
  }
}

function terminalCloseReason(value: unknown): string {
  const [variantId] = decodeUnionValue(value);
  switch (variantId) {
    case 0:
      return "Failed";
    case 1:
      return "ClosedByClient";
    case 2:
      return "DaemonShuttingDown";
    case 3:
      return "Unknown";
    default:
      return `TerminalSessionCloseReason${variantId}`;
  }
}

function fsMutationItemErrorCode(variantId: number): string {
  switch (variantId) {
    case 0:
      return "Failed";
    case 1:
      return "PermissionDenied";
    case 2:
      return "NotFound";
    case 3:
      return "AlreadyExists";
    case 4:
      return "NotDirectory";
    case 5:
      return "NotFile";
    case 6:
      return "InvalidPath";
    case 7:
      return "Unsupported";
    default:
      return `FsMutationItemError${variantId}`;
  }
}

function decodeErrorPayload(
  procId: number,
  kind: RpcErrorKind,
  bytes: Uint8Array,
): { code: string; message: string } {
  const value = decodeCbor(bytes);
  if (kind === RpcErrorKind.System) {
    if (!(value instanceof Map)) {
      throw new Error("invalid system error payload");
    }
    return {
      code: rpcErrorCode(value.get(1)),
      message: text(value.get(2)),
    };
  }
  if (kind !== RpcErrorKind.Method) {
    throw new Error("invalid error kind");
  }
  if (!Array.isArray(value) || value.length < 2) {
    throw new Error("invalid method error payload");
  }
  const variantId = integer(value[0]);
  const fields = value[1];
  if (!(fields instanceof Map)) {
    throw new Error("invalid method error payload");
  }
  return {
    code: methodErrorCode(procId, variantId),
    message: text(fields.get(1)),
  };
}

function methodErrorCode(procId: number, variantId: number): string {
  const key = methodErrorKey(procId, variantId);
  return METHOD_ERROR_CODES[key] ?? `method_error_${variantId}`;
}

function methodErrorKey(procId: number, variantId: number): string {
  return `${procId}:${variantId}`;
}

function rpcErrorCode(value: unknown): string {
  const code = integer(value);
  return RPC_ERROR_CODES[code.toString()] ?? `rpc_error_${code}`;
}

function sessionAuthErrorCode(value: SessionAuthErrorCode | undefined): string {
  if (value === undefined) return "SessionAuthError";
  return SESSION_AUTH_ERROR_CODES[value.toString()] ??
    `session_auth_error_${value}`;
}

const RPC_ERROR_CODES: Record<string, string> = {
  "1": "BadMessage",
  "2": "Unauthorized",
  "3": "MissingPayload",
  "4": "NotImplemented",
  "6": "PermissionDenied",
  "7": "NotFound",
  "8": "OperationFailed",
  "9": "MalformedPayload",
};

const METHOD_ERROR_CODES: Record<string, string> = {
  [methodErrorKey(PROC_GET_DAEMON_INFO, 0)]: "Failed",
  [methodErrorKey(PROC_START_PAIRING, 0)]: "Failed",
  [methodErrorKey(PROC_COMPLETE_PAIRING, 1)]: "PairingNotStarted",
  [methodErrorKey(PROC_COMPLETE_PAIRING, 2)]: "PairingExpired",
  [methodErrorKey(PROC_COMPLETE_PAIRING, 3)]: "InvalidPairingCode",
  [methodErrorKey(PROC_RENEW_CLIENT_CREDENTIAL, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_ROOTS, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_DIRECTORY, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_DIRECTORY, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_SUBSCRIBE_DIRECTORY, 2)]: "NotFound",
  [methodErrorKey(PROC_SUBSCRIBE_DIRECTORY, 3)]: "NotDirectory",
  [methodErrorKey(PROC_READ_FILE, 0)]: "Failed",
  [methodErrorKey(PROC_READ_FILE, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_READ_FILE, 2)]: "NotFound",
  [methodErrorKey(PROC_READ_FILE, 3)]: "NotFile",
  [methodErrorKey(PROC_READ_FILE, 4)]: "InvalidPath",
  [methodErrorKey(PROC_WRITE_FILE, 0)]: "Failed",
  [methodErrorKey(PROC_WRITE_FILE, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_WRITE_FILE, 2)]: "NotFound",
  [methodErrorKey(PROC_WRITE_FILE, 3)]: "AlreadyExists",
  [methodErrorKey(PROC_WRITE_FILE, 4)]: "NotDirectory",
  [methodErrorKey(PROC_WRITE_FILE, 5)]: "NotFile",
  [methodErrorKey(PROC_WRITE_FILE, 6)]: "InvalidPath",
  [methodErrorKey(PROC_CREATE_NODES, 0)]: "Failed",
  [methodErrorKey(PROC_RENAME_PATHS, 0)]: "Failed",
  [methodErrorKey(PROC_DELETE_PATHS, 0)]: "Failed",
  [methodErrorKey(PROC_CREATE_TERMINAL_SESSION, 0)]: "Failed",
  [methodErrorKey(PROC_CREATE_TERMINAL_SESSION, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_CREATE_TERMINAL_SESSION, 2)]: "InvalidSize",
  [methodErrorKey(PROC_CREATE_TERMINAL_SESSION, 3)]: "ShellNotFound",
  [methodErrorKey(PROC_CREATE_TERMINAL_SESSION, 4)]: "InvalidLaunch",
  [methodErrorKey(PROC_SUBSCRIBE_TERMINAL_SESSIONS, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_TERMINAL_SESSIONS, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_SUBSCRIBE_AVAILABLE_SHELLS, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_AVAILABLE_SHELLS, 1)]: "PermissionDenied",
  [methodErrorKey(PROC_ATTACH_TERMINAL_SESSION, 0)]: "Failed",
  [methodErrorKey(PROC_ATTACH_TERMINAL_SESSION, 1)]: "NotFound",
  [methodErrorKey(PROC_ATTACH_TERMINAL_SESSION, 2)]: "PermissionDenied",
  [methodErrorKey(PROC_ATTACH_TERMINAL_SESSION, 3)]: "InvalidSize",
  [methodErrorKey(PROC_TAKE_TERMINAL_CONTROL, 0)]: "Failed",
  [methodErrorKey(PROC_TAKE_TERMINAL_CONTROL, 1)]: "NotFound",
  [methodErrorKey(PROC_TAKE_TERMINAL_CONTROL, 2)]: "PermissionDenied",
  [methodErrorKey(PROC_TAKE_TERMINAL_CONTROL, 3)]: "AttachNotFound",
  [methodErrorKey(PROC_TAKE_TERMINAL_CONTROL, 4)]: "InvalidSize",
  [methodErrorKey(PROC_WRITE_TERMINAL_INPUT, 0)]: "Failed",
  [methodErrorKey(PROC_WRITE_TERMINAL_INPUT, 1)]: "NotFound",
  [methodErrorKey(PROC_WRITE_TERMINAL_INPUT, 2)]: "PermissionDenied",
  [methodErrorKey(PROC_WRITE_TERMINAL_INPUT, 3)]: "AttachNotFound",
  [methodErrorKey(PROC_WRITE_TERMINAL_INPUT, 4)]: "NotPrimaryAttach",
  [methodErrorKey(PROC_CLOSE_TERMINAL_SESSION, 0)]: "Failed",
  [methodErrorKey(PROC_CLOSE_TERMINAL_SESSION, 1)]: "NotFound",
  [methodErrorKey(PROC_CLOSE_TERMINAL_SESSION, 2)]: "PermissionDenied",
  [methodErrorKey(PROC_SUBSCRIBE_CLIENTS, 0)]: "Failed",
  [methodErrorKey(PROC_SUBSCRIBE_CLIENTS, 1)]: "PermissionDenied",
};

const SESSION_AUTH_ERROR_CODES: Record<string, string> = {
  "1": "UnsupportedMechanism",
  "2": "InvalidCredentials",
  "3": "MalformedPayload",
  "4": "AlreadyAuthenticated",
};

function integer(value: unknown): number {
  if (value === undefined) return 0;
  if (typeof value === "number" && Number.isSafeInteger(value)) return value;
  throw new Error("expected integer field");
}

function optionalInteger(value: unknown): number | undefined {
  if (value === undefined) return undefined;
  return integer(value);
}

function text(value: unknown): string {
  if (value === undefined) return "";
  if (typeof value !== "string") throw new Error("expected string field");
  return value;
}

function optionalText(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  return text(value);
}

function optionalBoolean(value: unknown): boolean | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "boolean") throw new Error("expected boolean field");
  return value;
}

function isCborValue(value: unknown): value is CborValue {
  return value === null ||
    typeof value === "boolean" ||
    typeof value === "number" ||
    typeof value === "string" ||
    value instanceof Uint8Array ||
    Array.isArray(value) ||
    value instanceof Map;
}
