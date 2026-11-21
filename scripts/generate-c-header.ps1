# Generate cpp/include/svg3dx.h from svg3dx-ffi-c via cbindgen.
# Usage:
#   .\scripts\generate-c-header.ps1
#   .\scripts\generate-c-header.ps1 -Check   # fail if checked-in header is stale
param(
  [switch]$Check
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
if (-not $env:CARGO_TARGET_DIR) {
  $env:CARGO_TARGET_DIR = Join-Path $Root "target"
}

if (-not (Get-Command cbindgen -ErrorAction SilentlyContinue)) {
  Write-Host "==> install cbindgen"
  cargo install cbindgen --locked
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$config = Join-Path $Root "crates\svg3dx-ffi-c\cbindgen.toml"
$header = Join-Path $Root "cpp\include\svg3dx.h"
$outFile = if ($Check) {
  Join-Path $env:TEMP ("svg3dx-h-" + [guid]::NewGuid().ToString() + ".h")
} else {
  $header
}

Write-Host "==> cbindgen -> $(if ($Check) { 'temp (check)' } else { 'cpp\include\svg3dx.h' })"
# cbindgen prints expected "Can't find Svg3dx*" warnings for opaque types; capture without
# treating stderr as a terminating NativeCommandError under $ErrorActionPreference Stop.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$stderr = & cbindgen --config $config --crate svg3dx-ffi-c --output $outFile 2>&1
$code = $LASTEXITCODE
$ErrorActionPreference = $prevEap
foreach ($line in $stderr) {
  $text = "$line"
  # Opaque structs are declared in after_includes (Rust-only fields).
  if ($text -match "Can't find Svg3dx(Result|Session|PackageHandle|Buffer)") { continue }
  Write-Host $text
}
if ($code -ne 0) {
  if ($Check -and (Test-Path $outFile)) { Remove-Item $outFile -Force }
  exit $code
}

if (-not (Test-Path $outFile)) {
  Write-Error "cbindgen produced no output"
  exit 1
}

if ($Check) {
  if (-not (Test-Path $header)) {
    Remove-Item $outFile -Force
    Write-Error "Missing checked-in header: $header (run without -Check to generate)"
    exit 1
  }
  $expected = (Get-Content -Raw $outFile) -replace "`r`n", "`n"
  $actual = (Get-Content -Raw $header) -replace "`r`n", "`n"
  Remove-Item $outFile -Force
  if ($expected -ne $actual) {
    Write-Error "cpp\include\svg3dx.h is stale. Run: .\scripts\generate-c-header.ps1"
    exit 1
  }
  Write-Host "Header is up to date."
} else {
  Write-Host "Wrote $header"
}
