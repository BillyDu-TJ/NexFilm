---
id: download-and-install
track: getting-started
trackOrder: 1
chapterOrder: 1
kicker: "01.1 / Getting Started"
title: "Download, Install, and Launch NexFilm"
subtitle: "Choose the right installer, clear the first-launch prompt, and confirm that NexFilm is ready to use."
readingTime: 6
inApp: true
sinceVersion: "v1.1.0"
aliases: ["quick-start"]
---

## Choose the Right Installer {#choose-installer}

The part that trips people up most often is not installation itself, but downloading the wrong file. The GitHub Releases page lists installers, source archives, and checksum files together, and they are easy to confuse on a first visit. Look for a filename ending in `setup.exe` or `.dmg`. You do not need `Source code (zip)` to run NexFilm.

On Windows, download the installer containing `x64-setup.exe`. On macOS, first check whether your Mac uses Apple Silicon or Intel: open the Apple menu, choose **About This Mac**, and look at the chip or processor line. If it says M1, M2, M3, M4, or a newer Apple chip, download the file containing `aarch64.dmg`. If it says Intel, download the file containing `x64.dmg`. A mismatched build will not damage your computer, but it will not launch properly.

<table class="doc-table">
    <thead>
        <tr><th>Computer</th><th>File to Download</th><th>How to Check</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows 10 / 11</strong></td>
            <td><code>NexFilm.Engine_{version}_x64-setup.exe</code></td>
            <td>The filename ends in <code>setup.exe</code>.</td>
        </tr>
        <tr>
            <td><strong>Apple Silicon Mac</strong></td>
            <td><code>NexFilm.Engine_{version}_aarch64.dmg</code></td>
            <td>The chip line in About This Mac starts with Apple M.</td>
        </tr>
        <tr>
            <td><strong>Intel Mac</strong></td>
            <td><code>NexFilm.Engine_{version}_x64.dmg</code></td>
            <td>The processor line in About This Mac says Intel.</td>
        </tr>
    </tbody>
</table>

<div class="doc-callout">
    <div class="doc-callout-title">Download from the Official Releases Page</div>
    <div class="doc-callout-body">Get the installer from the project repository's Releases page. Avoid reposts, third-party download sites, and copies with altered filenames. The packages are not commercially code-signed, so Windows and macOS may both show a security prompt on first launch. That is normal for unsigned open-source software.</div>
</div>

## Install NexFilm Engine {#install}

Installation on Windows is straightforward. Double-click the `.exe` file. If Microsoft Defender SmartScreen says Windows protected your PC, click **More info**, then **Run anyway**, and finish the setup wizard. NexFilm will be installed in the usual program directory and should appear in the Start menu.

On macOS, double-click the `.dmg` file to open the mounted disk image. Drag `NexFilm Engine.app` into the **Applications** folder, then find it in Finder under Applications. You can eject the disk image once the copy is finished. The downloaded DMG can be kept for later or removed after installation.

There is no separate account setup or license key. NexFilm runs from the app bundle and keeps its own project data elsewhere; the DMG itself is only the installer.

On the first launch, macOS may say that the app is damaged or that Apple cannot check it for malicious software. That does not usually mean the file is actually damaged; it is the quarantine flag macOS attaches to downloaded files. Make sure `NexFilm Engine.app` is in the Applications folder before continuing to the next section.

## First Launch and the macOS Prompt {#first-launch}

If you already clicked **Run anyway** while opening the Windows installer, NexFilm should launch normally after installation. If Windows blocks it again, go through **More info** and continue from there. There is no need to turn off Windows Defender or weaken any system-wide security setting just to run NexFilm.

On macOS, open **Launchpad** → **Other** → **Terminal**, or use **Finder** → **Applications** → **Utilities** → **Terminal**. Copy the complete command below, paste it into Terminal, and press <kbd>Return</kbd>:

```bash
xattr -cr "/Applications/NexFilm Engine.app"
```

If macOS asks for a password, type your Mac login password and press <kbd>Return</kbd> again. Terminal does not show characters while you type, so do not be surprised if the cursor appears not to move. When the command finishes, return to **Applications** and open `NexFilm Engine` again.

<div class="doc-callout">
    <div class="doc-callout-title">This Command Only Affects NexFilm</div>
    <div class="doc-callout-body">`xattr -cr` clears extended attributes, including the download quarantine flag, from the path that follows it. It does not disable Gatekeeper or System Integrity Protection (SIP). If you installed NexFilm somewhere other than <code>/Applications</code>, replace the path in the command with the actual location.</div>
</div>

## What to Check After the First Launch {#first-open}

NexFilm opens in the **Library** view. An empty Library is normal on a first launch; it simply means no scans have been imported yet. **Import Roll** in the top-right header is where most work begins. If you only have a few test frames, Loose Import is a good way to get a quick look before committing to a full roll.

There is no need to import dozens of files right away. A safer first run is to take two or three normally exposed frames through the complete path: import, open Develop, set the Film Area, run Auto Invert, and export. Once the thumbnails, preview, and exported file all open correctly, move on to the rest of the roll. NexFilm does not copy, move, or rewrite your original RAW or TIFF files. The project database and thumbnail cache live in the standard application support folder for your operating system.

If the Library shows thumbnails but a frame does not immediately appear as a positive when you open Develop, that is also normal. NexFilm builds a working proxy the first time a frame is opened and waits for you to run **Auto Invert**. This is on-demand processing, not a failed import.

## Common Installation and Launch Problems {#common-issues}

**The downloaded file will not open.** Check that you downloaded an installer rather than a `Source code` archive. Windows installers end in `.exe`; macOS installers end in `.dmg`. If the filename does not match your platform, return to the Releases page and choose the correct build.

**macOS cannot find the app.** Open the **Applications** folder in Finder and confirm that the app is named `NexFilm Engine.app`. If you placed it on the Desktop, in Downloads, or somewhere else, change the path in the Terminal command. If macOS has renamed it to something like `NexFilm Engine 2.app`, use that exact name instead.

**The app still will not open after the command.** Close the app, run the command once more, and try again. If macOS still blocks it, confirm the app is actually in Applications, then see [Troubleshooting and Appendix](/en/docs/troubleshooting-and-appendix/) for startup and file-path issues. Do not disable SIP just to launch one application.

**I have installed it. Where do I start?** The next step is [Understanding NexFilm: Principles, Installation, and Workspace](/en/docs/overview-and-install/), or you can follow [Walk Through Your First Roll](/en/docs/first-roll-in-five-minutes/) from beginning to end. If you have not started digitizing yet, read [Camera Scanning and Digitization Best Practices](/en/docs/scanning-best-practices/) first.
