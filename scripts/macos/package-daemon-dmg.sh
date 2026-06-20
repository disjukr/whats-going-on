#!/usr/bin/env bash
set -euo pipefail

VERSION=""
OUT_DIR=""
APP_NAME="Whats Going On"
SIGN_IDENTITY=""
SKIP_BUILD=0
SKIP_DMG=0

usage() {
  cat <<'EOF'
Usage: scripts/macos/package-daemon-dmg.sh [options]

Options:
  --version VERSION      Package version. Defaults to daemon/macos/Cargo.toml.
  --out-dir DIR         Output directory. Defaults to dist/macos.
  --app-name NAME       App bundle display name. Defaults to "Whats Going On".
  --sign IDENTITY       Code signing identity for codesign.
  --skip-build          Reuse target/release daemon binaries.
  --skip-dmg            Build only the .app bundle.
  -h, --help            Show this help.
EOF
}

new_icns_from_svg() {
  local source_svg="$1"
  local destination_icns="$2"
  local iconset_dir="$3"

  if ! command -v deno >/dev/null 2>&1; then
    echo "Warning: deno was not found; skipping macOS app icon rendering." >&2
    return 1
  fi
  if ! command -v iconutil >/dev/null 2>&1; then
    echo "Warning: iconutil was not found; skipping macOS app icon rendering." >&2
    return 1
  fi

  rm -rf "$iconset_dir"
  mkdir -p "$iconset_dir"

  local renderer_path
  if ! renderer_path="$(mktemp "${TMPDIR:-/tmp}/wgo-render-macos-icon.XXXXXX")"; then
    echo "Warning: failed to create temporary icon renderer; continuing without .icns." >&2
    return 1
  fi
  cat >"$renderer_path" <<'EOF'
import path from "node:path";
import sharp from "npm:sharp@0.33.5";

const [sourceSvg, iconsetDir] = Deno.args;
const icons = [
  ["icon_16x16.png", 16],
  ["icon_16x16@2x.png", 32],
  ["icon_32x32.png", 32],
  ["icon_32x32@2x.png", 64],
  ["icon_128x128.png", 128],
  ["icon_128x128@2x.png", 256],
  ["icon_256x256.png", 256],
  ["icon_256x256@2x.png", 512],
  ["icon_512x512.png", 512],
  ["icon_512x512@2x.png", 1024],
];

for (const [name, size] of icons) {
  await sharp(sourceSvg)
    .resize(size, size, {
      fit: "contain",
      background: { r: 0, g: 0, b: 0, alpha: 0 },
    })
    .png()
    .toFile(path.join(iconsetDir, name));
}
EOF

  if ! deno run --quiet --no-lock -A "$renderer_path" "$source_svg" "$iconset_dir" >/dev/null 2>&1; then
    rm -f "$renderer_path"
    echo "Warning: failed to render macOS app icon; continuing without .icns." >&2
    return 1
  fi
  rm -f "$renderer_path"
  if ! iconutil -c icns "$iconset_dir" -o "$destination_icns"; then
    echo "Warning: failed to build macOS .icns; continuing without app icon." >&2
    return 1
  fi
}

layout_dmg_volume() {
  local volume_name="$1"
  local mount_dir="$2"
  local app_name="$3"

  osascript <<EOF
tell application "Finder"
  tell disk "$volume_name"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {120, 120, 700, 440}
    set view_options to the icon view options of container window
    set arrangement of view_options to not arranged
    set icon size of view_options to 112
    set text size of view_options to 13
    set position of item "$app_name.app" of container window to {165, 160}
    set position of item "Applications" of container window to {415, 160}
    update without registering applications
    delay 1
    close
  end tell
end tell
EOF

  SetFile -a C "$mount_dir" >/dev/null 2>&1 || true
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:?missing value for --version}"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="${2:?missing value for --out-dir}"
      shift 2
      ;;
    --app-name)
      APP_NAME="${2:?missing value for --app-name}"
      shift 2
      ;;
    --sign)
      SIGN_IDENTITY="${2:?missing value for --sign}"
      shift 2
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --skip-dmg)
      SKIP_DMG=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS app/dmg packaging must be built on macOS." >&2
  exit 1
fi

if [[ -z "$VERSION" ]]; then
  VERSION="$(
    sed -nE 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' \
      "$REPO_ROOT/daemon/macos/Cargo.toml" |
      head -n 1
  )"
