$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$tauriDir = Join-Path $root "src-tauri"
$modelFiles = @(
    @{
        Path = "models\llm\qwen2.5-0.5b-instruct-q4_k_m.gguf"
        Url = "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/9217f5db79a29953eb74d5343926648285ec7e67/qwen2.5-0.5b-instruct-q4_k_m.gguf?download=true"
        Size = 491400032
        Sha256 = "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db"
    },
    @{
        Path = "models\llm\tokenizer.json"
        Url = "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct/resolve/7ae557604adf67be50417f59c2c2f167def9a775/tokenizer.json?download=true"
        Size = 7031645
        GitBlobSha1 = "443909a61d429dff23010e5bddd28ff530edda00"
    },
    @{
        Path = "models\llm\LICENSE-Qwen.txt"
        Url = "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/9217f5db79a29953eb74d5343926648285ec7e67/LICENSE?download=true"
        Size = 11343
        GitBlobSha1 = "6634c8cc3133b3848ec74b9f275acaaa1ea618ab"
    },
    @{
        Path = "models\embeddings\bge-small-en-v1.5-q4_k_m.gguf"
        Url = "https://huggingface.co/CompendiumLabs/bge-small-en-v1.5-gguf/resolve/d32f8c040ea3b516330eeb75b72bcc2d3a780ab7/bge-small-en-v1.5-q4_k_m.gguf?download=true"
        Size = 24808576
        Sha256 = "363a0a4855dff6c653e06efe3209157debcf7f74e52d0d7c71e2747cd523043e"
    }
)

foreach ($model in $modelFiles) {
    $modelPath = Join-Path $root $model.Path
    $modelDirectory = Split-Path -Parent $modelPath
    New-Item -ItemType Directory -Path $modelDirectory -Force | Out-Null

    $needsDownload = -not (Test-Path -LiteralPath $modelPath)
    if (-not $needsDownload) {
        $existingFile = Get-Item -LiteralPath $modelPath
        $needsDownload = $existingFile.Length -ne $model.Size
        if (-not $needsDownload -and $model.ContainsKey("Sha256")) {
            $existingHash = (Get-FileHash -LiteralPath $modelPath -Algorithm SHA256).Hash.ToLowerInvariant()
            $needsDownload = $existingHash -ne $model.Sha256
        }
        if (-not $needsDownload -and $model.ContainsKey("GitBlobSha1")) {
            $existingHash = (git hash-object -- $modelPath).ToLowerInvariant()
            if ($LASTEXITCODE -ne 0) {
                throw "Could not verify $($model.Path); Git is required for the asset integrity check."
            }
            $needsDownload = $existingHash -ne $model.GitBlobSha1
        }
    }

    if ($needsDownload) {
        $temporaryPath = "$modelPath.download"
        Remove-Item -LiteralPath $temporaryPath -Force -ErrorAction SilentlyContinue
        Write-Host "Downloading $($model.Path)..."
        & curl.exe --fail --location --retry 5 --retry-all-errors --output $temporaryPath $model.Url
        if ($LASTEXITCODE -ne 0) {
            Remove-Item -LiteralPath $temporaryPath -Force -ErrorAction SilentlyContinue
            throw "Download failed for $($model.Path) with exit code $LASTEXITCODE."
        }

        $downloadedFile = Get-Item -LiteralPath $temporaryPath
        if ($downloadedFile.Length -ne $model.Size) {
            Remove-Item -LiteralPath $temporaryPath -Force
            throw "Downloaded $($model.Path) has an unexpected size of $($downloadedFile.Length) bytes."
        }
        if ($model.ContainsKey("Sha256")) {
            $downloadedHash = (Get-FileHash -LiteralPath $temporaryPath -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($downloadedHash -ne $model.Sha256) {
                Remove-Item -LiteralPath $temporaryPath -Force
                throw "SHA-256 verification failed for $($model.Path)."
            }
        }
        if ($model.ContainsKey("GitBlobSha1")) {
            $downloadedHash = (git hash-object -- $temporaryPath).ToLowerInvariant()
            if ($LASTEXITCODE -ne 0 -or $downloadedHash -ne $model.GitBlobSha1) {
                Remove-Item -LiteralPath $temporaryPath -Force
                throw "Git blob SHA-1 verification failed for $($model.Path)."
            }
        }

        Move-Item -LiteralPath $temporaryPath -Destination $modelPath -Force
    }
}

$monetaExecutablePaths = @(
    (Join-Path $root "Moneta.exe"),
    (Join-Path $root "dist\Moneta.exe"),
    (Join-Path $tauriDir "target\release\moneta.exe")
) | ForEach-Object { [System.IO.Path]::GetFullPath($_) }

$runningMoneta = Get-Process -Name "moneta" -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Path -and $monetaExecutablePaths -contains [System.IO.Path]::GetFullPath($_.Path)
    }
if ($runningMoneta) {
    throw "Quit the running Moneta app before building so Windows can replace its executable."
}

Push-Location $tauriDir
try {
    & cargo tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri release build failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$builtExe = Join-Path $tauriDir "target\release\moneta.exe"
if (-not (Test-Path $builtExe)) {
    throw "Release executable was not produced at $builtExe."
}

$distDir = Join-Path $root "dist"
New-Item -ItemType Directory -Path $distDir -Force | Out-Null
foreach ($destination in @(
    (Join-Path $root "Moneta.exe"),
    (Join-Path $distDir "Moneta.exe")
)) {
    Copy-Item -LiteralPath $builtExe -Destination $destination -Force
}

$installer = Join-Path $tauriDir "target\release\bundle\nsis\Moneta_0.1.0_x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer)) {
    throw "NSIS installer was not produced at $installer."
}
$downloadableInstaller = Join-Path $distDir "Moneta-Setup.exe"
Copy-Item -LiteralPath $installer -Destination $downloadableInstaller -Force

Write-Host "Release executable copied to Moneta.exe and dist\Moneta.exe."
Write-Host "Offline Windows setup created at dist\Moneta-Setup.exe."
