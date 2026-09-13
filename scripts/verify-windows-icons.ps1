param([string[]]$ArtifactPath)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $ArtifactPath) {
    $ArtifactPath = @(
        (Join-Path $projectRoot 'target/x86_64-pc-windows-msvc/release/LLM-Nest.exe'),
        (Join-Path $projectRoot 'target/x86_64-pc-windows-msvc/release/bundle/nsis/LLM-Nest_0.1.0_x64-setup.exe')
    )
}

# Inspect actual executable resources, using fresh paths to avoid shell icon caches.
# This checks the shell's 32px representation; it is not a taskbar screenshot test.
$scratch = Join-Path $projectRoot ('target/icon-verification-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$copies = @()
function Get-IconDigest([string]$Path) {
    $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($Path)
    if (-not $icon) { throw "No Windows icon found: $Path" }
    $bitmap = $icon.ToBitmap()
    $stream = New-Object System.IO.MemoryStream
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
        return [BitConverter]::ToString($sha.ComputeHash($stream.ToArray()))
    } finally {
        $sha.Dispose()
        $stream.Dispose()
        $bitmap.Dispose()
        $icon.Dispose()
    }
}

try {
    $reference = Join-Path $scratch 'reference.ico'
    Copy-Item -LiteralPath (Join-Path $projectRoot 'frontends/tauri/src-tauri/icons/icon.ico') -Destination $reference
    $copies += $reference
    $expected = Get-IconDigest $reference
    foreach ($path in $ArtifactPath) {
        $artifact = Get-Item -LiteralPath (Resolve-Path -LiteralPath $path).Path
        if ($artifact.PSIsContainer -or $artifact.Extension -ne '.exe') { throw "Expected an executable: $path" }
        $copy = Join-Path $scratch ([guid]::NewGuid().ToString('N') + '.exe')
        Copy-Item -LiteralPath $artifact.FullName -Destination $copy
        $copies += $copy
        if ((Get-IconDigest $copy) -ne $expected) {
            throw "Windows icon does not match icons/icon.ico: $($artifact.FullName)"
        }
        Write-Output "PASS Windows icon: $($artifact.FullName)"
    }
} finally {
    # Remove only the explicit copies created above; never recursively delete.
    foreach ($copy in $copies) { Remove-Item -LiteralPath $copy }
    Remove-Item -LiteralPath $scratch
}
