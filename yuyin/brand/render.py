"""Render Yuyin's app icon and tray icons from yuyin/brand/render.html.

Usage (from the app/ directory):
    python3 yuyin/brand/render.py                 # every image
    python3 yuyin/brand/render.py tray_win_       # only names starting with a prefix

Writes:
    yuyin/brand/app-icon-1024.png      source for `bun tauri icon`
    src-tauri/resources/tray_*.png     menu bar icons (macOS templates)
    src-tauri/resources/tray_win_*.png notification-area icons (Windows, colour)
    src-tauri/resources/{handy,recording,transcribing,handy_warning}.png
Then run `bun tauri icon yuyin/brand/app-icon-1024.png` to regenerate src-tauri/icons/.

Needs Google Chrome (macOS, Windows) or Microsoft Edge (Windows). Rasterisers
differ slightly between browsers, so pass a prefix when adding new images to
leave the existing PNGs byte-for-byte unchanged.
"""

import base64
import json
import re
import subprocess
import sys
from pathlib import Path

BROWSERS = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
]
HERE = Path(__file__).resolve().parent
APP = HERE.parent.parent


def main():
    browser = next((b for b in BROWSERS if Path(b).exists()), None)
    if browser is None:
        sys.exit("Google Chrome or Microsoft Edge is required")
    prefixes = sys.argv[1:]
    dom = subprocess.run(
        [browser, "--headless=new", "--disable-gpu", "--virtual-time-budget=5000",
         "--dump-dom", (HERE / "render.html").as_uri()],
        capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    match = re.search(r'<pre id="out">(\{.*?)</pre>', dom, re.S)
    if not match or not match.group(1).strip():
        sys.exit("render.html produced no output")
    images = json.loads(match.group(1).replace("&quot;", '"').replace("&amp;", "&"))
    for name, url in images.items():
        if prefixes and not any(name.startswith(p) for p in prefixes):
            continue
        data = base64.b64decode(url.split(",", 1)[1])
        dest = HERE / name if name.startswith("app-icon") else APP / "src-tauri/resources" / name
        dest.write_bytes(data)
        print(f"{dest.relative_to(APP)}  {len(data)} bytes")


if __name__ == "__main__":
    main()
