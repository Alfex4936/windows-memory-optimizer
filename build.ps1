$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo fmt --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting failed' }
    cargo clippy --all-targets -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed' }
    cargo test -- --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed' }
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    $packagePath = Join-Path $PSScriptRoot 'dist\Memori'
    New-Item -ItemType Directory -Path $packagePath -Force | Out-Null
    Copy-Item -LiteralPath 'target\release\memori.exe' -Destination $packagePath
    Copy-Item -LiteralPath 'README.md', 'LICENSE' -Destination $packagePath
    if (Test-Path -LiteralPath 'README.en.md') {
        Copy-Item -LiteralPath 'README.en.md' -Destination $packagePath
    }
    Copy-Item -LiteralPath 'assets\memori.ico' -Destination $packagePath
    $assetPath = Join-Path $packagePath 'assets'
    $screenshotPath = Join-Path $assetPath 'screenshots'
    $docsPath = Join-Path $packagePath 'docs'
    New-Item -ItemType Directory -Path $screenshotPath, $docsPath -Force | Out-Null
    Copy-Item -LiteralPath 'assets\icon.svg' -Destination $assetPath
    Copy-Item -LiteralPath 'assets\screenshots\compact.png', 'assets\screenshots\widget.png' -Destination $screenshotPath
    Copy-Item -LiteralPath 'docs\usage.md' -Destination $docsPath
    python scripts\bundle_notices.py
    if ($LASTEXITCODE -ne 0) { throw 'License bundling failed' }
    $manifestText = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot 'Cargo.toml'))
    $version = [regex]::Match($manifestText, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $version) { throw 'Package version was not found' }
    $archiveName = "Memori-$version-windows-x64.zip"
    $archivePath = Join-Path $PSScriptRoot "dist\$archiveName"
    Compress-Archive -Path (Join-Path $packagePath '*') -DestinationPath $archivePath -Force
    $hashLines = @(
        "$((Get-FileHash -LiteralPath (Join-Path $packagePath 'memori.exe') -Algorithm SHA256).Hash.ToLowerInvariant())  memori.exe"
        "$((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant())  $archiveName"
    )
    [System.IO.File]::WriteAllLines((Join-Path $PSScriptRoot 'dist\SHA256SUMS.txt'), $hashLines, [System.Text.UTF8Encoding]::new($false))
    Write-Host "Ready: $packagePath\memori.exe"
}
finally { Pop-Location }
