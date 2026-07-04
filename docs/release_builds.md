# Release Builds and Cross-Compilation

This project uses **GitHub Actions** to automate the creation of release binaries for our primary targets: **Windows 10/11** and **Ubuntu Linux**.

## How the CI/CD Pipeline Works

When ready to publish a new version of the modpack utility, create a Release on GitHub (or push a version tag like `v1.0.0`). This will automatically trigger a GitHub Actions workflow in the cloud that performs the following steps:

1. **Provisioning:** GitHub spins up two separate virtual environments concurrently: a Windows machine and an Ubuntu machine.
2. **Setup:** The workflow checks out your repository's code and installs the latest stable Rust toolchain on both environments.
3. **Compilation:** It runs `cargo build --release` natively on both the Windows and Ubuntu runners. 
4. **Distribution:** Once compiled, the workflow automatically attaches the resulting binaries (the Windows `.exe` and the Linux executable) to your GitHub Release page.

## Why Use This Approach

Because a Linux binary needs to link against Linux-specific system libraries (like `glibc`), natively building it on a Windows machine requires setting up additional C-toolchains or using Docker (`cross`). 

By offloading this to GitHub Actions:
* **Zero Local Overhead:** No need to configure complex cross-compilation tools or WSL on your local Windows machine just to build a release.
* **Reproducibility:** Binaries are always built in a clean, standardized environment, preventing "it works on my machine" issues.
* **Automation:** The build and distribution process is entirely hands-off.

## Local Testing
If you ever need to test the Ubuntu build locally before a release, the recommended approach is to run `cargo build` inside of the **Windows Subsystem for Linux (WSL)**.
