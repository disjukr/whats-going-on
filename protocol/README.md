# rieul protocol

This directory contains the protocol contracts shared by the daemon and web
client. The protocol is split into narrow layers so each document has one job.

## Layers

- `rieul-cbor` defines the deterministic CBOR profile and schema-to-CBOR mapping.
- `rieul-wire` defines byte-level envelopes carried over WebTransport reqres
  streams and datagrams. It also provides the shared wire-schema encoding rules
  used by `rieul-socket-wire`.
- `rieul-socket-wire` defines socket-oriented reqres messages for local IPC and
  WebSocket fallback transports that need multiplexed logical RPC streams over
  one bidirectional channel.
- `rieul-rpc` defines RPC proc ids, stream shapes, payload schema selection, and
  method-level error unions.
- `rieul-ipc` defines local IPC process roles, profile-scoped endpoints, IPC
  proc direction attributes, and how IPC uses `rieul-socket-wire`.
- `schemas/rpc` defines domain RPC contracts such as pairing, filesystem, and
  terminal methods.
- `schemas/socket-wire` defines local IPC/WebSocket fallback socket reqres
  schemas.
- `schemas/ipc` defines IPC proc contracts carried on IPC endpoints. Proc files
  are grouped by provider role, such as `gui.bdl` and `user.bdl`.
- `schemas/config` defines local daemon configuration files. These schemas do
  not use RPC-only primitives such as `i53` or `u53`.

## Wire

`wire` means the byte-level envelope family that peers exchange over
WebTransport. It does not mean the whole transport stack, and it does not define
domain method payloads.

`rieul-wire` currently has two envelope shapes:

- `reqres`: reliable request/response-direction exchanges carried inside one
  WebTransport bidirectional stream.
- `datagram`: message-oriented envelopes carried by WebTransport datagrams.

## Reqres

One WebTransport bidirectional stream carries exactly one reqres exchange:

- one RPC invocation, or
- one session-control command.

The reqres stream body is a CBOR sequence of flattened `ReqResMessage` pairs:

```text
kind, fields, kind, fields, ...
```

`kind` is the `ReqResMessage` union variant id encoded as a CBOR unsigned
integer. `fields` is that variant's CBOR map. The normal two-element union array
wrapper is intentionally omitted only at this top-level stream grammar.

## Socket Wire

Socket wire uses the same flattened top-level reqres message encoding:

```text
kind, fields
```

Byte-stream transports carry one continuous CBOR sequence of flattened pairs.
Message transports such as WebSocket carry one flattened pair per binary
message. Socket wire adds `streamId` fields and socket-only direction-end
variants so multiple logical reqres exchanges can share one bidirectional
channel.

## Datagram

Each WebTransport datagram carries exactly one `DatagramMessage` encoded as the
normal two-element union tuple: `[variant_id, fields_map]`.

Datagram delivery may be lost, duplicated, or reordered. Datagram messages must
therefore be self-contained and must not rely on reqres stream lifecycle,
request/response cardinality, or half-close semantics.

The first datagram messages are `Ping` and `Pong`. `Ping.pingId` is a
sender-chosen session-local monotonic id. `Pong` echoes the same `pingId`.
`pingId` is only a correlation id, not a security primitive.

## RPC

RPC payload bytes are selected by proc id and by response variant. Method-level
errors use the proc's declared `throws` union. Failures outside a method
contract use the generic wire/envelope error payload.

Protocol proc id registry:

