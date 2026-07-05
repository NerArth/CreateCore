// CreateCore Modpack Updater
//
// Fetches the latest modpack CSV from GitHub and downloads all required mods
// into the local mods/ directory, with client/server side filtering.
//
// Usage:
//   createcore-updater            (client mode — downloads Client + Both mods)
//   createcore-updater --server   (server mode — downloads Server + Both mods)

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Constants ─────────────────────────────────────────────────────────────────

const GITHUB_API_MODLISTS: &str =
    "https://api.github.com/repos/NerArth/CreateCore/contents/modlists";
const RAW_BASE_URL: &str =
    "https://raw.githubusercontent.com/NerArth/CreateCore/refs/heads/main/modlists/";
const USER_AGENT: &str = concat!("createcore-updater/", env!("CARGO_PKG_VERSION"));
const LOG_PATH: &str = "logs/createcoreupdater.log";

// ── Data types ────────────────────────────────────────────────────────────────

/// A single entry returned by the GitHub Contents API.
#[derive(Deserialize)]
struct GithubEntry {
    name: String,
}

/// A mod row parsed from the CSV and filtered by side.
struct ModEntry {
    display_name: String,
    url: String,
}

/// Aggregated results for the run report and log.
struct RunReport {
    version: String,
    mode: &'static str,
    succeeded: Vec<String>,
    failed: Vec<(String, String)>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run() {
        eprintln!("\n[!] Fatal error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // ── Step 0: Parse CLI flags ───────────────────────────────────────────────
    let is_server = std::env::args().any(|a| a == "--server");
    let mode: &'static str = if is_server { "Server" } else { "Client" };

    println!("CreateCore Modpack Updater");
    println!("Mode: {}", mode);
    println!("{}", "─".repeat(40));

    // ── Step 1: Sanity check (client mode only) ───────────────────────────────
    if !is_server {
        sanity_check()?;
    }

    // ── Step 2: Detect latest modpack version ─────────────────────────────────
    let (csv_filename, csv_url) = detect_latest_version()?;
    let version_display = csv_filename.trim_end_matches(".csv").to_string();
    println!("Found latest modpack version: {}", version_display);

    // ── Step 3: Fetch and parse CSV ───────────────────────────────────────────
    // This is the last step before any writes occur. If it fails, the user's
    // existing mods/ directory remains completely untouched.
    let mods = fetch_and_parse_csv(&csv_url, is_server)
        .context("Failed to fetch or parse the modlist CSV")?;
    println!("Fetched modlist: {} mods to download.", mods.len());
    println!("{}", "─".repeat(40));

    // ── Step 4: Optional backup ───────────────────────────────────────────────
    offer_backup()?;

    // ── Step 5: Wipe mods/ ────────────────────────────────────────────────────
    wipe_mods_dir()?;

    // ── Step 6: Download loop ─────────────────────────────────────────────────
    let (succeeded, failed) = download_mods(&mods)?;

    // ── Step 7: Report and log ────────────────────────────────────────────────
    let report = RunReport { version: version_display, mode, succeeded, failed };
    report_and_log(&report)?;

    if !report.failed.is_empty() {
        std::process::exit(1);
    }

    Ok(())
}

// ── Step 1: Sanity check ──────────────────────────────────────────────────────

fn sanity_check() -> Result<()> {
    // Each tuple is (display label, does the path exist?).
    let markers = [
        ("minecraftinstance.json", Path::new("minecraftinstance.json").is_file()),
        ("config/",                Path::new("config").is_dir()),
        ("logs/createcoreupdater.log", Path::new(LOG_PATH).is_file()),
    ];

    let found_count = markers.iter().filter(|(_, exists)| *exists).count();

    if found_count >= 2 {
        // Two or more markers: full confidence, proceed silently.
    } else if found_count == 1 {
        // Exactly one marker: inform the user and give them a chance to abort.
        let found_name = markers
            .iter()
            .find(|(_, exists)| *exists)
            .map(|(name, _)| *name)
            .unwrap_or("unknown");

        println!("[i] Note: Only one instance marker was found ('{}').", found_name);
        println!("    If this is not your CreateCore instance root, press Ctrl+C to cancel.");
        print!("    Continuing in ");
        io::stdout().flush()?;
        for i in (1u8..=3).rev() {
            print!("{}... ", i);
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(1));
        }
        println!();
    } else {
        // No markers found: warn the user and prompt for an override.
        println!("[!] Warning: No instance markers were found.");
        println!("    Expected one or more of:");
        for (name, _) in &markers {
            println!("      - {}", name);
        }
        println!();
        println!("    This binary should be run from your CreateCore instance root folder.");
        println!("    In your launcher, open instance settings and click \"Open Folder\".");
        println!();
        print!("    Are you sure you want to continue from this directory? [y/N]: ");
        io::stdout().flush()?;

        let input = read_line();
        if input.to_lowercase() != "y" {
            println!("Cancelled. No changes were made.");
            std::process::exit(0);
        }
        println!();
    }

    Ok(())
}

// ── Step 2: Dynamic version detection ────────────────────────────────────────

fn detect_latest_version() -> Result<(String, String)> {
    println!("Checking for latest modpack version...");

    let entries: Vec<GithubEntry> = ureq::get(GITHUB_API_MODLISTS)
        .set("User-Agent", USER_AGENT)
        .call()
        .context("Failed to reach the GitHub API. Check your internet connection.")?
        .into_json()
        .context("Failed to parse the GitHub API response.")?;

    // Find the file with the highest semver among all CreateCore-vX.Y.Z.csv entries.
    let best = entries
        .iter()
        .filter_map(|e| parse_csv_version(&e.name).map(|v| (v, e.name.clone())))
        .max_by_key(|(version, _)| *version);

    match best {
        Some((_, filename)) => {
            let url = format!("{}{}", RAW_BASE_URL, filename);
            Ok((filename, url))
        }
        None => bail!(
            "No modlist CSV files found in the repository. \
             Expected files named 'CreateCore-vX.Y.Z.csv' in the modlists/ directory."
        ),
    }
}

/// Parses a filename like `CreateCore-v3.0.0.csv` into a comparable `(major, minor, patch)` tuple.
/// Returns `None` if the filename does not match the expected pattern.
fn parse_csv_version(name: &str) -> Option<(u32, u32, u32)> {
    let inner = name.strip_prefix("CreateCore-v")?.strip_suffix(".csv")?;
    let mut parts = inner.splitn(3, '.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

// ── Step 3: CSV fetch & parse ─────────────────────────────────────────────────

fn fetch_and_parse_csv(csv_url: &str, is_server: bool) -> Result<Vec<ModEntry>> {
    let response = ureq::get(csv_url)
        .set("User-Agent", USER_AGENT)
        .call()
        .context("Failed to fetch the modlist CSV.")?;

    let mut reader = csv::Reader::from_reader(response.into_reader());

    // Resolve column positions by header name so the column order in the CSV does not matter.
    let headers = reader.headers().context("CSV is missing a header row.")?.clone();
    let col_name = header_index(&headers, "Mod Name Text")?;
    let col_url  = header_index(&headers, "CDN Release URL")?;
    let col_side = header_index(&headers, "Client-Server Tag")?;

    let mut mods = Vec::new();

    for result in reader.records() {
        let record = result.context("Failed to read a CSV row.")?;
        let side = record.get(col_side).unwrap_or("").trim();

        let include = match side {
            "Both"   => true,
            "Client" => !is_server,
            "Server" =>  is_server,
            _        => false,
        };

        if include {
            mods.push(ModEntry {
                display_name: record.get(col_name).unwrap_or("Unknown").trim().to_string(),
                url:          record.get(col_url).unwrap_or("").trim().to_string(),
            });
        }
    }

    Ok(mods)
}

/// Returns the index of a named column in a CSV header record.
fn header_index(headers: &csv::StringRecord, name: &str) -> Result<usize> {
    headers
        .iter()
        .position(|h| h == name)
        .with_context(|| format!("CSV is missing the expected column: '{}'", name))
}

// ── Step 4: Optional backup ───────────────────────────────────────────────────

fn offer_backup() -> Result<()> {
    print!("Would you like to back up your current mods/ and config/ before updating? [Y/n]: ");
    io::stdout().flush()?;

    let input = read_line();
    if input.to_lowercase() == "n" {
        println!("Skipping backup.");
        println!();
        return Ok(());
    }

    // Use a Unix timestamp as a unique, unambiguous suffix for the backup folder names.
    // This avoids requiring date-formatting logic or additional crates.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for src_name in ["mods", "config"] {
        let src = Path::new(src_name);
        if src.is_dir() {
            let dst_name = format!("{}_backup_{}", src_name, ts);
            match copy_dir(src, Path::new(&dst_name)) {
                Ok(()) => println!("  Backed up {}/  →  {}/", src_name, dst_name),
                Err(e) => println!("  [!] Warning: Could not back up {}/: {}", src_name, e),
            }
        } else {
            println!("  [i] No {0}/ directory found, skipping {0}/ backup.", src_name);
        }
    }

    println!();
    Ok(())
}

/// Recursively copies a directory tree from `src` to `dst`.
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;

    for entry in fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)
                .with_context(|| format!("Failed to copy file: {}", entry.path().display()))?;
        }
    }

    Ok(())
}

