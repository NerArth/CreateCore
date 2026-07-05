#!/usr/bin/env python3
"""
toml_to_csv.py
--------------
Converts a directory of packwiz .pw.toml files into a CreateCore modlist CSV.

Usage:
    python tools/toml_to_csv.py <packwiz_mods_dir> <output_csv>

Example:
    python tools/toml_to_csv.py packwiz/CreateCore-v2 modlists/CreateCore-v2.0.0.csv

Deduplication logic:
    When two .pw.toml files resolve to the same output filename, the CurseForge
    entry is preferred (it is the canonical version for modpack releases). The
    Modrinth entry is used as a fallback URL source if the CurseForge entry has
    no direct download URL.

Output columns:
    Mod Name Text     - Human-readable mod name (from 'name' field in TOML)
    CDN Release URL   - Direct download URL (Modrinth CDN or manual)
    Client-Server Tag - One of: Both, Client, Server
    Source            - Where the URL was sourced from: modrinth, curseforge, manual

Mods that are CurseForge-only (no direct URL, no Modrinth pair) are skipped and
logged to a '_needs_manual_url.txt' file next to the output CSV.
"""

import sys
import csv
import os

try:
    import tomllib
except ModuleNotFoundError:
    # tomllib is stdlib in Python 3.11+. Fall back to tomli for older versions.
    try:
        import tomli as tomllib  # type: ignore
    except ModuleNotFoundError:
        print("Error: Python 3.11+ is required, or install 'tomli' via: pip install tomli")
        sys.exit(1)


# -- Constants -----------------------------------------------------------------

SIDE_MAP = {
    "both":   "Both",
    "client": "Client",
    "server": "Server",
}

CSV_COLUMNS = ["Mod Name Text", "CDN Release URL", "Client-Server Tag", "Source"]


# -- TOML loading --------------------------------------------------------------

def load_toml(path: str) -> dict:
    with open(path, "rb") as f:
        return tomllib.load(f)


def get_source(data: dict) -> str:
    """Returns 'modrinth' if the TOML has a direct Modrinth URL, else 'curseforge'."""
    mode = data.get("download", {}).get("mode", "")
    if mode == "metadata:curseforge":
        return "curseforge"
    url = data.get("download", {}).get("url", "")
    if "modrinth" in url:
        return "modrinth"
    if url:
        return "manual"
    return "curseforge"


def get_url(data: dict) -> str:
    """Returns the direct download URL, or empty string if none."""
    return data.get("download", {}).get("url", "")


# -- Deduplication -------------------------------------------------------------

def build_mod_table(toml_dir: str) -> tuple:
    """
    Reads all .pw.toml files and returns:
      - A deduplicated list of mod records (dicts with keys matching CSV columns)
      - A list of mod names that need a manual URL
    """
    buckets = {}
    modrinth_map = {}

    # Pass 1: Bucket by filename and index Modrinth entries by name
    for fname in sorted(os.listdir(toml_dir)):
        if not fname.endswith(".pw.toml"):
            continue
        path = os.path.join(toml_dir, fname)
        data = load_toml(path)
        jar = data.get("filename", "")
        if not jar:
            continue
        
        source = get_source(data)
        if jar not in buckets:
            buckets[jar] = {"modrinth": None, "curseforge": None, "_all": []}
        buckets[jar][source] = data
        buckets[jar]["_all"].append(fname)
        
        if source == "modrinth":
            mod_name = data.get("name")
            if mod_name:
                modrinth_map[mod_name] = data

    # Pass 2: Resolve records.
    # consumed_mr_jars: Modrinth jars borrowed by a CF-canonical bucket so we
    # don't write a second row when we reach that Modrinth bucket later.
    records = []
    needs_manual = []
    consumed_mr_jars: set = set()

    for jar, bucket in buckets.items():
        # Skip Modrinth-only buckets that were already consumed as a partner.
        if jar in consumed_mr_jars:
            continue

        cf_data = bucket.get("curseforge")
        mr_data = bucket.get("modrinth")

        # CF entry with no direct URL: look for a same-name Modrinth partner,
        # even if it's a different version (different jar filename/bucket).
        if cf_data and not get_url(cf_data) and mr_data is None:
            mod_name = cf_data.get("name")
            if mod_name and mod_name in modrinth_map:
                partner = modrinth_map[mod_name]
                partner_jar = partner.get("filename", "")
                print(
                    f"  [warn]  '{mod_name}': CF version is '{jar}' but Modrinth "
                    f"version is '{partner_jar}'. Using Modrinth URL — "
                    f"check which version you intend to ship."
                )
                mr_data = partner
                consumed_mr_jars.add(partner_jar)

        canonical = cf_data if cf_data is not None else mr_data
        if not canonical:
            continue

        name = canonical.get("name", "Unknown")
        side_tag = SIDE_MAP.get(canonical.get("side", "both").lower(), "Both")

        cf_url = get_url(cf_data) if cf_data else ""
        mr_url = get_url(mr_data) if mr_data else ""
        url = cf_url or mr_url
        source_label = "curseforge" if cf_url else "modrinth"

        if not url:
            print(f"  [skip]  '{name}' ({jar}): no direct URL. Needs manual entry.")
            needs_manual.append(f"{name}  |  {jar}")
            continue

        records.append({
            "Mod Name Text":     name,
            "CDN Release URL":   url,
            "Client-Server Tag": side_tag,
            "Source":            source_label,
        })

    return records, needs_manual


# -- CSV writing ---------------------------------------------------------------

def write_csv(records: list, output_path: str) -> None:
    os.makedirs(os.path.dirname(output_path) or ".", exist_ok=True)
    with open(output_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=CSV_COLUMNS)
        writer.writeheader()
        writer.writerows(records)
    print(f"\n  Written {len(records)} rows -> {output_path}")


def write_needs_manual(needs_manual: list, output_path: str) -> None:
    manual_path = os.path.splitext(output_path)[0] + "_needs_manual_url.txt"
    with open(manual_path, "w", encoding="utf-8") as f:
        f.write("# These mods have no direct CDN URL and need a manual entry in the CSV.\n")
        f.write("# Format: Mod Name  |  jar filename\n\n")
        for line in needs_manual:
            f.write(line + "\n")
    print(f"  Written {len(needs_manual)} manual-needed entries -> {manual_path}")


# -- Entry point ---------------------------------------------------------------

def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(1)

    toml_dir, output_csv = sys.argv[1], sys.argv[2]

    if not os.path.isdir(toml_dir):
        print(f"Error: '{toml_dir}' is not a directory.")
        sys.exit(1)

    print(f"Reading .pw.toml files from: {toml_dir}")
    print()

    records, needs_manual = build_mod_table(toml_dir)

    # Sort alphabetically by mod name for a stable, readable CSV.
    records.sort(key=lambda r: r["Mod Name Text"].lower())

    write_csv(records, output_csv)

    if needs_manual:
        write_needs_manual(needs_manual, output_csv)
        print(f"\n  [!] {len(needs_manual)} mod(s) need a manual URL -- see the _needs_manual_url.txt file.")
    else:
        print("\n  [ok] All mods resolved automatically.")


if __name__ == "__main__":
    main()
