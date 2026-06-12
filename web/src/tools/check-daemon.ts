import {
  DatagramMessageKind,
  decodeDatagramMessage,
  encodeDatagramMessage,
} from "../protocol/wire.ts";

interface Args {
  url?: string;
  config: string;
  timeoutMs: number;
}

const args = parseArgs(Deno.args[0] === "--" ? Deno.args.slice(1) : Deno.args);
const configTls = await readConfigTls(args.config);
if (!configTls.trustedTls) {
  throw new Error(
    `${args.config} must configure tls or a .ts.net domain; self-signed hash pinning is not supported`,
  );
}
const baseUrl = args.url ?? configTls.defaultUrl ?? "https://localhost:9012";

const rpcUrl = rpcEndpoint(baseUrl);

console.log(`connecting ${rpcUrl} with Deno WebTransport`);
const latencyMs = await checkWebTransport(rpcUrl, args.timeoutMs);
console.log(`connected; datagram ping ${formatLatency(latencyMs)}`);

function parseArgs(argv: string[]): Args {
  const args: Args = {
    config: decodeURIComponent(
      new URL("../../../tmp/dev/system-wgo.yaml", import.meta.url).pathname,
    ),
    timeoutMs: 10_000,
  };

  if (Deno.build.os === "windows") {
    args.config = args.config.replace(/^\//, "");
  }

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!;
    if (arg === "--url") args.url = requireValue(argv, ++i, arg);
    else if (arg === "--config") args.config = requireValue(argv, ++i, arg);
    else if (arg === "--timeout-ms") {
      args.timeoutMs = Number(requireValue(argv, ++i, arg));
    } else if (arg === "--help" || arg === "-h") {
      printHelp();
      Deno.exit(0);
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return args;
}

function printHelp() {
  console.log(`Usage:
  deno task check:daemon
  deno run --unstable-net --allow-net --allow-read=.. src/tools/check-daemon.ts [--url https://localhost:9012] [--config ../tmp/dev/system-wgo.yaml]
`);
}

function requireValue(argv: string[], index: number, flag: string): string {
  const value = argv[index];
  if (!value) throw new Error(`${flag} requires a value`);
  return value;
}

async function readConfigTls(
  configPath: string,
): Promise<{ trustedTls: boolean; defaultUrl?: string }> {
  const yaml = await Deno.readTextFile(configPath);
  const domain = yaml.match(/^\s*domain:\s*["']?([^"'\s#]+)["']?/m)?.[1]
    ?.trim()
    .toLowerCase();
  const trustedTls = /^\s*tls:\s*$/m.test(yaml) ||
    !!domain?.endsWith(".ts.net");
  return {
    trustedTls,
    defaultUrl: domain ? `https://${domain}:9012` : undefined,
  };
}

function rpcEndpoint(raw: string): string {
  const url = new URL(raw);
  if (url.protocol !== "https:") {
    throw new Error("WebTransport URL must use https");
  }
  if (url.pathname === "/" || url.pathname === "") url.pathname = "/rpc";
  return url.toString();
}

async function checkWebTransport(
  url: string,
  timeoutMs: number,
): Promise<number> {
  if (typeof WebTransport === "undefined") {
    throw new Error(
      "WebTransport is unavailable. Run with Deno --unstable-net.",
    );
  }

  const transport = new WebTransport(url);

  try {
    await withTimeout(transport.ready, "WebTransport ready", timeoutMs);
    return await withTimeout(
      pingDatagram(transport),
      "datagram ping",
      timeoutMs,
    );
  } finally {
    try {
      transport.close({ closeCode: 0, reason: "done" });
    } catch {
      // Deno throws when closing a WebTransport object that never reached ready.
    }
  }
}

async function pingDatagram(transport: WebTransport): Promise<number> {
  const pingId = 1;
  const writer = transport.datagrams.writable.getWriter();
  const reader = transport.datagrams.readable.getReader();
  const startedAt = performance.now();
  try {
    await writer.write(encodeDatagramMessage({
      kind: DatagramMessageKind.Ping,
      pingId,
    }));
    while (true) {
      const { done, value } = await reader.read();
      if (done) throw new Error("datagram stream closed before pong");
      const message = decodeDatagramMessage(value);
      if (
        message.kind === DatagramMessageKind.Pong &&
        message.pingId === pingId
      ) {
        return performance.now() - startedAt;
      }
      if (message.kind === DatagramMessageKind.Ping) {
        await writer.write(encodeDatagramMessage({
          kind: DatagramMessageKind.Pong,
          pingId: message.pingId,
        }));
      }
    }
  } finally {
    try {
      reader.releaseLock();
    } catch {
      // The reader may already be detached by transport shutdown.
    }
    try {
      writer.releaseLock();
    } catch {
      // The writer may be in an errored state after transport shutdown.
    }
  }
}

function formatLatency(latencyMs: number): string {
  return `${Math.max(1, Math.round(latencyMs))}ms`;
}

async function withTimeout<T>(
  promise: Promise<T>,
  label: string,
  timeoutMs: number,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