// ── Step 5: Wipe mods/ ────────────────────────────────────────────────────────

fn wipe_mods_dir() -> Result<()> {
    println!("Clearing mods directory...");

    if Path::new("mods").is_dir() {
        fs::remove_dir_all("mods").context("Failed to clear the mods/ directory.")?;
    }
    fs::create_dir("mods").context("Failed to create the mods/ directory.")?;

    Ok(())
}

// ── Step 6: Sequential download loop ──────────────────────────────────────────

fn download_mods(mods: &[ModEntry]) -> Result<(Vec<String>, Vec<(String, String)>)> {
    let total = mods.len();
    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    println!();
    for (i, entry) in mods.iter().enumerate() {
        let filename = derive_filename(&entry.url);
        print!("[{}/{}] Downloading: {}...", i + 1, total, entry.display_name);
        io::stdout().flush()?;

        match download_file(&entry.url, &filename) {
            Ok(()) => {
                println!("  ✓");
                succeeded.push(filename);
            }
            Err(e) => {
                println!("  ✗");
                failed.push((entry.display_name.clone(), e.to_string()));
            }
        }
    }

    println!();
    Ok((succeeded, failed))
}

/// Derives a safe filename to save to disk from a CDN URL.
///
/// Takes the path segment after the last `/` and strips any query string.
/// Example: `.../linkage-0.2.5.jar?mr_download_reason=standalone` → `linkage-0.2.5.jar`
fn derive_filename(url: &str) -> String {
    let path_part = url.split('/').next_back().unwrap_or("unknown.jar");
    path_part.split('?').next().unwrap_or(path_part).to_string()
}

