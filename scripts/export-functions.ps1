param([Parameter(Mandatory=$true)][string[]]$Addresses)
$ErrorActionPreference='Stop'
foreach($address in $Addresses) {
    if($address -notmatch '^[0-9a-fA-F]{8}$') {throw 'Pass each address as a quoted, eight-digit hexadecimal string (PowerShell treats e digits as exponents).'}
}
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try {
    $env:JAVA_HOME='C:\Program Files\Java\jdk-26'
    $env:PATH="$env:JAVA_HOME\bin;$env:PATH"
    & .\local\tools\ghidra_12.0.3_PUBLIC\support\analyzeHeadless.bat local\research\ghidra AVPE-Xbox -process default.xbe -noanalysis -scriptPath scripts -postScript ExportFunctions.java local\research\decompiled @Addresses -log local\research\ghidra-functions.log *> local\research\ghidra-functions-console.log
    if ($LASTEXITCODE -ne 0) { throw 'Ghidra function export failed. See local/research/ghidra-functions-console.log.' }
} finally { Pop-Location }
