# Builds a release and packs a Velopack installer into .\Releases.
# With -Publish it also uploads the release to GitHub (needs a GH_TOKEN env var with `repo` scope).
# Requirements: .NET SDK and `dotnet tool install -g vpk`.
param([switch]$Publish)
$ErrorActionPreference = 'Stop'

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

# vpk packs a whole folder, so stage only the exe.
$stage = 'target\pack'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\seindtask.exe $stage

vpk pack --packId seindtask --packVersion $version --packDir $stage --mainExe seindtask.exe --runtime win-x64 `
    --packTitle 'Odak' --icon assets\icon.ico --framework vcredist143-x64 --outputDir Releases
if ($LASTEXITCODE -ne 0) { throw 'vpk pack failed' }

if ($Publish) {
    if (-not $env:GH_TOKEN) { throw 'GH_TOKEN is not set' }
    vpk upload github --repoUrl https://github.com/seind-dev/Odak --token $env:GH_TOKEN `
        --publish --releaseName "v$version" --tag "v$version" --outputDir Releases
    if ($LASTEXITCODE -ne 0) { throw 'vpk upload failed' }
}
