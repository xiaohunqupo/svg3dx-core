# Build svg3dx-core release artifacts: tests, C ABI DLL, dump_json, WASM pkg.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$env:CARGO_TARGET_DIR = Join-Path $Root "target"

Write-Host "==> cargo test svg3dx-core"
cargo test -p svg3dx-core
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> cargo build svg3dx-ffi-c --release"
cargo build -p svg3dx-ffi-c --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$dll = Join-Path $Root "target\release\svg3dx_core.dll"
if (-not (Test-Path $dll)) {
  Write-Error "DLL not found: $dll"
}

Write-Host "==> build dump_json (CMake)"
$buildDir = Join-Path $Root "cpp\build"
New-Item -ItemType Directory -Force -Path $buildDir | Out-Null
cmake -S (Join-Path $Root "cpp") -B $buildDir
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cmake --build $buildDir --config Release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> wasm-pack -> pkg/"
if (-not (Get-Command wasm-pack -ErrorAction SilentlyContinue)) {
  cargo install wasm-pack --locked
}
rustup target add wasm32-unknown-unknown | Out-Null
$pkgOut = Join-Path $Root "pkg"
wasm-pack build (Join-Path $Root "crates\svg3dx-ffi-wasm") --target bundler --out-dir $pkgOut --out-name svg3dx_ffi_wasm
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "Done."
Write-Host "  DLL:  $dll"
Write-Host "  CLI:  cpp\build\Debug\dump_json.exe"
Write-Host "  WASM: pkg\"
Write-Host ""
Write-Host "Try:"
Write-Host "  .\cpp\build\Debug\dump_json.exe ..\svg3dx\samples\draft-0.1-example.svg3dx --dll .\target\release\svg3dx_core.dll"
Write-Host "  (viewer/studio are separate repos: ../svg3dx-viewer, ../svg3dx-studio)"
