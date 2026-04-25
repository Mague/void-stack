param(
    [Parameter(Mandatory)][string]$Platform,
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Version
)

$StagingDir = "staging-extension"
$Output = "void-stack-$Version-$Platform.mcpb"

Remove-Item -Recurse -Force $StagingDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $StagingDir | Out-Null

Copy-Item $Binary "$StagingDir\void-stack-mcp.exe"
(Get-Content manifest.json -Raw) `
    -replace '"void-stack-mcp"', '"void-stack-mcp.exe"' `
    -replace '"version": ".*?"', "`"version`": `"$Version`"" |
    Set-Content "$StagingDir\manifest.json"

Compress-Archive -Path "$StagingDir\*" -DestinationPath $Output -Force
Remove-Item -Recurse -Force $StagingDir

Write-Host "Packaged: $Output"