| id | proc                             |
| -: | -------------------------------- |
|  1 | `GetDaemonInfo`                  |
|  2 | `StartPairing`                   |
|  3 | `CompletePairing`                |
|  4 | `RenewClientCredential`          |
|  5 | `SubscribeRoots`                 |
|  6 | `SubscribeDirectory`             |
|  7 | `ReadFile`                       |
|  8 | `WriteFile`                      |
|  9 | `CreateNodes`                    |
| 10 | `RenamePaths`                    |
| 11 | `DeletePaths`                    |
| 12 | `CreateTerminalSession`          |
| 13 | `SubscribeTerminalSessions`      |
| 14 | `SubscribeAvailableShells`       |
| 15 | `AttachTerminalSession`          |
| 16 | `TakeTerminalControl`            |
| 17 | `WriteTerminalInput`             |
| 18 | `CloseTerminalSession`           |
| 19 | `SubscribeClients`               |
| 20 | `SubscribeTrashItems`            |
| 21 | `RestoreTrashItems`              |
| 22 | `PurgeTrashItems`                |
| 23 | `GetDaemonEnvironment`           |
| 24 | `SubscribeProcesses`             |
| 25 | `SubscribeProcessDetail`         |
| 26 | `SubscribeWindows`               |
| 27 | `SubscribeWindowDetail`          |
| 28 | `SubscribeProcessResourcesInUse` |
| 29 | `SubscribeProcessSocketsInUse`   |
| 30 | `SubscribeProcessModules`        |
| 31 | `RunCommand`                     |
| 32 | `CreateJob`                      |
| 33 | `SubscribeJobs`                  |
| 34 | `SubscribeJobOutput`             |
| 35 | `KillJob`                        |
| 36 | `DeleteJobs`                     |
| 37 | `ClearJobs`                      |
| 38 | `CreateSchedule`                 |
| 39 | `UpdateSchedule`                 |
| 40 | `SubscribeSchedules`             |
| 41 | `DeleteSchedules`                |
| 42 | `GetScheduleNextRuns`            |
| 43 | `RemoveClient`                   |
| 44 | `KillProcess`                    |
| 45 | `SubscribeAgentProviders`        |
| 46 | `CreateAgentProject`             |
| 47 | `SubscribeAgentProjects`         |
| 48 | `ListAgentSessions`              |
| 49 | `SubscribeAgentSessionCatalog`   |
| 50 | `SubscribeActiveAgentSessions`   |
| 51 | `CreateAgentSession`             |
| 52 | `SubscribeAgentSession`          |
| 53 | `CreateAgentTurn`                |
| 54 | `CancelAgentTurn`                |
| 55 | `RespondAgentPermission`         |
| 56 | `SetAgentSessionConfig`          |
| 57 | `UpdateAgentSession`             |
| 58 | `CloseAgentSession`              |
| 59 | `DeleteAgentSession`             |
| 60 | `RemoveAgentProject`             |
| 61 | `ListAgentSessionTurns`          |
| 62 | `ReadAgentTerminalOutput`        |
| 63 | `AttachAgentSession`             |

`GetDaemonInfo` returns daemon metadata: supported proc ids, daemon version, a
human-readable OS name for the daemon host, daemon instance lifecycle fields,
and the daemon's current server time. The OS string should include useful
platform-specific details when available, such as Windows edition, bitness,
display version, build, and service pack. `instanceId` and `startedAtMs` are
fixed while that daemon process is running and change when the daemon process
restarts. `serverTimeMs` is sampled while producing each response. Clients
should fetch daemon info again after reconnecting and compare `instanceId` with
the previous value to detect process-local state loss, such as terminal
sessions. `supportedProcIds` is the daemon's implemented subset of the protocol
registry for that process lifetime. Protected proc ids may still require session
authentication before invocation.

`GetDaemonEnvironment` returns daemon-host environment defaults such as the home
directory path visible to the daemon process. These values are environment
derived, not user-edited daemon config.

A proc's `stream` attribute defines request and response cardinality:

- `unary`: unary request, unary response
- `client`: streaming request, unary response
- `server`: unary request, streaming response
- `bidi`: streaming request, streaming response

Normal completion is represented by WebTransport half-close/EOF. There are no
application-level request-end or response-end messages.

## Session Control

Session authentication is part of `rieul-wire`, not an application RPC proc. A
client authenticates the WebTransport session by sending `SessionAuthenticate`
on a session-control stream. The mechanism name identifies the authentication
profile, and the payload is mechanism-specific deterministic CBOR bytes.

Protected RPC procs use the resulting session authentication state.

## Filesystem Model

Filesystem read-side state is modeled as reactive table subscriptions.

- `SubscribeRoots` streams a roots table.
- `SubscribeDirectory` streams a directory entry table.
- `SubscribeTrashItems` streams OS trash/recycle-bin items when supported by the
  daemon platform.
- The first event is `Snapshot`.
- Later `Snapshot` events replace the whole subscribed table view.
- `Patch` events update table membership with `removes` and `upserts`.
- `Closed` is a domain-level terminal event followed by normal stream close.

Filesystem metadata mutations are best-effort bulk commands:

- `CreateNodes`
- `RenamePaths`
- `DeletePaths`
- `RestoreTrashItems`
- `PurgeTrashItems`

Bulk mutation responses report item-level results by zero-based request item
index. There is no rollback guarantee. Subscribed table events are the source of
truth for resulting filesystem state.

`DeletePaths` with `DeleteMode.Trash` moves filesystem entries to the host OS
trash/recycle bin without permanently deleting on trash failure. Trash items are
identified by opaque OS-defined ids returned by `SubscribeTrashItems`. Restore
and purge requests use those ids. Clients derive display-only values such as the
original full path from `TrashItem.originalParent` and `TrashItem.name`; the API
does not duplicate values the client can derive.

File content I/O is range-oriented rather than cursor-oriented:

