import { CborValue, decodeCbor, encodeCbor } from "./cbor.ts";
import {
  DatagramMessageKind,
  decodeDatagramMessage,
  decodeReqResMessageSequence,
  decodeReqResMessageSequencePrefix,
  encodeDatagramMessage,
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
const CONNECT_TIMEOUT_MS = 10_000;
const DATAGRAM_PING_TIMEOUT_MS = 5_000;
const CLIENT_CREDENTIAL_RENEWAL_INTERVAL_MS = 24 * 60 * 60 * 1000;

export interface RpcSession {
  transport: WebTransport;
  datagrams: DatagramRuntime;
}

interface DatagramRuntime {
  writer: WritableStreamDefaultWriter<Uint8Array>;
  pendingPings: Map<number, PendingDatagramPing>;
  nextPingId: number;
  closed: boolean;
}

interface PendingDatagramPing {
  startedAt: number;
  timeout: ReturnType<typeof setTimeout>;
  resolve: (latencyMs: number) => void;
  reject: (err: Error) => void;
}

export interface ClientCredentialRenewal {
  machineId: string;
  clientId: string;
  clientSecret: string;
  clientCredentialExpiresAtUnix: number;
}

export interface RpcCallOptions {
  onClientCredentialRenewal?: (renewal: ClientCredentialRenewal) => void;
}

const rpcSessions = new Map<string, Promise<RpcSession>>();

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
}

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

export class RpcError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "RpcError";
  }
}

export class DatagramPingTimeoutError extends Error {
  constructor(timeoutMs: number) {
    super(`datagram pong timed out after ${timeoutMs}ms`);
    this.name = "DatagramPingTimeoutError";
  }
}

export function isDatagramPingTimeoutError(
  err: unknown,
): err is DatagramPingTimeoutError {
  return err instanceof DatagramPingTimeoutError;
}

export function isInvalidCredentialsError(err: unknown): boolean {
  return err instanceof RpcError && err.code === "InvalidCredentials";
}

export async function completePairing(
  session: RpcSession,
  code: string,
): Promise<CompletePairingResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([[1, code]]),
  );
  const response = await sendUnaryPayload(
    session.transport,
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
  session: RpcSession,
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
    session.transport,
    PROC_START_PAIRING,
    payload,
  );
  const map = decodeMap(response);
  return {
    pairingCodeExpiresAtUnix: integer(map.get(1)),
  };
}

export async function getDaemonInfo(
  machine: Machine,
): Promise<DaemonInfo> {
  const response = await callUnaryPayload(
    machine,
    PROC_GET_DAEMON_INFO,
    undefined,
    {
      includeAuth: false,
    },
  );
  const map = decodeMap(response);
  return {
    supportedProcIds: array(map.get(1)).map(integer),
    version: text(map.get(2)),
    os: text(map.get(3)),
  };
}

export async function* subscribeRoots(
  machine: Machine,
  options: RpcCallOptions = {},
): AsyncGenerator<RootsTableEvent> {
  yield* callServerStreamEvents(
    machine,
    PROC_SUBSCRIBE_ROOTS,
    undefined,
    decodeRootsTableEvent,
    options,
  );
}

export async function* subscribeDirectory(
  machine: Machine,
  path: string,
  options: RpcCallOptions = {},
): AsyncGenerator<DirectoryTableEvent> {
  const payload = encodeCbor(new Map<number, CborValue>([[1, path]]));
  yield* callServerStreamEvents(
    machine,
    PROC_SUBSCRIBE_DIRECTORY,
    payload,
    decodeDirectoryTableEvent,
    options,
  );
}

export async function readFile(
  machine: Machine,
  path: string,
  options: ReadFileOptions = {},
  rpcOptions: RpcCallOptions = {},
): Promise<Uint8Array> {
  const request = new Map<number, CborValue>([[1, path]]);
  if (options.offset !== undefined) request.set(2, options.offset);
  if (options.length !== undefined) request.set(3, options.length);

  const chunks: ReadFileChunk[] = [];
  for await (
    const chunk of callServerStreamEvents(
      machine,
      PROC_READ_FILE,
      encodeCbor(request),
      decodeReadFileChunk,
      rpcOptions,
    )
  ) {
    chunks.push(chunk);
  }
  return assembleReadFileChunks(chunks, options.offset ?? 0);
}

