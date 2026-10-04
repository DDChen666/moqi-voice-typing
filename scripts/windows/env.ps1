# Build environment for Moqi on Windows. Run it in the PowerShell you build
# from (it sets process environment variables):
#
#   .\scripts\windows\env.ps1
#
# - Downloads Microsoft's ONNX Runtime (used by the Silero VAD) once, checks
#   its SHA-256, and links it dynamically. The alternative, ort's default
#   download, is a static library built with /arch:AVX2 that crashes at start
#   on CPUs older than Haswell and needs MSVC 14.43 or later to link.
#   build.rs copies onnxruntime.dll next to handy.exe in the installer.
# - Points build.rs at the VC++ runtime DLLs to ship beside handy.exe.
# - Picks up VULKAN_SDK when the SDK was installed after this terminal opened.
# - Refreshes PATH so freshly installed tools (bun, cargo) are found.

# A script block, so 'Stop' doesn't leak into the caller's session (where
# it would turn cargo's progress on stderr into errors).
& {
    $ErrorActionPreference = 'Stop'

    $OrtVersion = '1.24.2'
    $OrtSha256 = '8e3e9c826375352e29cb2614fe44f3d7a4b0ff7b8028ad7a456af9d949a7e8b0'
    $OrtName = "onnxruntime-win-x64-$OrtVersion"
    $Cache = Join-Path $env:LOCALAPPDATA 'moqi-build'
    $OrtDir = Join-Path $Cache $OrtName

    if (-not (Test-Path (Join-Path $OrtDir 'lib\onnxruntime.dll'))) {
        New-Item -ItemType Directory -Force $Cache | Out-Null
        $zip = Join-Path $Cache "$OrtName.zip"
        if (-not (Test-Path $zip)) {
            Write-Host "Downloading ONNX Runtime $OrtVersion..."
            $url = "https://github.com/microsoft/onnxruntime/releases/download/v$OrtVersion/$OrtName.zip"
            Invoke-WebRequest -Uri $url -OutFile $zip
        }
        $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
        if ($hash -ne $OrtSha256) {
            Remove-Item $zip
            throw "ONNX Runtime download has SHA-256 $hash, expected $OrtSha256"
        }
        Expand-Archive -Path $zip -DestinationPath $Cache -Force
    }

    $env:ORT_LIB_LOCATION = Join-Path $OrtDir 'lib'
    $env:ORT_PREFER_DYNAMIC_LINK = '1'

    $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
        [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $env:VULKAN_SDK) {
        $env:VULKAN_SDK = [Environment]::GetEnvironmentVariable('VULKAN_SDK', 'Machine')
    }
    if (-not $env:VULKAN_SDK) {
        throw 'VULKAN_SDK is not set: install the Vulkan SDK (winget install KhronosGroup.VulkanSDK)'
    }

    # Ship the VC++ runtime next to handy.exe (build.rs stages it), so the app
    # starts on PCs without the Visual C++ Redistributable (Handy #1527).
    if (-not $env:HANDY_VC_REDIST_DIRS) {
        $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
        $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        $crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory -ErrorAction SilentlyContinue |
            Sort-Object FullName | Select-Object -Last 1
        if (-not $crt) {
            throw "No VC++ redistributable under $vs; install the C++ desktop workload"
        }
        $env:HANDY_VC_REDIST_DIRS = $crt.FullName
    }

    # `tauri dev` and the tests run handy.exe and the test binaries straight from
    # target\. Windows looks next to the .exe before PATH, and System32 ships an
    # older onnxruntime.dll (Windows ML), so put ours where they find it first.
    $target = Join-Path $PSScriptRoot '..\..\src-tauri\target'
    foreach ($dir in 'release', 'release\deps', 'debug', 'debug\deps') {
        $path = Join-Path $target $dir
        New-Item -ItemType Directory -Force $path | Out-Null
        Copy-Item (Join-Path $env:ORT_LIB_LOCATION 'onnxruntime.dll') $path -Force
    }

    Write-Host "ORT_LIB_LOCATION=$env:ORT_LIB_LOCATION"
    Write-Host "VULKAN_SDK=$env:VULKAN_SDK"
    Write-Host "HANDY_VC_REDIST_DIRS=$env:HANDY_VC_REDIST_DIRS"
}
