param([string]$InstallerPath)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $InstallerPath) {
    $InstallerPath = Join-Path $projectRoot 'target/x86_64-pc-windows-msvc/release/bundle/nsis/LLM Nest_0.1.0_x64-setup.exe'
}
$installer = Get-Item -LiteralPath (Resolve-Path -LiteralPath $InstallerPath).Path
if ($installer.PSIsContainer -or $installer.Extension -notin '.exe', '.msi') {
    throw 'Expected a built .exe or .msi installer.'
}

# Share only a fresh package copy, never the workspace or user configuration.
$output = Join-Path $projectRoot ('target/sandbox-acceptance-' + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $output 'package'
New-Item -ItemType Directory -Path $payload | Out-Null
Copy-Item -LiteralPath $installer.FullName -Destination (Join-Path $payload $installer.Name)
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/windows-clean-install.md') -Destination (Join-Path $payload 'CHECKLIST.md')
$hash = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash
Set-Content -LiteralPath (Join-Path $payload 'SHA256.txt') -Value ($hash + '  ' + $installer.Name) -Encoding UTF8
$escapedPath = [System.Security.SecurityElement]::Escape($payload)
$config = @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$escapedPath</HostFolder>
      <SandboxFolder>C:\LLM-Nest-Package</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>explorer.exe C:\LLM-Nest-Package</Command>
  </LogonCommand>
</Configuration>
"@
$wsb = Join-Path $output 'LLM-Nest.wsb'
Set-Content -LiteralPath $wsb -Value $config -Encoding UTF8
Write-Output $wsb
if (-not (Get-Command WindowsSandbox.exe -ErrorAction SilentlyContinue)) {
    Write-Warning 'Windows Sandbox is unavailable. The package is prepared; no guest has been launched or tested.'
}
