#!/bin/sh
# Builds Odak and installs it for the current user (no root needed):
#   ~/.local/bin/odak, an app menu entry and its icon. Run again to update.
# Remove with: tools/install_linux.sh --uninstall
set -e
cd "$(dirname "$0")/.."

APP_ID=seindtask
BIN="$HOME/.local/bin/odak"
DESKTOP="${XDG_DATA_HOME:-$HOME/.local/share}/applications/$APP_ID.desktop"
ICON="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps/$APP_ID.png"

if [ "$1" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$APP_ID.desktop"
    echo "Odak kaldırıldı. Görevlerin ${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID klasöründe duruyor."
    exit 0
fi

cargo build --release
install -Dm755 target/release/seindtask "$BIN"

# The largest image in icon.ico (its entries are PNGs) becomes the app icon.
mkdir -p "$(dirname "$ICON")"
python3 - assets/icon.ico "$ICON" <<'PY'
import struct, sys
data = open(sys.argv[1], "rb").read()
count = struct.unpack_from("<H", data, 4)[0]
entries = [struct.unpack_from("<BBBBHHII", data, 6 + 16 * i) for i in range(count)]
size, offset = max(((e[0] or 256), e) for e in entries)[1][6:8]
open(sys.argv[2], "wb").write(data[offset:offset + size])
PY

mkdir -p "$(dirname "$DESKTOP")"
cat > "$DESKTOP" <<DESK
[Desktop Entry]
Type=Application
Name=Odak
GenericName=Görev yöneticisi
Comment=Hızlı, yerel görev yöneticisi
Exec="$BIN"
Icon=$APP_ID
Terminal=false
Categories=Office;ProjectManagement;
Keywords=task;todo;görev;yapılacak;
StartupWMClass=$APP_ID
SingleMainWindow=true
DESK

update-desktop-database "$(dirname "$DESKTOP")" 2>/dev/null || true
gtk-update-icon-cache -q "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" 2>/dev/null || true
echo "Odak kuruldu: uygulama menüsünde \"Odak\" olarak görünür ($BIN)."