export async function writeFile(
  machine: Machine,
  path: string,
  mode: WriteFileMode,
  fileBytes: Uint8Array,
  options: Omit<WriteFileStart, "path" | "mode"> & { offset?: number } = {},
  rpcOptions: RpcCallOptions = {},
): Promise<WriteFileResult> {
  return await writeFileChunks(
    machine,
    {
      path,
      mode,
      expectedResultSize: options.expectedResultSize,
      modifiedAtMs: options.modifiedAtMs,
    },
    [{ offset: options.offset, bytes: fileBytes }],
    rpcOptions,
  );
}

export async function writeFileChunks(
  machine: Machine,
  start: WriteFileStart,
  chunks: WriteFileChunk[],
  options: RpcCallOptions = {},
): Promise<WriteFileResult> {
  const response = await callClientStreamPayload(
    machine,
    PROC_WRITE_FILE,
    encodeWriteFileStart(start),
    chunks.map(encodeWriteFileChunk),
    options,
  );
  return decodeWriteFileResult(response);
}

export async function createNodes(
  machine: Machine,
  nodes: CreateNodeOp[],
  options: RpcCallOptions = {},
): Promise<BulkMutationResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([
      [1, nodes.map(encodeCreateNodeOp)],
    ]),
  );
  const response = await callUnaryPayload(
    machine,
    PROC_CREATE_NODES,
    payload,
    options,
  );
  return decodeBulkMutationResponse(response);
}

export async function renamePaths(
  machine: Machine,
  ops: RenamePathOp[],
  options: RpcCallOptions = {},
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
    machine,
    PROC_RENAME_PATHS,
    payload,
    options,
  );
  return decodeBulkMutationResponse(response);
}

export async function deletePaths(
  machine: Machine,
  paths: string[],
  mode: DeleteMode,
  options: RpcCallOptions = {},
): Promise<BulkMutationResponse> {
  const payload = encodeCbor(
    new Map<number, CborValue>([
      [1, paths],
      [2, mode],
    ]),
  );
  const response = await callUnaryPayload(
    machine,
    PROC_DELETE_PATHS,
    payload,
    options,
  );
  return decodeBulkMutationResponse(response);
}

export async function checkReachable(machine: Machine): Promise<number> {
  const startedAt = performance.now();
  const session = await openRpcSession(machine, "/rpc");
  try {
    return performance.now() - startedAt;
  } finally {
    closeRpcSession(session);
  }
}

export function closeMachineSession(machine: Machine): void {
  closeSession(machine);
}

async function callUnaryPayload(
  machine: Machine,
  procId: number,
  payload?: Uint8Array,
  options: RpcCallOptions & { includeAuth?: boolean } = {},
): Promise<Uint8Array> {
  const response = await callUnary(machine, procId, payload, options);
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
  machine: Machine,
  procId: number,
  payload?: Uint8Array,
  options: RpcCallOptions & { includeAuth?: boolean } = {},
): Promise<Uint8Array | undefined> {
  if (
    options.includeAuth !== false && machine.clientId && machine.clientSecret
  ) {
    const session = await authenticatedSession(machine, options);
    try {
      return await sendUnary(session.transport, procId, payload);
    } catch (err) {
      closeSession(machine);
      throw err;
    }
  }

  const session = await openRpcSession(machine, "/rpc");
  try {
    return await sendUnary(session.transport, procId, payload);
  } finally {
    closeRpcSession(session);
  }
}

async function callClientStreamPayload(
  machine: Machine,
  procId: number,
  startPayload: Uint8Array,
  chunkPayloads: Uint8Array[],
  options: RpcCallOptions = {},
): Promise<Uint8Array> {
  const response = await callClientStream(
    machine,
    procId,
    startPayload,
    chunkPayloads,
    options,
  );
  if (!response) throw new Error("missing response payload");
  return response;
}

async function callClientStream(
  machine: Machine,
  procId: number,
  startPayload: Uint8Array,
  chunkPayloads: Uint8Array[],
  options: RpcCallOptions = {},
): Promise<Uint8Array | undefined> {
  if (machine.clientId && machine.clientSecret) {
    const session = await authenticatedSession(machine, options);
    try {
      return await sendClientStream(
        session.transport,
        procId,
        startPayload,
        chunkPayloads,
      );
    } catch (err) {
      closeSession(machine);
      throw err;
    }
  }

  const session = await openRpcSession(machine, "/rpc");
  try {
    return await sendClientStream(
      session.transport,
      procId,
      startPayload,
      chunkPayloads,
    );
  } finally {
    closeRpcSession(session);
  }
}

