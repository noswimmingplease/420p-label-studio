---
lang: en-GB
---

# LabelManager 420P

A Windows desktop label editor for the DYMO LabelManager 420P. The interface is written in Rust and egui; a local .NET bridge uses the DYMO SDK for previews and printing. This is an independent project.

## Features

- Dark editor with text formatting, image thresholding, metric/imperial sizes and millimetre rulers.
- Smooth design and printer-resolution SDK previews.
- Object selection, dragging, guides, alignment and manual positions retained across object changes.
- Shapes, lines, addresses, barcodes, QR codes, captured dates and counters.
- Local `.420p` project files, checked `.dymo` export, contacts and CSV/TSV batches.
- Connection indicator, notification-area restoration and separate app/DYMO software update checks.

Print jobs use the same frozen template, image and captured object values as the verified preview. Batches validate all labels before submission and stop on failure. An accepted job is not confirmation of a physical print.

## Build and test

Use Windows with Rust 1.95 or later, the .NET 10 SDK, and Visual Studio's Desktop development with C++ workload and Windows SDK. DYMO Connect and the 420P driver are required for hardware printing.

In an x64 Visual Studio developer PowerShell, run:

```powershell
pwsh -File scripts/build.ps1
```

The script restores the pinned dependencies, builds the bridge, supplies synthetic fixtures to the SDK integration tests, runs the release tests and builds `dist/DYMO420P`. It does not print, install or upload anything. `-SkipTests` is available for local iteration; use the default test run before distribution. Restore requires access to the Rust and NuGet package registries.

The `sdk` directory must remain beside `dymo-420p.exe`. Local preferences, contacts, projects, diagnostics, package caches and build outputs are not repository content.

## Updates

Settings has separate app and DYMO Connect checks. Configure the public GitHub repository in App updates to check its latest stable release, read the notes and open its download page. Private releases require opening GitHub manually. Checks do not execute downloads or replace a running app. The version shown by the SDK is its assembly version, which may differ from the NuGet package version.

## Limitations

The 420P supports 6, 9, 12 and 19 mm D1 tape and has a manual cutter. Enlarging a printer-resolution preview exposes its dots. The smooth view is a design aid; barcodes can fall back to an approximate view and require checking the regular SDK preview. Rulers describe the SDK canvas, not the physical cut length.

Geometry editing, smooth rendering and resolved export depend on guarded private SDK members. Export is accepted only after a saved/reloaded label reproduces the resolved XML and printer-resolution bitmap. Unsupported SDK changes fail safely. Gross template-width mismatches are rejected when geometry is available; the app does not automatically measure the fitted cassette.

Imported live date/time objects and embedded counters must be recreated through the app to keep previews and printing consistent. Static template dates remain supported. Removing a replacement image from an imported template restores any image embedded in the original template.

The basic 9 mm text/image workflow has physical validation. Wider tapes, advanced objects and batches still need broader physical verification. Automated tests and code review do not establish a bug-free application or full DYMO Connect parity. Mirror, collate, chain marks and graphics-quality controls remain pending device-specific preview/print validation. No firmware or hidden-setting update source is configured.

## Dependencies

See `THIRD-PARTY-NOTICES.txt`. Vendor applications, SDK binaries and reference executables are not included in this source repository.