/// Downloads a single file from `url` and streams it to `mods/<filename>`.
fn download_file(url: &str, filename: &str) -> Result<()> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("HTTP request failed for '{}'", filename))?;

    let dest = format!("mods/{}", filename);
    let mut file = fs::File::create(&dest)
        .with_context(|| format!("Could not create file: {}", dest))?;

    io::copy(&mut response.into_reader(), &mut file)
        .with_context(|| format!("Failed while writing '{}'", filename))?;

    Ok(())
}

// ── Step 7: Report & log ──────────────────────────────────────────────────────

fn report_and_log(report: &RunReport) -> Result<()> {
    println!("{}", "─".repeat(40));

    if report.failed.is_empty() {
        println!("✓ Done! {} mods downloaded successfully.", report.succeeded.len());
    } else {
        println!(
            "⚠ Completed with {} failure(s) ({} succeeded):",
            report.failed.len(),
            report.succeeded.len()
        );
        for (name, err) in &report.failed {
            println!("  - {}: {}", name, err);
        }
        println!();
        println!("Re-run the updater to retry, or check the URLs in the modlist.");
    }

    // Log failures are non-fatal — the download run itself has completed.
    if let Err(e) = append_log(report) {
        println!("[!] Warning: Could not write to log file: {}", e);
    }

    Ok(())
}

/// Appends a timestamped run summary to `logs/createcoreupdater.log`.
/// The log is never cleared between runs; it grows as a persistent history.
fn append_log(report: &RunReport) -> Result<()> {
    fs::create_dir_all("logs").context("Failed to create the logs/ directory.")?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)
        .context("Failed to open the log file.")?;

    writeln!(
        log,
        "=== {} | Mode: {} | Version: {} ===",
        format_unix_utc(ts),
        report.mode,
        report.version
    )?;

    for filename in &report.succeeded {
        writeln!(log, "[OK]   {}", filename)?;
    }
    for (name, err) in &report.failed {
        writeln!(log, "[FAIL] {}: {}", name, err)?;
    }

    writeln!(
        log,
        "=== Run complete: {} succeeded, {} failed ===\n",
        report.succeeded.len(),
        report.failed.len()
    )?;

    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Reads a line from stdin, returning a trimmed string.
fn read_line() -> String {
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).unwrap_or_default();
    input.trim().to_string()
}

/// Formats a Unix timestamp (seconds since epoch) as `YYYY-MM-DD HH:MM:SS` (UTC).
/// Implemented without external crates using standard calendar arithmetic.
fn format_unix_utc(secs: u64) -> String {
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Compute year and remaining day-of-year from days since the Unix epoch (1970-01-01).
    let mut remaining_days = (secs / 86400) as u32;
    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    // Compute month and day-of-month.
    let month_lengths = [
        31u32,
        if is_leap_year(year) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1u32;
    for &days_in_month in &month_lengths {
        if remaining_days < days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, h, m, s)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}