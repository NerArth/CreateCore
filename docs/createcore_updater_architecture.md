# Project Architecture Summary: Minecraft Modpack Updater

## 1. Project Goals

Frictionless Updates: Provide a seamless, one-click modpack update experience for Windows 10/11 users without requiring manual .zip extractions or folder deletions.

Server Parity: Provide an equally efficient update method for the headless Ubuntu VPS hosting the server.

Avoid OS Restrictions: Bypass Windows SmartScreen warnings and PowerShell Execution Policy blockers by using a compiled native binary rather than scripts.

Licensing Compliance: Aim of respecting author licensing and any reward programmes they may be involved in, by downloading individual .jar files directly from official CDNs (Modrinth, CurseForge) instead of redistributing the files in a custom archive.

Maintainability: Decoupled core execution logic (the binary) from the mod list (the CSV) allows the modpack to be updated indefinitely without requiring users to download a new executable.

## 2. Project Architecture

Core Tool: A single-file compiled Rust executable. Distributed as an .exe for Windows users and a native ELF binary for the Ubuntu server.

Data Source: A comma-separated values (.csv) file that is kept up to date on the Git repository (accessed via a Raw URL), e.g. `CreateCore-v3.0.0.csv` in the `modlists` directory.

CSV Schema Structure: The CSV uses three specific columns:

- FileName: The exact name of the .jar file to be saved to disk.

- Url: The direct CDN download link.

- Side: Specifies the target environment (e.g., Client, Server, Both).

Execution Flow:

- Parse optional command-line flags (e.g., --server).

- Locate the local mods directory and wipe its contents completely.

- Fetch and parse the remote CSV file into memory.

- Filter the download list based on the Side column and the execution flag.

- Download files sequentially directly into the mods folder, providing command-line UI feedback.

- Report final success or display a list of failed downloads.

## 3. Target Environments

Clients: Windows 10 and Windows 11. The .exe is executed directly from within the user's CurseForge/NeoForge instance folder.

Server: Ubuntu VPS (headless environment). The Linux binary is executed via SSH from within the server's root directory, utilizing the --server flag.

## 4. Runtime Considerations

Network Limits: Downloads are strictly sequential. Concurrent downloading is intentionally avoided to prevent bandwidth saturation on weak client connections and to avoid triggering rate-limiting (HTTP 429) from Modrinth or CurseForge CDNs.

URL Stability: Modrinth CDN links are the primary source due to their stability and bot-friendly infrastructure. CurseForge CDN links are secondary. A mod's Git Releases page, if available, may serve as a final fallback proxy in the case of strict CDN exclusions by the mod author.

Client vs. Server Filtering: NeoForge servers may crash if initialized with specific client-side rendering or UI mods. The executable strictly respects the Side column to prevent downloading these files to the Ubuntu VPS.

Memory Overhead: By utilizing Rust, the tool runs bare-metal with zero garbage collection overhead. It consumes minimal RAM and cleans up immediately upon closure, ensuring no lingering processes impact game performance.