param([string]$BuildDirectory)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$config = Get-Content -LiteralPath (Join-Path $projectRoot 'desktop/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if (-not $BuildDirectory) {
    $BuildDirectory = Join-Path $projectRoot 'target/x86_64-pc-windows-msvc/release'
}
function Assert-Equal($Actual, $Expected, [string]$Label) {
    if ($Actual -ne $Expected) { throw "${Label}: expected '$Expected', got '$Actual'" }
}

$exe = Get-Item -LiteralPath (Join-Path $BuildDirectory 'LLM-Nest.exe')
Assert-Equal $exe.VersionInfo.ProductName $config.productName 'EXE product name'
Assert-Equal $exe.VersionInfo.FileDescription $config.productName 'EXE file description'

$msi = Join-Path $BuildDirectory "bundle/msi/$($config.productName)_$($config.version)_x64_en-US.msi"
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase((Resolve-Path -LiteralPath $msi).Path, 0)
function Read-MsiColumn([string]$Query) {
    $view = $database.OpenView($Query)
    try {
        [void]$view.Execute()
        while ($null -ne ($record = $view.Fetch())) { $record.StringData(1) }
    } finally { [void]$view.Close() }
}
Assert-Equal (Read-MsiColumn "SELECT Value FROM Property WHERE Property = 'ProductName'") $config.productName 'MSI product name'
Assert-Equal (Read-MsiColumn "SELECT Value FROM Property WHERE Property = 'Manufacturer'") $config.bundle.publisher 'MSI publisher'
Assert-Equal (Read-MsiColumn "SELECT Value FROM Property WHERE Property = 'ProductVersion'") $config.bundle.windows.wix.version 'MSI version'
$upgrade = (Read-MsiColumn "SELECT Value FROM Property WHERE Property = 'UpgradeCode'").Trim('{}')
Assert-Equal $upgrade '0dc13402-ce85-5dcb-ba1a-1f629fecd8b8' 'MSI legacy upgrade code'
$folder = Read-MsiColumn "SELECT DefaultDir FROM Directory WHERE Directory = 'INSTALLDIR'"
Assert-Equal (($folder -split '\|')[-1]) 'LLM-Nest' 'MSI install directory'
$registryKeys = @(Read-MsiColumn 'SELECT `Key` FROM Registry')
if ('Software\LLM-Nest\LLM-Nest' -notin $registryKeys) { throw 'MSI lost its legacy registry key' }
$shortcutNames = @(Read-MsiColumn 'SELECT Name FROM Shortcut' | ForEach-Object { ($_ -split '\|')[-1] })
if ($config.productName -notin $shortcutNames -or 'LLM-Nest' -in $shortcutNames) {
    throw 'MSI shortcut names do not use the new display name'
}

$nsis = Get-Content -LiteralPath (Join-Path $BuildDirectory 'nsis/x64/installer.nsi') -Raw
foreach ($definition in @(
    "!define PRODUCTNAME `"$($config.productName)`"",
    '!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\LLM-Nest"',
    '!define MANUPRODUCTKEY "${MANUKEY}\LLM-Nest"',
    'StrCpy $INSTDIR "$LOCALAPPDATA\LLM-Nest"'
)) {
    if (-not $nsis.Contains($definition)) { throw "NSIS missing expected identity setting: $definition" }
}
Write-Output "PASS: $($config.productName) $($config.version) EXE/MSI display names, shortcuts and preserved Windows install identity"
