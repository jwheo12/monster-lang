$ErrorActionPreference = "Stop"

$Repo = "BitIntx/monster-lang"
$Version = if ($env:MST_VERSION) { $env:MST_VERSION } else { $null }
$InstallDir = if ($env:MST_INSTALL_DIR) { $env:MST_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\mst\bin" }
$InstallRoot = Split-Path -Parent $InstallDir
$StdDir = if ($env:MST_STD_DIR) { $env:MST_STD_DIR } else { Join-Path $InstallRoot "share\mst\std" }

function Get-LatestVersion {
    $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases"

    foreach ($release in $releases) {
        if (-not $release.draft) {
            return $release.tag_name
        }
    }

    throw "No published releases found."
}

function Get-AssetName {
    param([string]$ResolvedVersion)

    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($arch) {
        "X64" { return "mst-$ResolvedVersion-windows-x86_64.zip" }
        default { throw "Unsupported Windows architecture: $arch" }
    }
}

function Test-BackendTools {
    $clang = Get-Command clang.exe -ErrorAction SilentlyContinue
    $opt = Get-Command opt.exe -ErrorAction SilentlyContinue

    return ($null -ne $clang -and $null -ne $opt)
}

function Show-BackendToolHelp {
    Write-Warning "clang.exe and opt.exe are required for 'mst build' and 'mst run'."
    Write-Host "[mst] Install LLVM and ensure LLVM\bin is on PATH."
}

if (-not $Version) {
    $Version = Get-LatestVersion
}

$AssetName = Get-AssetName -ResolvedVersion $Version
$ChecksumName = "$AssetName.sha256"
$DownloadBase = "https://github.com/$Repo/releases/download/$Version"
$PackageDir = [System.IO.Path]::GetFileNameWithoutExtension($AssetName)
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())

New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $AssetPath = Join-Path $TempDir $AssetName
    $ChecksumPath = Join-Path $TempDir $ChecksumName

    Write-Host "[mst] downloading $AssetName..."
    Invoke-WebRequest -Uri "$DownloadBase/$AssetName" -OutFile $AssetPath
    Invoke-WebRequest -Uri "$DownloadBase/$ChecksumName" -OutFile $ChecksumPath

    Write-Host "[mst] verifying checksum..."
    $ExpectedHash = ((Get-Content $ChecksumPath).Trim() -split '\s+')[0].ToLowerInvariant()
    $ActualHash = (Get-FileHash -Path $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($ExpectedHash -ne $ActualHash) {
        throw "Checksum verification failed for $AssetName"
    }

    Write-Host "[mst] extracting release..."
    Expand-Archive -Path $AssetPath -DestinationPath $TempDir -Force

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item -Path (Join-Path $TempDir $PackageDir "mst.exe") -Destination (Join-Path $InstallDir "mst.exe") -Force

    Write-Host "[mst] installed to $(Join-Path $InstallDir 'mst.exe')"

    $SourceStd = Join-Path (Join-Path $TempDir $PackageDir) "std"
    if (Test-Path $SourceStd) {
        if (Test-Path $StdDir) {
            Remove-Item -Path $StdDir -Recurse -Force
        }

        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StdDir) | Out-Null
        Copy-Item -Path $SourceStd -Destination $StdDir -Recurse -Force
        Write-Host "[mst] installed std to $StdDir"
    } else {
        Write-Warning "Release package does not contain std/."
    }

    if (-not (Test-BackendTools)) {
        Show-BackendToolHelp
    }

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $Separator = [System.IO.Path]::PathSeparator
    $Segments = @()

    if ($UserPath) {
        $Segments = $UserPath.Split($Separator, [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if ($Segments -notcontains $InstallDir) {
        $NewPath = if ([string]::IsNullOrEmpty($UserPath)) { $InstallDir } else { "$InstallDir$Separator$UserPath" }
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        Write-Host "[mst] added $InstallDir to your user PATH"
        Write-Host "[mst] restart your terminal to use 'mst' directly"
    }

    Write-Host "[mst] try: mst --help"
}
finally {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
