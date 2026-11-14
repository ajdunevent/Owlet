#!/usr/bin/env python3
"""Downloads KitchenOwl's item icons from Icons8 as 100 px PNGs into data/resources/item-icons/.

Icons8 allows PNGs up to 100 px free with attribution. Each name in data/item-icons.txt is
fetched as <name>.png and listed in item-icons.gresource.xml; existing files are kept unless --force is given. A second field on a line
overrides the Icons8 name, gives id:<id> for an icon in another style, or is "drawn" for an
icon of Owlet's own, rendered from data/item-icon-drawings/<name>.svg with rsvg-convert.

Usage: build-aux/fetch-item-icons.py [--style ios] [--force] [--base-url URL]
"""

import argparse
import pathlib
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
NAMES = ROOT / "data" / "item-icons.txt"
OUT = ROOT / "data" / "resources" / "item-icons"
DRAWINGS = ROOT / "data" / "item-icon-drawings"
SIZE = 100


def png_size(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


def write_manifest():
    files = "".join(
        f'    <file>{path.name}</file>\n' for path in sorted(OUT.glob("*.png"))
    )
    (OUT / "item-icons.gresource.xml").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        "<gresources>\n"
        '  <gresource prefix="/net/dunevent/Owlet/item-icons">\n'
        f"{files}"
        "  </gresource>\n"
        "</gresources>\n"
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--style", default="ios", help="Icons8 style id (default: ios)")
    parser.add_argument("--force", action="store_true", help="replace icons already downloaded")
    parser.add_argument("--base-url", default="https://img.icons8.com")
    args = parser.parse_args()

    OUT.mkdir(parents=True, exist_ok=True)
    entries = []
    for line in NAMES.read_text().splitlines():
        fields = line.split("#", 1)[0].split()
        if fields:
            entries.append((fields[0], fields[1] if len(fields) > 1 else None))
    fetched, kept, drawn, failed = 0, 0, 0, []
    for name, source in entries:
        target = OUT / f"{name}.png"
        if source == "drawn":
            svg = DRAWINGS / f"{name}.svg"
            size = str(SIZE)
            subprocess.run(
                ["rsvg-convert", "-w", size, "-h", size, "-o", target, svg], check=True
            )
            drawn += 1
            continue
        if target.exists() and not args.force:
            kept += 1
            continue
        if source and source.startswith("id:"):
            url = f"{args.base_url}/?size={SIZE}&id={source[3:]}&format=png&color=000000"
        else:
            # Icons8 names its icons with dashes; KitchenOwl stores them with underscores.
            icon = source or name.replace("_", "-")
            url = f"{args.base_url}/{args.style}/{SIZE}/000000/{icon}.png"
        request = urllib.request.Request(url, headers={"User-Agent": "Owlet icon fetch"})
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                data = response.read()
        except urllib.error.URLError as err:
            failed.append((name, str(err)))
            continue
        if png_size(data) != (SIZE, SIZE):
            failed.append((name, f"not a {SIZE}x{SIZE} PNG"))
            continue
        target.write_bytes(data)
        fetched += 1
        time.sleep(0.2)

    write_manifest()
    print(f"{fetched} downloaded, {kept} already present, {drawn} drawn, {len(failed)} failed")
    for name, reason in failed:
        print(f"  {name}: {reason}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
