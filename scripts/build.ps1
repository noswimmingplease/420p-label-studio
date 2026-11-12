#Requires -Version 7.0
# Language: en-GB
[CmdletBinding()]
param([switch]$SkipTests, [switch]$PublicRelease)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$bridgeOutput = Join-Path $projectRoot 'sdk-bridge/bin/Release/net10.0-windows10.0.19041.0'
$fixtureDirectory = Join-Path $projectRoot '.local-test-fixtures'
$distribution = Join-Path $projectRoot 'dist/DYMO420P'

if (-not $IsWindows) { throw 'This application requires Windows.' }
foreach ($tool in @('dotnet', 'cargo', 'link.exe', 'rc.exe')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "Cannot find $tool. Run in an x64 Visual Studio developer shell with Rust, the Windows SDK and the .NET 10 SDK installed."
    }
}

Push-Location $projectRoot
$previousEnvironment = @{}
try {
    $bridgeArguments = @('build', 'sdk-bridge/DymoBridge.csproj', '--configuration', 'Release', '--nologo')
    if ($PublicRelease) {
        if ($SkipTests) { throw 'Public release builds require the complete test run.' }
        $bridgeOutput = Join-Path $fixtureDirectory 'public-bridge'
        $distribution = Join-Path $projectRoot 'dist/DYMO420P-public'
        foreach ($directory in @($bridgeOutput, $distribution)) {
            if ((Test-Path -LiteralPath $directory) -and @(Get-ChildItem -LiteralPath $directory -Force).Count -gt 0) {
                throw 'Public release staging must be empty. Inspect and remove the previous generated public-bridge and DYMO420P-public folders before rebuilding.'
            }
        }
        $bridgeArguments += @('--runtime', 'win-x64', '--output', $bridgeOutput, '-p:DebugType=None', '-p:DebugSymbols=false', '-p:ContinuousIntegrationBuild=true', "-p:PathMap=$projectRoot=/_/source/dymo-420p")
        foreach ($name in @('CARGO_ENCODED_RUSTFLAGS', 'RUSTFLAGS')) {
            $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $flags = @('-C', 'debuginfo=0', '-C', 'strip=symbols', '-C', 'link-arg=/PDBALTPATH:dymo-420p.pdb')
        $paths = @{
            $projectRoot = '/_/source/dymo-420p'
        }
        foreach ($name in @('CARGO_HOME', 'RUSTUP_HOME', 'CARGO_TARGET_DIR', 'USERPROFILE')) {
            $value = [Environment]::GetEnvironmentVariable($name, 'Process')
            if ($value) { $paths[[IO.Path]::GetFullPath($value)] = '/_/' + $name.ToLowerInvariant() }
        }
        foreach ($path in $paths.Keys) {
            foreach ($form in @($path, $path.Replace('\', '/')) | Select-Object -Unique) { $flags += "--remap-path-prefix=$form=$($paths[$path])" }
        }
        $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
        $env:RUSTFLAGS = $null
    }
    & dotnet @bridgeArguments
    if ($LASTEXITCODE -ne 0) { throw 'SDK bridge build failed.' }

    if (-not $SkipTests) {
        New-Item -ItemType Directory -Path $fixtureDirectory -Force | Out-Null
        Add-Type -AssemblyName System.Drawing
        $imagePath = Join-Path $fixtureDirectory 'synthetic-image.png'
        $bitmap = [System.Drawing.Bitmap]::new(16, 16)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([System.Drawing.Color]::White)
            $graphics.FillRectangle([System.Drawing.Brushes]::Black, 3, 3, 10, 10)
            $graphics.FillRectangle([System.Drawing.Brushes]::White, 6, 6, 4, 4)
            $bitmap.Save($imagePath, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
        $fixtureEnvironment = @{
            DYMO_TEST_BRIDGE = Join-Path $bridgeOutput 'DymoBridge.exe'
            DYMO_TEST_TEMPLATE_DIR = Join-Path $projectRoot 'sdk-bridge/templates'
            DYMO_TEST_TEMPLATE = Join-Path $projectRoot 'sdk-bridge/templates/6mm.dymo'
            DYMO_TEST_IMAGE = $imagePath
        }
        foreach ($name in $fixtureEnvironment.Keys) {
            $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
            [Environment]::SetEnvironmentVariable($name, $fixtureEnvironment[$name], 'Process')
        }
        & cargo test --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release tests failed; no distribution was prepared.' }
    }

    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust application build failed.' }
    $metadataText = & cargo metadata --format-version 1 --no-deps --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the Cargo target directory.' }
    $metadata = ($metadataText -join "`n") | ConvertFrom-Json
    New-Item -ItemType Directory -Path (Join-Path $distribution 'sdk') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $metadata.target_directory 'release/dymo-420p.exe') -Destination $distribution
    if ($PublicRelease) {
        # Copy only runtime artefacts from this clean, separate bridge output.
        # PDBs, build logs, profiles and application preferences are excluded.
        $extensions = @('.exe', '.dll', '.json', '.xml', '.dymo')
        foreach ($file in Get-ChildItem -LiteralPath $bridgeOutput -File -Recurse) {
            if ($file.Extension -notin $extensions) { continue }
            $relative = [IO.Path]::GetRelativePath($bridgeOutput, $file.FullName)
            $destination = Join-Path (Join-Path $distribution 'sdk') $relative
            New-Item -ItemType Directory -Path (Split-Path $destination) -Force | Out-Null
            Copy-Item -LiteralPath $file.FullName -Destination $destination -Force
        }
    } else {
        Copy-Item -Path (Join-Path $bridgeOutput '*') -Destination (Join-Path $distribution 'sdk') -Recurse -Force
    }
    Copy-Item -LiteralPath 'README.md', 'THIRD-PARTY-NOTICES.txt', 'COPYRIGHT.txt' -Destination $distribution
    Write-Host "Build ready at $distribution"
    Write-Host 'This script does not install the app, submit print jobs or upload files.'
} finally {
    foreach ($name in $previousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
    Pop-Location
}
