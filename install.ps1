# Installs AIPL (https://github.com/mwarpinski/AIPL) on Windows, for the current user.
#
#   irm https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.ps1 | iex
#
# Or read it first:
#
#   irm https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.ps1 -OutFile install.ps1
#   notepad install.ps1; powershell -ExecutionPolicy Bypass -File install.ps1
#
# What it does, and nothing else:
#   1. downloads the release for Windows (x86-64) and checks it against its
#      published SHA-256; it needs no git, no Rust, and no administrator rights;
#   2. unpacks it into %USERPROFILE%\.aipl: the two programs, aipl.exe and
#      aipl-run.exe, in bin, and the README, specification, and examples in share;
#   3. adds %USERPROFILE%\.aipl\bin to your user PATH (open a new terminal to see it);
#   4. checks the result by compiling and running a small program.
# Nothing is written outside .aipl except that PATH entry. Run it again to
# update; set AIPL_UNINSTALL=1 to remove AIPL and its PATH entry.
#
# Options are environment variables (a pipe to iex takes no arguments):
#   AIPL_VERSION    the release to install, e.g. v0.1.0 (default: the latest)
#   AIPL_HOME       where to install (default: %USERPROFILE%\.aipl)
#   AIPL_RELEASES   the URL the release files are under, or a folder (default:
#                   https://github.com/mwarpinski/AIPL/releases)
#   AIPL_NO_PATH=1  leave the PATH alone
#   AIPL_UNINSTALL=1  remove an installation

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Say($m) { Write-Host "aipl-install: $m" }
function Fail($m) { throw "aipl-install: error: $m" }

$AiplHome = if ($env:AIPL_HOME) { $env:AIPL_HOME } else { Join-Path $env:USERPROFILE '.aipl' }
$Version = if ($env:AIPL_VERSION) { $env:AIPL_VERSION } else { 'latest' }
$Releases = if ($env:AIPL_RELEASES) { $env:AIPL_RELEASES } else { 'https://github.com/mwarpinski/AIPL/releases' }
$Bin = Join-Path $AiplHome 'bin'

function Remove-FromPath {
    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($path) {
        $kept = ($path -split ';') | Where-Object { $_ -and ($_ -ne $Bin) }
        [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
    }
}

if ($env:AIPL_UNINSTALL -eq '1') {
    if (-not (Test-Path $AiplHome)) { Fail "nothing to remove: $AiplHome does not exist" }
    Remove-FromPath
    Remove-Item -Recurse -Force $AiplHome
    Say "removed $AiplHome and its PATH entry"
    return
}

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($arch -ne 'X64') { Fail "there is no Windows release for $arch yet (x86-64 only)" }
$target = 'x86_64-pc-windows-msvc'
$name = "aipl-$target.zip"
$base = if ($Version -eq 'latest') { "$Releases/latest/download" } else { "$Releases/download/$Version" }

# a file under $base: downloaded, or copied when $base is a folder (file:// or a path)
function Get-Release($file, $to) {
    if ($base -match '^https?://') {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$file" -OutFile $to }
        catch { Fail "could not download $base/$file (is there a release $Version?): $($_.Exception.Message)" }
    } else {
        $dir = $base -replace '^file:///?', ''
        Copy-Item -ErrorAction Stop (Join-Path $dir $file) $to
    }
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("aipl-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    Say "downloading $name ($Version)"
    Get-Release $name (Join-Path $tmp $name)
    Get-Release "$name.sha256" (Join-Path $tmp "$name.sha256")
    $want = ((Get-Content (Join-Path $tmp "$name.sha256") -TotalCount 1) -split '\s+')[0].ToLower()
    $have = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $name)).Hash.ToLower()
    if ($have -ne $want) { Fail "the download does not match its SHA-256 ($want); not installing it" }
    Expand-Archive -Path (Join-Path $tmp $name) -DestinationPath $tmp
    $dir = Join-Path $tmp "aipl-$target"
    foreach ($b in 'aipl.exe', 'aipl-run.exe') {
        if (-not (Test-Path (Join-Path $dir "bin\$b"))) { Fail 'the download is not an AIPL release' }
    }
    New-Item -ItemType Directory -Force $Bin | Out-Null
    foreach ($b in 'aipl.exe', 'aipl-run.exe') {
        try { Copy-Item -Force (Join-Path $dir "bin\$b") (Join-Path $Bin $b) }
        catch { Fail "could not replace $b (is it running?): $($_.Exception.Message)" }
    }
    if (Test-Path (Join-Path $AiplHome 'share')) { Remove-Item -Recurse -Force (Join-Path $AiplHome 'share') }
    Copy-Item -Recurse (Join-Path $dir 'share') (Join-Path $AiplHome 'share')
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

if ($env:AIPL_NO_PATH -ne '1') {
    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($path -split ';') -contains $Bin)) {
        $new = if ($path) { $path.TrimEnd(';') + ';' + $Bin } else { $Bin }
        [Environment]::SetEnvironmentVariable('Path', $new, 'User')
        Say "added $Bin to your user PATH"
    }
    if (-not (($env:Path -split ';') -contains $Bin)) { $env:Path += ";$Bin" }
}

# compiles and runs a program importing the standard library, from outside the install
$work = Join-Path ([IO.Path]::GetTempPath()) ("aipl-check-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null
try {
    @'
(module hello
  (import io)
  (fn main [] -> i32
    (call io.println "AIPL is installed")
    0))
'@ | Set-Content -Encoding ascii (Join-Path $work 'hello.aipl')
    Push-Location $work
    & (Join-Path $Bin 'aipl.exe') compile hello.aipl -o hello.wasm | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail 'the installed aipl could not compile a test program' }
    & (Join-Path $Bin 'aipl-run.exe') hello.wasm
    if ($LASTEXITCODE -ne 0) { Fail 'the installed aipl-run could not run a test program' }
} finally {
    Pop-Location
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
Say ("installed " + (& (Join-Path $Bin 'aipl.exe') --version))
Say "done. Open a new terminal to use aipl; the language: $(Join-Path $AiplHome 'share\README.md')"