fi
if [[ -z "$VERSION" ]]; then
  echo "Could not infer package version from daemon/macos/Cargo.toml." >&2
  exit 1
fi

if [[ -z "$OUT_DIR" ]]; then
  OUT_DIR="$REPO_ROOT/dist/macos"
fi

if [[ "$SKIP_BUILD" != "1" ]]; then
  (cd "$REPO_ROOT" && cargo build -p wgo-macos-daemon --release --bins)
fi

RELEASE_DIR="$REPO_ROOT/target/release"
SYSTEM_EXE="$RELEASE_DIR/wgo-macos-system"
USER_EXE="$RELEASE_DIR/wgo-macos-user"
if [[ ! -x "$SYSTEM_EXE" ]]; then
  echo "Missing release binary: $SYSTEM_EXE" >&2
  exit 1
fi
if [[ ! -x "$USER_EXE" ]]; then
  echo "Missing release binary: $USER_EXE" >&2
  exit 1
fi

SYSTEM_LABEL="com.disjukr.whats-going-on.system"
USER_LABEL="com.disjukr.whats-going-on.user"
APP_SUPPORT_DIR="/Library/Application Support/wgo"
BIN_DIR="$APP_SUPPORT_DIR/bin"
LOG_DIR="/Library/Logs/wgo"
SYSTEM_PLIST="/Library/LaunchDaemons/$SYSTEM_LABEL.plist"
USER_PLIST="/Library/LaunchAgents/$USER_LABEL.plist"
APP_DEST="/Applications/$APP_NAME.app"
SYSTEM_DAEMON_EXE="$BIN_DIR/wgo-macos-system"
APP_USER_LAUNCHER="$APP_DEST/Contents/MacOS/wgo-macos-app"

PACKAGE_BASE_NAME="wgo-macos-daemon-$VERSION"
STAGING_DIR="$OUT_DIR/$PACKAGE_BASE_NAME-app"
APP_PATH="$STAGING_DIR/$APP_NAME.app"
DMG_ROOT="$STAGING_DIR/dmg"
DMG_PATH="$OUT_DIR/$PACKAGE_BASE_NAME.dmg"
TMP_DMG_PATH="$OUT_DIR/$PACKAGE_BASE_NAME.tmp.dmg"
RW_DMG_PATH="$OUT_DIR/$PACKAGE_BASE_NAME.rw.dmg"
MOUNT_DIR="$STAGING_DIR/mount"
VOLUME_NAME="$APP_NAME $VERSION"

rm -rf "$STAGING_DIR"
mkdir -p \
  "$APP_PATH/Contents/MacOS" \
  "$APP_PATH/Contents/Resources" \
  "$DMG_ROOT"

install -m 0755 "$USER_EXE" "$APP_PATH/Contents/Resources/wgo-macos-user"
install -m 0755 "$SYSTEM_EXE" "$APP_PATH/Contents/Resources/wgo-macos-system"
install -m 0644 "$REPO_ROOT/wgo.svg" "$APP_PATH/Contents/Resources/wgo.svg"
APP_ICON_PLIST=""
if new_icns_from_svg \
  "$REPO_ROOT/wgo.svg" \
  "$APP_PATH/Contents/Resources/wgo.icns" \
  "$STAGING_DIR/wgo.iconset"; then
  APP_ICON_PLIST="  <key>CFBundleIconFile</key>
  <string>wgo</string>"
fi

cat >"$APP_PATH/Contents/MacOS/wgo-macos-app" <<EOF
#!/usr/bin/env bash
set -euo pipefail

CONTENTS_DIR="\$(cd "\$(dirname "\$0")/.." && pwd)"
export WGO_APP_BUNDLE_PATH="\$CONTENTS_DIR/.."
export WGO_APP_INSTALL_PATH="$APP_DEST"
export WGO_SYSTEM_LABEL="$SYSTEM_LABEL"
export WGO_USER_LABEL="$USER_LABEL"
exec "\$CONTENTS_DIR/Resources/wgo-macos-user" run
EOF
chmod 0755 "$APP_PATH/Contents/MacOS/wgo-macos-app"

cat >"$APP_PATH/Contents/MacOS/install" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if [[ "\${EUID:-\$(/usr/bin/id -u)}" -ne 0 ]]; then
  exec /usr/bin/sudo /bin/bash "\$0" "\$@"
fi

