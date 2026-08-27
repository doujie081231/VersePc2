# build-msvc.ps1 — 导入 MSVC 环境并执行便携版打包（MSVC 单文件便携包）
$vcvars = 'F:\vs\VC\Auxiliary\Build\vcvars64.bat'
$raw = & cmd /c "call `"$vcvars`" >nul 2>&1 && set" 2>$null
foreach ($line in $raw) {
  if ($line -match '^(.*?)=(.*)$') {
    Set-Item -Path "Env:$($matches[1])" -Value $matches[2]
  }
}
$env:RUSTUP_HOME = 'F:\versepc2\.tools\rustup'
$env:CARGO_HOME = 'F:\versepc2\.tools\cargo'
$env:PATH = 'F:\tools\node\node-v20.19.6-win-x64;F:\versepc2\.tools\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin;F:\versepc2\.tools\cargo\bin;' + $env:PATH
$env:CARGO_TARGET_DIR = 'F:\versepc2\.target-msvc'
$env:VERSEPC2_TARGET_DIR = 'F:\versepc2\.target-msvc'
$env:VERSEPC2_BUILD_ROOT = 'F:\versepc2'
Write-Host '[build-msvc] MSVC 环境已导入，开始打包...'
node 'F:\versepc2\scripts\build-portable.mjs'
exit $LASTEXITCODE