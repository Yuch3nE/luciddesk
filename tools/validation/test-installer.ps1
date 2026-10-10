[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourcePath,
    [switch]$AllUsers
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$id = [guid]::NewGuid().ToString()
$name = "LucidDesk Test $id"
$upgrade = [guid]::NewGuid().ToString()
$root = Join-Path $repo "target/installer-test/$id"
$installed = Join-Path $root 'app'
$hive = 'HKCU:'
$programs = [Environment]::GetFolderPath('Programs')
if ($AllUsers) {
    $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run -AllUsers from an elevated PowerShell.' }
    $installed = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $name
    $hive = 'HKLM:'
    $programs = [Environment]::GetFolderPath('CommonPrograms')
}
$registry = "$hive\Software\Yuchen95\$name"
$shortcut = Join-Path $programs "$name.lnk"
New-Item -ItemType Directory -Path $root | Out-Null
$probe = $null
$lock = $null
$current = $null
function Run-Msi([string]$Mode, [string]$Package, [string[]]$Properties = @()) {
    $log = Join-Path $root "$Mode-$([guid]::NewGuid()).log"
    $scope = @()
    if ($AllUsers) { $scope = @('ALLUSERS=1', 'MSIINSTALLPERUSER=""') }
    $arguments = @("/$Mode", "`"$Package`"", '/qn', '/norestart', '/L*v', "`"$log`"") + $scope + $Properties
    $exit = (Start-Process msiexec.exe -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait).ExitCode
    Write-Host "MSI /$Mode returned $exit ($log)"
    return $exit
}
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
try {
    $old = & "$PSScriptRoot/../packaging/build-msi.ps1" -SourcePath $SourcePath -OutputPath "$root/old" -Version '0.0.1' -ProductName $name -UpgradeCode $upgrade -TestFixture
    $current = & "$PSScriptRoot/../packaging/build-msi.ps1" -SourcePath $SourcePath -OutputPath "$root/current" -Version '0.0.2' -ProductName $name -UpgradeCode $upgrade -TestFixture
    New-Item -ItemType Directory -Path $installed | Out-Null
    Set-Content (Join-Path $installed 'portable') 'portable'
    Require ((Run-Msi i $old @("INSTALLFOLDER=`"$installed`"")) -ne 0) 'Portable directory was accepted.'
    Remove-Item -LiteralPath (Join-Path $installed 'portable')
    Require ((Run-Msi i $old @("INSTALLFOLDER=`"$installed`"")) -eq 0) 'Initial installation failed.'
    Require (Test-Path -LiteralPath $registry) 'Missing installation registry entry.'
    Require (Test-Path -LiteralPath $shortcut) 'Missing Start menu shortcut.'
    $shell = New-Object -ComObject WScript.Shell
    Require ($shell.CreateShortcut($shortcut).TargetPath -eq (Join-Path $installed 'luciddesk.exe')) 'Shortcut target is incorrect.'
    New-Item -ItemType Directory -Path "$installed/data" | Out-Null
    Set-Content "$installed/data/keep.txt" 'retain user data'
    $lock = [IO.File]::Open((Join-Path $installed 'luciddesk_explorer.dll'), 'Open', 'Read', 'None')
    Require ((Run-Msi i $current) -ne 0) 'Locked component was overwritten.'
    $lock.Dispose(); $lock = $null
    $csc = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    & $csc /nologo /target:winexe "/out:$root\probe.exe" (Join-Path $repo 'tools\validation\installer-close-probe.cs')
    if ($LASTEXITCODE -ne 0) { throw 'Probe compilation failed.' }
    $hash = (Get-FileHash "$installed/luciddesk.exe").Hash
    $args = @("Local\LucidDesk.Test.$id", 'windows-window.Window', "`"$name`"", "`"$root/ready`"", "`"$root/closed`"", "`"$installed/luciddesk.exe`"", $hash, 'close')
    $probe = Start-Process "$root/probe.exe" -ArgumentList $args -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path "$root/ready") -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    Require (Test-Path "$root/ready") 'Probe did not start.'
    Require ((Run-Msi i $current @('CLOSEAPP=0')) -ne 0) 'CLOSEAPP=0 did not block a running app.'
    Require (-not $probe.HasExited) 'Blocked install closed the app.'
    Require ((Run-Msi i $current) -eq 0) 'Upgrade failed.'
    $windowsInstaller = New-Object -ComObject WindowsInstaller.Installer
    $database = $windowsInstaller.OpenDatabase($current, 0)
    $view = $database.OpenView("SELECT Value FROM Property WHERE Property = 'ProductCode'")
    $view.Execute()
    $productCode = $view.Fetch().StringData(1)
    $view.Close()
    Require ($windowsInstaller.ProductInfo($productCode, 'VersionString') -eq '0.0.2') 'Upgrade did not register the new version.'
    Require ($probe.WaitForExit(5000)) 'Upgrade did not wait for normal app exit.'
    Require ((Get-Content "$root/closed" -Raw).Trim() -eq 'normal exit completed') 'Normal exit failed.'
    Require ((Get-ItemProperty -LiteralPath $registry).InstallFolder.TrimEnd('\') -eq $installed.TrimEnd('\')) 'Upgrade changed the install directory.'
    Require ((Get-FileHash "$installed/luciddesk.exe").Hash -eq (Get-FileHash "$SourcePath/luciddesk.exe").Hash) 'Payload hash mismatch.'
    Require (Test-Path "$installed/installed") 'Installed marker is missing.'
    Require ((Run-Msi i $old) -ne 0) 'Downgrade was accepted.'
    Remove-Item -LiteralPath "$installed/LICENSE"
    Require ((Run-Msi fa $current) -eq 0) 'Repair failed.'
    Require (Test-Path "$installed/LICENSE") 'Repair did not restore the file.'
    Require ((Run-Msi x $current) -eq 0) 'Uninstall failed.'
    Require (-not (Test-Path "$installed/luciddesk.exe")) 'Uninstall retained the executable.'
    Require (-not (Test-Path -LiteralPath $registry)) 'Uninstall retained its registry entry.'
    Require (-not (Test-Path -LiteralPath $shortcut)) 'Uninstall retained its shortcut.'
    Require ((Get-Content "$installed/data/keep.txt" -Raw).Trim() -eq 'retain user data') 'Uninstall removed user data.'
    # Exercise the default directory too; normal installation supplies no path or scope.
    if (-not $AllUsers) {
        Require ((Run-Msi i $current) -eq 0) 'Default-directory installation failed.'
        $expected = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) "Programs\$name"
        Require ((Get-ItemProperty -LiteralPath $registry).InstallFolder.TrimEnd('\') -eq $expected) 'Default user directory is incorrect.'
        Require ((Run-Msi x $current) -eq 0) 'Default-directory uninstall failed.'
        Require (-not (Test-Path "$expected/luciddesk.exe")) 'Default-directory uninstall retained the executable.'
    }
    Write-Host "PASS: installation, portable/lock guards, normal exit, upgrade, downgrade, repair, uninstall and data preservation. Logs: $root"
} finally {
    if ($lock) { $lock.Dispose() }
    if ($probe -and -not $probe.HasExited) { $probe.Kill(); $probe.WaitForExit() }
    if (Test-Path -LiteralPath $registry) {
        # Only uninstall this randomly named fixture; never touch the real product.
        $package = if (Test-Path "$installed/installed") { $current } else { $old }
        if ($package) { $null = Run-Msi x $package }
        if (Test-Path -LiteralPath $registry) { $null = Run-Msi x $old }
        if (Test-Path -LiteralPath $registry) { Write-Warning "Fixture cleanup failed: $registry; logs: $root" }
    }
}
