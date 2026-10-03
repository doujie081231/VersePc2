$MSVC = 'D:\VerseTools\VSBuildTools\VC\Tools\MSVC\14.44.35207'
$SDK  = 'C:\Program Files (x86)\Windows Kits\10'
$SDKV = '10.0.26100.0'

$env:VCINSTALLDIR = 'D:\VerseTools\VSBuildTools\VC'
$env:WindowsSdkDir = "$SDK\"
$env:WindowsSdkVersion = "$SDKV\"
$env:UniversalCRTSdkDir = "$SDK\"

$bins = @(
  "$MSVC\bin\Hostx64\x64",
  "$SDK\bin\$SDKV\x64",
  'D:\VerseTools\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin',
  'D:\VerseTools\.cargo\bin',
  'D:\VerseTools\node-v22.23.2-win-x64'
)
$env:PATH = ($bins + $env:PATH) -join ';'

$inc = @(
  "$MSVC\include",
  "$MSVC\atlmfc\include",
  "$SDK\Include\$SDKV\ucrt",
  "$SDK\Include\$SDKV\um",
  "$SDK\Include\$SDKV\shared",
  "$SDK\Include\$SDKV\winrt"
)
$env:INCLUDE = $inc -join ';'

$lib = @(
  "$MSVC\lib\x64",
  "$MSVC\atlmfc\lib\x64",
  "$SDK\Lib\$SDKV\ucrt\x64",
  "$SDK\Lib\$SDKV\um\x64"
)
$env:LIB = $lib -join ';'
$env:LIBPATH = $lib -join ';'

$env:RUSTUP_HOME = 'D:\VerseTools\.rustup'
$env:CARGO_HOME  = 'D:\VerseTools\.cargo'
$env:CARGO_TARGET_DIR = 'E:\VerseTools\.verse-target'
$env:VERSEPC2_TARGET_DIR = 'E:\VerseTools\.verse-target'
$env:VERSEPC2_BUILD_ROOT = 'D:\Verse Explorer X\versepc2'

Write-Host '[build-msvc] MSVC env imported'
if (Get-Command cl.exe -ErrorAction SilentlyContinue) { Write-Host "cl: OK" } else { Write-Host "cl: MISSING" }
if (Get-Command cargo.exe -ErrorAction SilentlyContinue) { Write-Host "cargo: OK" } else { Write-Host "cargo: MISSING" }
if (Get-Command node.exe -ErrorAction SilentlyContinue) { Write-Host "node: OK" } else { Write-Host "node: MISSING" }

Set-Location 'D:\Verse Explorer X\versepc2'
node 'D:\Verse Explorer X\versepc2\scripts\build-portable.mjs'
exit $LASTEXITCODE