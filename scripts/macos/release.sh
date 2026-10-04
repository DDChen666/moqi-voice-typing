#!/bin/zsh
# Build a signed macOS release of Moqi: the .dmg for new installs, the
# .app.tar.gz the updater downloads, its signature, and the darwin-aarch64
# entry of latest.json. Uploads nothing; publishing the GitHub Release is a
# manual step (docs/發布新版.md).
#
#   APPLE_SIGNING_IDENTITY="Moqi Dev" scripts/macos/release.sh \
#     --notes "這一版改了什麼" [--latest path/to/windows/latest.json]
#
# --latest merges the macOS entry into the latest.json the Windows release
# made, so both platforms share one file (the app reads only one).
# The updater key lives outside the repository (default
# ~/.moqi-signing/moqi-updater.key). Never commit it.
set -euo pipefail

notes=""
key="$HOME/.moqi-signing/moqi-updater.key"
latest_in=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --notes) notes="$2"; shift 2 ;;
    --key) key="$2"; shift 2 ;;
    --latest) latest_in="$2"; shift 2 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

root="${0:A:h:h:h}"
cd "$root"
[[ -f "$key" ]] || { echo "Updater key not found: $key" >&2; exit 1; }
[[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] || {
  echo "Set APPLE_SIGNING_IDENTITY (a fixed identity keeps users' permissions across updates; see BUILD.md)." >&2
  exit 1
}

# tauri.updater.conf.json turns on the updater artifacts for this build only,
# so everyday builds don't need the key. The key goes in through the
# environment of this one command and is never printed or written anywhere.
TAURI_SIGNING_PRIVATE_KEY="$(cat "$key")" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${MOQI_RELEASE_KEY_PASSWORD-}" \
  bun run tauri build --bundles app,dmg --config src-tauri/tauri.updater.conf.json

version=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
bundle="src-tauri/target/release/bundle"
tarball="$bundle/macos/Moqi.app.tar.gz"
dmg="$bundle/dmg/Moqi_${version}_aarch64.dmg"
[[ -f "$tarball.sig" ]] || { echo "No updater signature: $tarball.sig" >&2; exit 1; }
[[ -f "$dmg" ]] || { echo "No disk image: $dmg" >&2; exit 1; }

out="$root/dist-release"
mkdir -p "$out"
update_name="Moqi_${version}_aarch64.app.tar.gz"
cp "$dmg" "$out/"
cp "$tarball" "$out/$update_name"
cp "$tarball.sig" "$out/$update_name.sig"

python3 - "$out" "$version" "$notes" "$update_name" "$latest_in" <<'EOF'
import datetime, json, os, sys
out, version, notes, name, latest_in = sys.argv[1:6]
if latest_in:
    latest = json.load(open(latest_in, encoding="utf-8-sig"))
    if latest.get("version") != version:
        sys.exit(f"{latest_in} is for {latest.get('version')}, this build is {version}")
else:
    latest = {
        "version": version,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {},
    }
latest.setdefault("platforms", {})["darwin-aarch64"] = {
    "signature": open(os.path.join(out, name + ".sig")).read().strip(),
    "url": f"https://github.com/DDChen666/moqi-voice-typing/releases/download/v{version}/{name}",
}
with open(os.path.join(out, "latest.json"), "w", encoding="utf-8") as f:
    json.dump(latest, f, ensure_ascii=False, indent=2)
print("platforms in latest.json:", ", ".join(latest["platforms"]))
EOF

echo "Release files in $out:"
ls -1 "$out"
