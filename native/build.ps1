param([switch]$Publish)
$ErrorActionPreference = 'Stop'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:MSBuildEnableWorkloadResolver = 'false'
$project = Join-Path $PSScriptRoot 'AgentUI/AgentUI.csproj'
if ($Publish) {
    dotnet publish $project -c Release -p:RestoreLockedMode=true -p:DebugType=None -o (Join-Path $PSScriptRoot 'artifacts/windows')
} else {
    dotnet build $project -c Release -p:RestoreLockedMode=true
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
if ($Publish) {
    $output = Join-Path $PSScriptRoot 'artifacts/windows'
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination $output
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'verification.md') -Destination $output
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../LICENSE') -Destination $output
    $notes = [System.Text.StringBuilder]::new()
    [void]$notes.AppendLine('AgentUI third-party components. Versions are pinned in AgentUI/packages.lock.json.')
    $lock = Get-Content -Raw (Join-Path $PSScriptRoot 'AgentUI/packages.lock.json') | ConvertFrom-Json
    $cache = if ($env:NUGET_PACKAGES) { $env:NUGET_PACKAGES } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.nuget/packages' }
    $seenNotices = [System.Collections.Generic.HashSet[string]]::new()
    foreach ($package in $lock.dependencies.'.NETFramework,Version=v4.8'.PSObject.Properties) {
        if ($package.Name.StartsWith('Microsoft.NETFramework.ReferenceAssemblies')) { continue }
        $folder = Join-Path $cache ($package.Name.ToLowerInvariant() + '/' + $package.Value.resolved)
        [xml]$metadata = Get-Content -Raw (Join-Path $folder ($package.Name.ToLowerInvariant() + '.nuspec'))
        [void]$notes.AppendLine("`n$($package.Name) $($package.Value.resolved) — $($metadata.package.metadata.license.InnerText)")
        [void]$notes.AppendLine([string]$metadata.package.metadata.copyright)
        foreach ($notice in Get-ChildItem -LiteralPath $folder -Filter '*NOTICES*' -File) {
            if ($seenNotices.Add((Get-FileHash -LiteralPath $notice.FullName).Hash)) {
                [void]$notes.AppendLine((Get-Content -Raw -LiteralPath $notice.FullName))
            }
        }
    }
    foreach ($license in Get-ChildItem (Join-Path $PSScriptRoot 'licenses') -File) { [void]$notes.AppendLine((Get-Content -Raw -LiteralPath $license.FullName)) }
    [System.IO.File]::WriteAllText((Join-Path $output 'THIRD-PARTY-NOTICES.txt'), $notes.ToString())
    $files = Get-ChildItem -LiteralPath $output -File | Where-Object Extension -NE '.pdb'
    $zip = Join-Path $PSScriptRoot 'artifacts/AgentUI-windows.zip'
    Compress-Archive -LiteralPath $files.FullName -DestinationPath $zip -Force
    (Get-FileHash -LiteralPath $zip).Hash | Set-Content (Join-Path $PSScriptRoot 'artifacts/AgentUI-windows.sha256')
    Write-Output "Package: $zip"
    Write-Output "Installed files: $([math]::Round(($files | Measure-Object Length -Sum).Sum / 1MB, 2)) MiB; zip: $([math]::Round((Get-Item $zip).Length / 1MB, 2)) MiB"
}
