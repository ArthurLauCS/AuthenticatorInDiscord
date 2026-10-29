# Fetches the bundled font (TeX Gyre Heros, regular + bold, about 270 KB) into tile\assets\fonts (not in git).
# Run once before a dev run and before building the installer. Files already present with the right hash are skipped.
# GUST Font License (see THIRD_PARTY_NOTICES.md). Official source: GUST e-foundry, TeX Gyre Heros 2.004 (OpenType).
# Uses curl.exe because Windows PowerShell 5.1 Invoke-WebRequest fails on some hosts here.
$dir = "$PSScriptRoot\..\tile\assets\fonts"
New-Item -ItemType Directory -Force $dir | Out-Null
$zipUrl = 'https://www.gust.org.pl/projects/e-foundry/tex-gyre/heros/qhv2.004otf.zip'
$zipSha256 = '755954b7349b803fc1c3d82fe9d9c4f7cf66467af718eaaf4f78ae1a09bf265d'
$fonts = @(
    @{ Name = 'texgyreheros-regular.otf'; Sha256 = '6ae1a09d5a940367b7aaaa91ee8bd8a2c333bfe193e7096e23f931357d62081f' },
    @{ Name = 'texgyreheros-bold.otf'; Sha256 = 'b170162835f4efc288886dd4231406dc47e19b614cf4416836635599d44a7d60' }
)
$missing = $fonts | Where-Object { -not ((Test-Path (Join-Path $dir $_.Name)) -and (Get-FileHash (Join-Path $dir $_.Name) -Algorithm SHA256).Hash -eq $_.Sha256) }
if (-not $missing) { exit 0 }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = Join-Path ([IO.Path]::GetTempPath()) ("authtile-font-" + [guid]::NewGuid().ToString('N') + '.zip')
try {
    & curl.exe -sSL --fail -o $zip $zipUrl
    if ($LASTEXITCODE) { throw "download failed: $zipUrl" }
    $zh = (Get-FileHash $zip -Algorithm SHA256).Hash
    if ($zh -ne $zipSha256) { throw "$zipUrl SHA-256 mismatch: $zh" }
    $za = [IO.Compression.ZipFile]::OpenRead($zip)
    try {
        foreach ($f in $missing) {
            $e = $za.GetEntry($f.Name)
            if (-not $e) { throw "$($f.Name) not in archive" }
            $path = Join-Path $dir $f.Name
            [IO.Compression.ZipFileExtensions]::ExtractToFile($e, $path, $true)
            $hash = (Get-FileHash $path -Algorithm SHA256).Hash
            if ($hash -ne $f.Sha256) { [IO.File]::Delete($path); throw "$($f.Name) SHA-256 mismatch: $hash" }
        }
    } finally { $za.Dispose() }
} finally { if (Test-Path $zip) { [IO.File]::Delete($zip) } }
