#!/bin/sh
# Linux counterpart of release.ps1: builds a release and packs a Velopack AppImage into ./Releases.
# With --publish it also uploads it to GitHub (GH_TOKEN with `repo` scope, or a logged-in `gh`);
# --merge adds the files to the release if release.ps1 already made it for Windows.
# Requirements: .NET SDK, `dotnet tool install -g vpk`, and the .env with the Supabase settings.
set -e
cd "$(dirname "$0")"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -f .env ] || { echo ".env is missing: accounts would be off in this build" >&2; exit 1; }

cargo build --release

# vpk packs a whole folder, so stage only the binary; the icon is the largest image in icon.ico.
stage=target/pack
rm -rf "$stage" && mkdir -p "$stage"
cp target/release/seindtask "$stage/"
python3 - assets/icon.ico target/pack-icon.png <<'PY'
import struct, sys
data = open(sys.argv[1], "rb").read()
count = struct.unpack_from("<H", data, 4)[0]
entries = [struct.unpack_from("<BBBBHHII", data, 6 + 16 * i) for i in range(count)]
size, offset = max(((e[0] or 256), e) for e in entries)[1][6:8]
open(sys.argv[2], "wb").write(data[offset:offset + size])
PY

vpk pack --packId seindtask --packVersion "$version" --packDir "$stage" --mainExe seindtask \
    --packTitle Odak --icon target/pack-icon.png --categories "Office;ProjectManagement" --outputDir Releases

if [ "$1" = "--publish" ]; then
    token=${GH_TOKEN:-$(gh auth token)}
    vpk upload github --repoUrl https://github.com/seind-dev/Odak --token "$token" \
        --publish --merge --releaseName "v$version" --tag "v$version" --outputDir Releases
fi
