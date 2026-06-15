# wgo protocol

This directory contains the protocol contracts shared by the daemon and web
client. The protocol is split into narrow layers so each document has one job.

## Layers

- `wgo-cbor` defines the deterministic CBOR profile and schema-to-CBOR mapping.
- `wgo-wire` defines byte-level envelopes carried over WebTransport reqres
  streams and datagrams.
- `wgo-rpc` defines RPC proc ids, stream shapes, payload schema selection, and
  method-level error unions.
- `schemas/rpc` defines domain RPC contracts such as pairing and filesystem
  methods.
- `schemas/config` defines local daemon configuration files. These schemas do
  not use RPC-only primitives such as `i53` or `u53`.

## Wire

`wire` means the byte-level envelope family that peers exchange over
WebTransport. It does not mean the whole transport stack, and it does not define
domain method payloads.

`wgo-wire` currently has two envelope shapes:

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

Current proc id registry:

| id | proc                    |
| -: | ----------------------- |
|  1 | `GetDaemonInfo`         |
|  2 | `StartPairing`          |
|  3 | `CompletePairing`       |
|  4 | `RenewClientCredential` |
|  5 | `SubscribeRoots`        |
|  6 | `SubscribeDirectory`    |
|  7 | `ReadFile`              |
|  8 | `WriteFile`             |
|  9 | `CreateNodes`           |
| 10 | `RenamePaths`           |
| 11 | `DeletePaths`           |

`GetDaemonInfo` returns process-level daemon metadata: supported proc ids,
daemon version, and a human-readable OS name for the daemon host. The OS string
should include useful platform-specific details when available, such as Windows
edition, bitness, display version, build, and service pack. The value is fixed
while that daemon process is running, but a reconnect may reach an updated
daemon. Clients may cache daemon info for a live connection or session and
should fetch it again after reconnecting. Protected proc ids may still require
session authentication before invocation.

A proc's `stream` attribute defines request and response cardinality:

- `unary`: unary request, unary response
- `client`: streaming request, unary response
- `server`: unary request, streaming response
- `bidi`: streaming request, streaming response

Normal completion is represented by WebTransport half-close/EOF. There are no
application-level request-end or response-end messages.

## Session Control

Session authentication is part of `wgo-wire`, not an application RPC proc. A
client authenticates the WebTransport session by sending `SessionAuthenticate`
on a session-control stream. The mechanism name identifies the authentication
profile, and the payload is mechanism-specific deterministic CBOR bytes.

Protected RPC procs use the resulting session authentication state.

## Filesystem Model

Filesystem read-side state is modeled as reactive table subscriptions.

- `SubscribeRoots` streams a roots table.
- `SubscribeDirectory` streams a directory entry table.
- The first event is `Snapshot`.
- Later `Snapshot` events replace the whole subscribed table view.
- `Patch` events update table membership with `removes` and `upserts`.
- `Closed` is a domain-level terminal event followed by normal stream close.

Filesystem metadata mutations are best-effort bulk commands:

- `CreateNodes`
- `RenamePaths`
- `DeletePaths`

Bulk mutation responses report item-level results by zero-based request item
index. There is no rollback guarantee. Subscribed table events are the source of
truth for resulting filesystem state.

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
