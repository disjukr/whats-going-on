# whats-going-on (wgo)

An AI-era remote shell/explorer prototype.

The product-level model is one daemon per machine. Internally each OS backend
can choose the process topology that fits that OS. The first backend is Windows
and uses a system service plus a per-user tray daemon.

## Layout

- `daemon/core`: shared Rust protocol, config, pairing, and service traits.
- `daemon/host`: shared daemon runtime for WebTransport, RPC, auth, filesystem subscriptions, and TLS.
- `daemon/windows`: Windows-specific system/user daemon binaries.
- `daemon/macos`: macOS-specific system/user daemon binaries.
- `protocol`: BDL schemas, RPC/wire standards, and protocol docs.
- `web`: Vite + React + TypeScript browser client, managed with Deno.

See `protocol/README.md` for protocol layer terminology. In short, `wgo-wire` is
the byte-level envelope family carried over WebTransport reqres streams and
datagrams, and `wgo-rpc` defines proc ids, stream shapes, payload schemas, and
method errors.

## Development

Run the currently implemented daemon pair:

```sh
deno task windows:dev:daemons
```

If `tmp/dev/system-wgo.yaml` does not exist, the system daemon creates it. When
Tailscale is installed, the generated config uses `tailscale status --json` to
prefill `domain` from this machine's MagicDNS name. Otherwise, edit that file
and set `domain` to the Windows machine's Tailscale hostname, or add explicit
`tls` certificate paths. The daemon keeps running and enables transport once the
config is valid.

Run the macOS daemon pair in dev mode. The system daemon is launched with
`sudo`; the user daemon runs as the current user.

```sh
deno task macos:dev:daemons
```

Use the same generated `tmp/dev/system-wgo.yaml` flow for macOS.

Stop any detached dev daemons:

```sh
deno task windows:kill:daemons
```

```sh
deno task macos:kill:daemons
```

Check the daemon RPC endpoint:

```sh
cd web
deno task check:daemon
```

Create a short-lived pairing code for the dev daemon:

```sh
deno task windows:pair:dev
```

```sh
deno task macos:pair:dev
```

Enter the printed code in the web client's pairing field. The browser stores the
returned client id and client secret in `localStorage`.

Use a trusted certificate by adding a domain and certificate files to the daemon
config:

```yaml
domain: pc.example.com
tls:
  certFile: /etc/wgo/cert.pem
  keyFile: /etc/wgo/key.pem
```

If `domain` ends in `.ts.net` and `tls` is omitted, the daemon runs
`tailscale
cert --min-validity=168h` and loads the generated Let's Encrypt
certificate from the config directory.

```yaml
domain: minipc.example-tailnet.ts.net
```

Certificate reloads are live for new WebTransport handshakes. Config and PEM
changes are detected with filesystem events. Managed `.ts.net` certificates also
run an hourly scheduled refresh.

Run the web client:

```sh
cd web
deno task dev
```

## Windows Packaging

Build an unsigned Windows daemon MSI package:

```sh
deno task windows:package:daemon
```

The default Windows packaging task uses WiX Toolset to write
`dist/windows/wgo-windows-daemon-<version>.msi`. Install the .NET SDK first; the
script restores the repo-local WiX CLI tool and required WiX extensions
automatically when `wix` is not already on `PATH`.

```sh
dotnet tool restore
deno task windows:package:daemon
```

Install the MSI from an elevated prompt, or double-click it and accept the UAC
prompt:

```sh
msiexec /i .\dist\windows\wgo-windows-daemon-0.1.0.msi
```

The MSI installs `wgo-windows-system.exe` and `wgo-windows-user.exe` under
`%ProgramFiles%\WhatsGoingOn`, registers `wgo-windows-system` as an automatic
LocalSystem service, starts the service during install, launches the tray app
once when install finishes, creates a Start Menu shortcut for the tray app, and
adds an HKLM Run entry so the tray app starts on user logon. The installer uses
the standard WiX wizard UI, including a completion dialog. Daemon data under
`%ProgramData%\WhatsGoingOn` is intentionally outside the install directory and
is not removed by uninstall.

The MSI is intentionally unsigned for now. Windows may still show an unknown
publisher or SmartScreen warning for downloaded installers, but MSI packaging
does not require trusting a development certificate before install.

Uninstall any earlier MSIX package before installing the MSI because both
packages own the same Windows service name.

Build the older development MSIX package:

```sh
deno task windows:package:daemon:msix
```

The MSIX script stages `wgo-windows-system.exe`, `wgo-windows-user.exe`,
generated app icons, and an `AppxManifest.xml`, then invokes the Windows SDK
`MakeAppx.exe` tool. By default it also creates a development code-signing
certificate and signs the package. The generated `.cer` must be trusted on the
test machine before the MSIX can be installed. The trust task requests elevation
when needed:

```sh
deno task windows:trust:daemon:dev-cert
```

```sh
Add-AppxPackage .\dist\windows\wgo-windows-daemon-0.1.0.msix
```

After installing, launch Whats Going On from the Start menu once if you want the
tray icon immediately.

Passing `-SkipSign` writes `wgo-windows-daemon-0.1.0.unsigned.msix` so it cannot
accidentally replace the signed installable package.

The MSIX manifest declares `wgo-windows-system.exe` as a delayed-start
LocalSystem packaged service and `wgo-windows-user.exe` as the interactive tray
app. Uninstalling the package removes the packaged service and app binaries.
Daemon data under `%ProgramData%\WhatsGoingOn` is intentionally outside the
package and is not removed by MSIX uninstall.

For production signing, pass `-CertificatePath` and `-CertificatePassword` to
`scripts/windows/package-daemon.ps1`.

## macOS Packaging

Build a macOS app bundle and DMG:

```sh
deno task macos:package:daemon
```

The DMG contains `Whats Going On.app` and an `/Applications` shortcut, laid out
for the usual drag-to-install flow. The app bundle is the per-user tray app and
installer controller.

On first launch from the DMG, the app prompts to install itself. Accepting the
prompt copies the app to `/Applications`, installs the privileged system daemon
under `/Library/Application Support/wgo/bin`, installs the LaunchDaemon and
LaunchAgent plists, and starts the daemon pair. macOS asks for an administrator
password because the system daemon runs through `/Library/LaunchDaemons`.

The app bundle also exposes Docker Desktop-style CLI entry points:

```sh
/Applications/Whats Going On.app/Contents/MacOS/install
/Applications/Whats Going On.app/Contents/MacOS/uninstall
```

The installed system config lives at:

```sh
/Library/Application Support/wgo/wgo.yaml
```

Logs are written under:

```sh
/Library/Logs/wgo
```

For Developer ID signing, pass a signing identity to the script:

```sh
scripts/macos/package-daemon-dmg.sh --sign "Developer ID Application: Example"
```