async function* callServerStreamEvents<T>(
  machine: Machine,
  procId: number,
  payload: Uint8Array | undefined,
  decodePayload: (bytes: Uint8Array) => T,
  options: RpcCallOptions = {},
): AsyncGenerator<T> {
  if (machine.clientId && machine.clientSecret) {
    const session = await authenticatedSession(machine, options);
    try {
      yield* streamServerEvents(
        session.transport,
        procId,
        payload,
        decodePayload,
      );
    } catch (err) {
      closeSession(machine);
      throw err;
    }
    return;
  }

  const session = await openRpcSession(machine, "/rpc");
  try {
    yield* streamServerEvents(
      session.transport,
      procId,
      payload,
      decodePayload,
    );
  } finally {
    closeRpcSession(session);
  }
}

export async function openRpcSession(
  machine: Machine,
  path = "/rpc",
): Promise<RpcSession> {
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
  return {
    transport,
    datagrams: startDatagramRuntime(transport),
  };
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

interface AuthenticatedSessionOptions extends RpcCallOptions {
  renewOnConnect?: boolean;
}

function authenticatedSession(
  machine: Machine,
  options: AuthenticatedSessionOptions = {},
): Promise<RpcSession> {
  const key = sessionKey(machine);
  const current = rpcSessions.get(key);
  if (current) return current;

  const sessionPromise: Promise<RpcSession> = (async () => {
    if (!machine.clientId || !machine.clientSecret) {
      throw new Error("missing paired client credentials");
    }
    const session = await openRpcSession(machine, "/rpc");
    try {
      await authenticateSession(
        session.transport,
        machine.clientId,
        machine.clientSecret,
      );
    } catch (err) {
      closeRpcSession(session);
      throw err;
    }
    if (options.renewOnConnect !== false) {
      await renewSessionCredential(machine, session.transport, options);
      startClientCredentialRenewalLoop(machine, session, options);
    }
    session.transport.closed.finally(() => {
      if (rpcSessions.get(key) === sessionPromise) rpcSessions.delete(key);
    });
    return session;
  })();

  rpcSessions.set(key, sessionPromise);
  sessionPromise.catch(() => {
    if (rpcSessions.get(key) === sessionPromise) rpcSessions.delete(key);
  });
  return sessionPromise;
}

async function renewSessionCredential(
  machine: Machine,
  transport: WebTransport,
  options: RpcCallOptions,
): Promise<void> {
  if (!machine.clientId || !machine.clientSecret) return;
  try {
    const renewal = await renewAuthenticatedSessionCredential(transport);
    emitClientCredentialRenewal(machine, options, renewal);
  } catch {
    // A valid session can still be useful even if persisting a refreshed
    // credential expiry fails. The next renewal tick or authenticated session
    // will retry.
  }
}

function startClientCredentialRenewalLoop(
  machine: Machine,
  session: RpcSession,
  options: RpcCallOptions,
): void {
  const timer = setInterval(
    () => void renewSessionCredential(machine, session.transport, options),
    CLIENT_CREDENTIAL_RENEWAL_INTERVAL_MS,
  );
  session.transport.closed
    .catch(() => {})
    .finally(() => clearInterval(timer));
}

function emitClientCredentialRenewal(
  machine: Machine,
  options: RpcCallOptions,
  renewal: RenewClientCredentialResponse,
): void {
  if (!machine.clientId || !machine.clientSecret) return;
  options.onClientCredentialRenewal?.({
    machineId: machine.id,
    clientId: machine.clientId,
    clientSecret: machine.clientSecret,
    clientCredentialExpiresAtUnix: renewal.clientCredentialExpiresAtUnix,
  });
}

function closeSession(machine: Machine): void {
  const key = sessionKey(machine);
  const session = rpcSessions.get(key);
  rpcSessions.delete(key);
  session?.then(closeRpcSession).catch(() => {});
}

function sessionKey(machine: Machine): string {
  return [
    machine.id,
    normalizeMachineUrl(machine.baseUrl),
    machine.clientId ?? "",
    machine.clientSecret ?? "",
  ].join("\n");
}

async function authenticateSession(
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

function startDatagramRuntime(transport: WebTransport): DatagramRuntime {
  const runtime: DatagramRuntime = {
    writer: transport.datagrams.writable.getWriter(),
    pendingPings: new Map(),
    nextPingId: 0,
    closed: false,
  };
  void readDatagrams(transport, runtime);
  transport.closed
    .catch(() => {})
    .finally(() => {
      closeDatagramRuntime(runtime, new Error("WebTransport session closed"));
    });
  return runtime;
}

async function readDatagrams(
  transport: WebTransport,
  runtime: DatagramRuntime,
): Promise<void> {
  const reader = transport.datagrams.readable.getReader();
  try {
    while (!runtime.closed) {
      const { done, value } = await reader.read();
      if (done) break;
      await handleIncomingDatagram(runtime, value);
    }
  } catch (err) {
    closeDatagramRuntime(
      runtime,
      err instanceof Error ? err : new Error(String(err)),
    );
  } finally {
    try {
      reader.releaseLock();
    } catch {
      // The reader may already be detached by transport shutdown.
    }
  }
}

async function handleIncomingDatagram(
  runtime: DatagramRuntime,
  bytes: Uint8Array,
): Promise<void> {
  let message;
  try {
    message = decodeDatagramMessage(bytes);
  } catch {
    return;
  }

  if (message.kind === DatagramMessageKind.Ping) {
    try {
      await runtime.writer.write(encodeDatagramMessage({
        kind: DatagramMessageKind.Pong,
        pingId: message.pingId,
      }));
    } catch {
      closeDatagramRuntime(runtime, new Error("failed to send datagram pong"));
    }
    return;
  }

  const pending = runtime.pendingPings.get(message.pingId);
  if (!pending) return;

  runtime.pendingPings.delete(message.pingId);
  clearTimeout(pending.timeout);
  pending.resolve(performance.now() - pending.startedAt);
}

function pingDatagram(
  runtime: DatagramRuntime,
  timeoutMs = DATAGRAM_PING_TIMEOUT_MS,
): Promise<number> {
  if (runtime.closed) {
    return Promise.reject(new Error("datagram runtime is closed"));
  }

  const pingId = nextPingId(runtime);
  const startedAt = performance.now();
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      runtime.pendingPings.delete(pingId);
      reject(new DatagramPingTimeoutError(timeoutMs));
    }, timeoutMs);
    const pending: PendingDatagramPing = {
      startedAt,
      timeout,
      resolve,
      reject,
    };
    runtime.pendingPings.set(pingId, pending);

    runtime.writer.write(encodeDatagramMessage({
      kind: DatagramMessageKind.Ping,
      pingId,
    })).catch((err) => {
      if (runtime.pendingPings.get(pingId) !== pending) return;
      runtime.pendingPings.delete(pingId);
      clearTimeout(timeout);
      const error = err instanceof Error ? err : new Error(String(err));
      closeDatagramRuntime(runtime, error);
      reject(error);
    });
  });
}