SOURCE_APP="\$(cd "\$(dirname "\$0")/../.." && pwd)"
DEST_APP="$APP_DEST"
SYSTEM_LABEL="$SYSTEM_LABEL"
USER_LABEL="$USER_LABEL"
APP_SUPPORT_DIR="$APP_SUPPORT_DIR"
BIN_DIR="$BIN_DIR"
LOG_DIR="$LOG_DIR"
SYSTEM_PLIST="$SYSTEM_PLIST"
USER_PLIST="$USER_PLIST"
SYSTEM_DAEMON_EXE="$SYSTEM_DAEMON_EXE"

if [[ ! -d "\$SOURCE_APP" ]]; then
  echo "Missing source app: \$SOURCE_APP" >&2
  exit 1
fi

echo "Installing \$DEST_APP"
/bin/mkdir -p "\$BIN_DIR" "\$LOG_DIR"
/usr/sbin/chown -R root:wheel "\$APP_SUPPORT_DIR"
/bin/chmod 0755 "\$APP_SUPPORT_DIR" "\$BIN_DIR"
/usr/sbin/chown root:wheel "\$LOG_DIR"
/bin/chmod 1777 "\$LOG_DIR"

/bin/launchctl bootout system "\$SYSTEM_PLIST" >/dev/null 2>&1 || true

console_user="\$(/usr/bin/stat -f %Su /dev/console 2>/dev/null || true)"
console_uid=""
if [[ -n "\$console_user" && "\$console_user" != "root" ]]; then
  console_uid="\$(/usr/bin/id -u "\$console_user" 2>/dev/null || true)"
  if [[ -n "\$console_uid" ]]; then
    /bin/launchctl asuser "\$console_uid" /bin/launchctl bootout "gui/\$console_uid" "\$USER_PLIST" >/dev/null 2>&1 || true
  fi
fi

if [[ "\$SOURCE_APP" != "\$DEST_APP" ]]; then
  /bin/rm -rf "\$DEST_APP"
  /usr/bin/ditto "\$SOURCE_APP" "\$DEST_APP"
fi
/usr/sbin/chown -R root:wheel "\$DEST_APP"

/usr/bin/install -m 0755 -o root -g wheel "\$DEST_APP/Contents/Resources/wgo-macos-system" "\$SYSTEM_DAEMON_EXE"

/bin/cp "\$DEST_APP/Contents/Resources/\$SYSTEM_LABEL.plist" "\$SYSTEM_PLIST"
/usr/sbin/chown root:wheel "\$SYSTEM_PLIST"
/bin/chmod 0644 "\$SYSTEM_PLIST"
/bin/launchctl bootstrap system "\$SYSTEM_PLIST" >/dev/null 2>&1 || true
/bin/launchctl enable "system/\$SYSTEM_LABEL" >/dev/null 2>&1 || true
/bin/launchctl kickstart -k "system/\$SYSTEM_LABEL" >/dev/null 2>&1 || true

/bin/cp "\$DEST_APP/Contents/Resources/\$USER_LABEL.plist" "\$USER_PLIST"
/usr/sbin/chown root:wheel "\$USER_PLIST"
/bin/chmod 0644 "\$USER_PLIST"

if [[ -n "\$console_uid" ]]; then
  /bin/launchctl asuser "\$console_uid" /bin/launchctl bootstrap "gui/\$console_uid" "\$USER_PLIST" >/dev/null 2>&1 || true
  /bin/launchctl asuser "\$console_uid" /bin/launchctl enable "gui/\$console_uid/\$USER_LABEL" >/dev/null 2>&1 || true
  /bin/launchctl asuser "\$console_uid" /bin/launchctl kickstart -k "gui/\$console_uid/\$USER_LABEL" >/dev/null 2>&1 || true
fi

echo "Installed Whats Going On."
echo "Config: $APP_SUPPORT_DIR/wgo.yaml"
echo "System daemon: $SYSTEM_DAEMON_EXE"
echo "Logs: $LOG_DIR"
EOF

cat >"$APP_PATH/Contents/MacOS/uninstall" <<EOF
#!/usr/bin/env bash
set -euo pipefail

if [[ "\${EUID:-\$(/usr/bin/id -u)}" -ne 0 ]]; then
  exec /usr/bin/sudo /bin/bash "\$0" "\$@"
fi