- `ReadFile` is unary request, server-streaming response. The request names a
  path plus optional `offset` and `length`. The response carries zero or more
  `ReadFileChunk` messages. EOF and empty reads are represented by normal stream
  close.
- `WriteFile` is client-streaming request, unary response. The request stream
  starts with exactly one `WriteFileStart`; all later request messages are
  `WriteFileChunk`. Normal request completion is represented by transport
  half-close. The response reports `bytesWritten` and `resultSize`.

Offsets, lengths, sizes, and epoch millisecond timestamps use `u53`.
`WriteFileStart.modifiedAtMs` is best-effort; inability to apply it does not
fail an otherwise successful write.

## Terminal Model

Terminal sessions are daemon-process-local pseudo-terminal sessions. They may
outlive browser tabs and WebTransport connections, but not a daemon process
restart. `TerminalSessionInfo.creatorClientId` records the paired client that
created the session as a cleanup and UI hint.

Terminal session membership is modeled as a reactive table subscription.
`SubscribeTerminalSessions` streams a `Snapshot` first. Later `Snapshot` events
replace the whole table view, and `Patch` events update membership with
`removes` and `upserts`. Newly created sessions are upserted into the table.
Exited sessions remain in the table with `TerminalSessionInfo.exit` until the
user explicitly closes them. User-closed sessions are removed from the table.

Available host shells are also modeled as a reactive table subscription.
`SubscribeAvailableShells` reports daemon-defined shell launch options. Clients
should start known shells by passing `TerminalLaunchSpec.Shell(shellId)` to
`CreateTerminalSession`, not by reconstructing host-specific commands from
display text. If launch is absent, the daemon uses the active user's default
interactive shell. `TerminalLaunchSpec.CustomCommand` is reserved for explicit
custom program launches.

Clients attach to a terminal session with `AttachTerminalSession`. Each attach
stream gets a unique `attachId`; the same WebTransport connection may attach to
the same terminal session more than once. Terminal output is an append-only raw
byte stream split into `OutputChunk` events with monotonic `seq` values. A
session with no output has `latestOutputSeq = 0`, and the first output chunk has
`seq = 1`. If `afterSeq` is absent, attach replay starts from the oldest
retained output. A client that reconnects sends `afterSeq` to replay retained
output after that sequence before following live output. A client that wants
live output only can pass the `latestOutputSeq` it has observed. If the
requested history has fallen out of the daemon's retention buffer, the daemon
reports `HistoryGap`.

Current daemon implementations retain up to 1 MiB of raw terminal output per
terminal session, including exited sessions retained for user cleanup. This is
an implementation policy, not a wire contract. Older retained output is dropped
first; `latestOutputSeq` remains monotonic across dropped chunks.

Terminal control is attach-scoped. `TakeTerminalControl` replaces the session's
`primaryAttachId` with a live attach owned by the current WebTransport session.
The previous primary attach does not need to be live. Calling
`TakeTerminalControl` again from the current primary attach is valid and updates
the pseudo-terminal size without changing control ownership. Input is accepted
only from the live primary attach and is sent over a client-streaming
`WriteTerminalInput` RPC whose first message binds the stream to one attach, and
later messages carry raw input bytes. The pseudo-terminal has a single
server-side size; successful take operations update that size. Repeated take
calls are idempotent for event purposes: attach streams report `ControlChanged`
only when `primaryAttachId` changes, and `PseudoTerminalResized` only when the
pseudo-terminal size changes.

`WriteTerminalInput` starts by binding the input stream to one live attach owned
by the current WebTransport session. If that attach stops being the live primary
attach while the input stream is open, the daemon fails the stream with
`WriteTerminalInputError.NotPrimaryAttach`.

`TerminalSessionInfo.lastKnownCwd` is a best-effort hint for the last working
directory the daemon learned for the hosted shell. The daemon may learn this
from shell integration escape sequences such as OSC 7 or from platform-specific
inspection. When the daemon learns a new value, attach streams may report
`WorkingDirectoryChanged`. When this event is derived from output bytes, it is
reported after the `OutputChunk` containing the bytes that caused the update.
These hints may be absent or stale, and the raw terminal byte stream remains
authoritative for terminal rendering.

`CreateTerminalSessionReq.title` is an initial title hint. After creation,
`TerminalSessionInfo.lastKnownTitle` is the best-effort title the daemon learned
for the hosted terminal. The daemon may learn this from terminal title escape
sequences such as OSC 0 or OSC 2. When the daemon learns a new value, attach
streams may report `TitleChanged`. When this event is derived from output bytes,
it is reported after the `OutputChunk` containing the bytes that caused the
update.