function nextPingId(runtime: DatagramRuntime): number {
  runtime.nextPingId = runtime.nextPingId >= Number.MAX_SAFE_INTEGER
    ? 1
    : runtime.nextPingId + 1;
  return runtime.nextPingId;
}

export function closeRpcSession(session: RpcSession): void {
  closeDatagramRuntime(session.datagrams, new Error("RPC session closed"));
  session.transport.close();
}

function closeDatagramRuntime(runtime: DatagramRuntime, err: Error): void {
  if (runtime.closed) return;
  runtime.closed = true;
  for (const [pingId, pending] of runtime.pendingPings) {
    runtime.pendingPings.delete(pingId);
    clearTimeout(pending.timeout);
    pending.reject(err);
  }
  try {
    runtime.writer.releaseLock();
  } catch {
    // The writer may be in an errored state after transport shutdown.
  }
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
  if (!isCborValue(value)) throw new Error("expected union value");
  return decodeUnion(value);
}

function mapValue(value: unknown): Map<number, CborValue> {
  if (!(value instanceof Map)) throw new Error("expected CBOR map");
  return value;
}

function array(value: unknown): CborValue[] {
  if (!Array.isArray(value)) throw new Error("expected CBOR array");
  return value;
}

function bytesField(value: unknown): Uint8Array {
  if (!(value instanceof Uint8Array)) throw new Error("expected bytes field");
  return value;
}

function fsEntryKind(value: unknown): FsEntryKind {
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
};

const SESSION_AUTH_ERROR_CODES: Record<string, string> = {
  "1": "UnsupportedMechanism",
  "2": "InvalidCredentials",
  "3": "MalformedPayload",
  "4": "AlreadyAuthenticated",
};

function integer(value: unknown): number {
  if (typeof value === "number" && Number.isSafeInteger(value)) return value;
  throw new Error("expected integer field");
}

function optionalInteger(value: unknown): number | undefined {
  if (value == null) return undefined;
  return integer(value);
}

function text(value: unknown): string {
  if (typeof value !== "string") throw new Error("expected string field");
  return value;
}

function optionalText(value: unknown): string | undefined {
  if (value == null) return undefined;
  return text(value);
}

function optionalBoolean(value: unknown): boolean | undefined {
  if (value == null) return undefined;
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