SYSTEM_LABEL="$SYSTEM_LABEL"
USER_LABEL="$USER_LABEL"
SYSTEM_PLIST="$SYSTEM_PLIST"
USER_PLIST="$USER_PLIST"
SYSTEM_DAEMON_EXE="$SYSTEM_DAEMON_EXE"

echo "Uninstalling Whats Going On daemons"
/bin/launchctl bootout system "\$SYSTEM_PLIST" >/dev/null 2>&1 || true

console_user="\$(/usr/bin/stat -f %Su /dev/console 2>/dev/null || true)"
if [[ -n "\$console_user" && "\$console_user" != "root" ]]; then
  console_uid="\$(/usr/bin/id -u "\$console_user" 2>/dev/null || true)"
  if [[ -n "\$console_uid" ]]; then
    /bin/launchctl asuser "\$console_uid" /bin/launchctl bootout "gui/\$console_uid" "\$USER_PLIST" >/dev/null 2>&1 || true
  fi
fi

/bin/rm -f "\$SYSTEM_PLIST" "\$USER_PLIST" "\$SYSTEM_DAEMON_EXE"
/bin/rmdir "$BIN_DIR" >/dev/null 2>&1 || true

echo "Removed launchd jobs and system daemon."
echo "The app bundle and configuration files were left in place."
EOF

chmod 0755 \
  "$APP_PATH/Contents/MacOS/wgo-macos-app" \
  "$APP_PATH/Contents/MacOS/install" \
  "$APP_PATH/Contents/MacOS/uninstall"

cat >"$APP_PATH/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>wgo-macos-app</string>
  <key>CFBundleIdentifier</key>
  <string>com.disjukr.whats-going-on</string>
$APP_ICON_PLIST
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF
plutil -lint "$APP_PATH/Contents/Info.plist" >/dev/null

cat >"$APP_PATH/Contents/Resources/$SYSTEM_LABEL.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$SYSTEM_LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$SYSTEM_DAEMON_EXE</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/system.out.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/system.err.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RUST_LOG</key>
    <string>info</string>
  </dict>
</dict>
</plist>
EOF

cat >"$APP_PATH/Contents/Resources/$USER_LABEL.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$USER_LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$APP_USER_LAUNCHER</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/user.out.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/user.err.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RUST_LOG</key>
    <string>info</string>
  </dict>
</dict>
</plist>
EOF
plutil -lint \
  "$APP_PATH/Contents/Resources/$SYSTEM_LABEL.plist" \
  "$APP_PATH/Contents/Resources/$USER_LABEL.plist" >/dev/null

if [[ -n "$SIGN_IDENTITY" ]]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" \
    "$APP_PATH/Contents/Resources/wgo-macos-system"
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" \
    "$APP_PATH/Contents/Resources/wgo-macos-user"
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" \
    "$APP_PATH"
fi

if [[ "$SKIP_DMG" == "1" ]]; then
  echo "Wrote app: $APP_PATH"
  exit 0
fi

cp -R "$APP_PATH" "$DMG_ROOT/$APP_NAME.app"
ln -s /Applications "$DMG_ROOT/Applications"

rm -f "$TMP_DMG_PATH"
rm -f "$RW_DMG_PATH"
rm -rf "$MOUNT_DIR"
mkdir -p "$MOUNT_DIR"
DMG_SIZE_MB="$(du -sm "$DMG_ROOT" | awk '{ print $1 + 64 }')"
hdiutil create \
  -size "${DMG_SIZE_MB}m" \
  -fs HFS+ \
  -volname "$VOLUME_NAME" \
  "$RW_DMG_PATH"
hdiutil attach "$RW_DMG_PATH" -mountpoint "$MOUNT_DIR" -nobrowse -quiet
/usr/bin/ditto "$DMG_ROOT" "$MOUNT_DIR"
if ! layout_dmg_volume "$VOLUME_NAME" "$MOUNT_DIR" "$APP_NAME"; then
  echo "Warning: failed to apply Finder DMG layout; continuing with default layout." >&2
fi
hdiutil detach "$MOUNT_DIR" -quiet
hdiutil convert "$RW_DMG_PATH" \
  -ov \
  -format UDZO \
  -imagekey zlib-level=9 \
  -o "$TMP_DMG_PATH"
mv -f "$TMP_DMG_PATH" "$DMG_PATH"
rm -f "$RW_DMG_PATH"

echo "Wrote app: $APP_PATH"
echo "Wrote dmg: $DMG_PATH"
