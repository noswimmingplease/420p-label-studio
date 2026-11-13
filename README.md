---
lang: en-GB
---

# 420P Label Studio

A Windows desktop label editor dedicated to the DYMO LabelManager 420P. The interface is written in Rust and egui; a local .NET bridge uses the DYMO SDK for previews and printing. This is an independent project.

## Features

- Dark editor with text formatting, image thresholding, metric/imperial sizes and millimetre rulers.
- Smooth design and printer-resolution SDK previews.
- Object selection, dragging, guides, alignment and manual positions retained across object changes.
- Shapes, lines, addresses, barcodes, QR codes, captured dates and counters.
- Local `.420p` project files, checked `.dymo` export, contacts and CSV/TSV batches.
- Connection and battery status, notification-area restoration and GitHub app update checks.

Print jobs use the same frozen template, image and captured object values as the verified preview. Batches validate all labels before submission and stop on failure. An accepted job is not confirmation of a physical print.

## Run the Windows release

Extract the complete release ZIP to a folder and run `label-studio-420p.exe`. Keep the `sdk` directory beside it. The app requires 64-bit Windows 10 or later, the x64 .NET 10 Desktop Runtime, and DYMO Connect with the 420P driver installed. Read `COPYRIGHT.txt` and the bundled third-party terms before using or redistributing the release; agreement to the included Microsoft component terms is required. The release is unsigned, so Windows may show an unknown-publisher prompt. Existing project files and local preferences are stored separately from the application folder.

## Build and test

Use Windows with Rust 1.95 or later, the .NET 10 SDK, and Visual Studio's Desktop development with C++ workload and Windows SDK. DYMO Connect and the 420P driver are required for hardware printing.

In an x64 Visual Studio developer PowerShell, run:

```powershell
pwsh -File scripts/build.ps1
```

The script restores the pinned dependencies, builds the bridge, supplies synthetic fixtures to the SDK integration tests, runs the release tests and builds `dist/DYMO420P`. It does not print, install or upload anything. `-SkipTests` is available for local iteration; use the default test run before distribution. Restore requires access to the Rust and NuGet package registries.

`-PublicRelease` requires the full tests, remaps local compiler paths, disables debug symbols and stages the x64 runtime files in `dist/DYMO420P-public`. Its `public-bridge` test output and public distribution staging folders must be empty before the build. Binary publication also requires the applicable transitive dependency and embedded-font licence texts/notices, and an audit of the exact archive and metadata. The build script does not publish a release.

The `sdk` directory must remain beside `label-studio-420p.exe`. Local preferences, contacts, projects, diagnostics, package caches and build outputs are not repository content.

## Updates

Settings and Help check this project's public GitHub repository for the latest stable app release. Fresh installations default to `noswimmingplease/dymo-420p-rust`; an explicitly saved repository or disabled setting is preserved. Checks display release notes and a link to the download page. Save the label and close the app before replacing it with a downloaded release. Checks do not execute downloads or replace a running app. Firmware and DYMO software update controls have been removed.

## Limitations

The 420P supports 6, 9, 12 and 19 mm D1 tape and has a manual cutter. Enlarging a printer-resolution preview exposes its dots. The smooth view is a design aid; barcodes can fall back to an approximate view and require checking the regular SDK preview. Rulers describe the SDK canvas, not the physical cut length.

Geometry editing, smooth rendering and resolved export depend on guarded private SDK members. Export is accepted only after a saved/reloaded label reproduces the resolved XML and printer-resolution bitmap. Unsupported SDK changes fail safely. Gross template-width mismatches are rejected when geometry is available.

The app reads the 420P's battery percentage, charging state, detected tape width and reported printer errors through DYMO's legacy status interface. Refresh reads these details again; background checks run every 15 seconds and pause during print jobs. Status reads and print submission are serialised. Missing or invalid details appear as unavailable. Detected tape width is advisory and does not change the label's selected width. A live connected/idle battery reply has been checked; active charging and error combinations have synthetic test coverage and still need broader physical validation.

Imported live date/time objects and embedded counters must be recreated through the app to keep previews and printing consistent. Static template dates remain supported. Removing a replacement image from an imported template restores any image embedded in the original template.

The basic 9 mm text/image workflow has physical validation. Wider tapes, advanced objects and batches still need broader physical verification. Automated tests and code review do not establish a bug-free application or full DYMO Connect parity. Mirror, collate, chain marks and graphics-quality controls remain pending device-specific preview/print validation. Firmware and hidden-setting modification are not supported.

## Dependencies

See `THIRD-PARTY-NOTICES.txt`. Vendor applications, SDK binaries and reference executables are not included in this source repository.
