# 從原始碼編譯

> English: requirements are Xcode Command Line Tools, Rust, Bun and CMake on an Apple Silicon Mac. `bun install`, then `bun run tauri dev` or `bun run tauri build --bundles app`. Sign every build with the same identity (`scripts/signing_identity.sh`) so macOS keeps the permissions. On Windows (x64) you also need Visual Studio 2022 Build Tools with C++ and the Vulkan SDK; run `.\scripts\windows\env.ps1` in PowerShell, then `bun run tauri build --bundles nsis` (see [Windows](#windows)).

支援 **macOS（Apple 晶片）** 和 **Windows 10／11（x64）**，Windows 的步驟在最後的 [Windows](#windows) 一節。Linux 沒有計畫。

## 需要的工具

| 工具             | 安裝                                                       |
| ---------------- | ---------------------------------------------------------- |
| Xcode 命令列工具 | `xcode-select --install`                                   |
| Rust（stable）   | [rustup.rs](https://rustup.rs/)                            |
| Bun              | [bun.sh](https://bun.sh/)                                  |
| CMake            | `brew install cmake`（編譯語音辨識引擎 transcribe.cpp 用） |

第一次編譯要編 transcribe.cpp 和 Metal 核心，約 20 分鐘；之後約 4–6 分鐘。

## 開發

```sh
git clone https://github.com/DDChen666/moqi-voice-typing.git
cd moqi
bun install
bun run tauri dev
```

- 辨識模型（Qwen3-ASR 1.7B，約 1.5 GB）第一次打開時會自動下載到 `~/Library/Application Support/tw.yuyin.dictation/models/`。
- 靜音偵測模型 `silero_vad_v4.onnx` 已經放在 `src-tauri/resources/models/`。
- 潤稿服務的 API key 在 App 的「設定」裡填，存在鑰匙圈；開發時不需要任何環境變數。

## 編譯成 App

```sh
sh scripts/signing_identity.sh "Moqi Dev"          # 第一次會建立憑證，之後只解鎖
APPLE_SIGNING_IDENTITY="Moqi Dev" bun run tauri build --bundles app
```

產出在 `src-tauri/target/release/bundle/macos/Moqi.app`。

### 發佈用的安裝檔（維護者）

```sh
sh scripts/make_release_dmg.sh     # 用「Moqi Release」簽章，產出 dmg 並印出 SHA-256
```

- 公開版一律用同一張「Moqi Release」憑證簽章，用戶更新後權限才會保留。憑證的私鑰（`~/.config/moqi-signing/moqi-release/`）要另外備份。
- 腳本不用 `tauri build --bundles dmg`：在 exFAT 等外接硬碟上編譯時，那樣做出的 App 權限只有擁有者能讀，用戶拖進「應用程式」後會從啟動台消失；它的視窗排版步驟也需要「控制 Finder」的權限。

### 為什麼要固定的簽章

macOS 的「麥克風」和「輔助使用」權限跟著 App 的程式碼簽章走。

- 不指定 `APPLE_SIGNING_IDENTITY` 時，Tauri 用 ad-hoc 簽章，**每次編譯都會變**，每次都要重新授權。
- `scripts/signing_identity.sh` 建立一張只在你電腦上有效的自製憑證，放在獨立的鑰匙圈，不碰你的登入鑰匙圈。之後每次都用它簽章，權限就會保留。
- 這不是 Apple 的 Developer ID：第一次打開仍會被 Gatekeeper 擋下，要到「隱私權與安全性」按「強制打開」（見 [docs/安裝教學-Mac.md](docs/安裝教學-Mac.md)）。

### 裝進「應用程式」

```sh
pkill -f /Applications/Moqi.app/Contents/MacOS/handy || true
ditto src-tauri/target/release/bundle/macos/Moqi.app /Applications/Moqi.app
open /Applications/Moqi.app
```

程式碼放在 exFAT 等外接硬碟時，複製後的權限可能只有自己能讀，App 會從啟動台和 Spotlight 消失。補一行 `chmod -R go+rX /Applications/Moqi.app` 即可。

## 測試與檢查

提交前請跑：

```sh
cd src-tauri && cargo fmt && cargo test --release --lib && cd ..
bun x prettier --write . && bun run lint && bun x tsc --noEmit
```

這些也是 [CI](.github/workflows/ci.yml) 會跑的項目。

## 常見問題

### 重新編譯後，快捷鍵沒反應、權限卡在「等待中」

通常是簽章換了（例如忘了設 `APPLE_SIGNING_IDENTITY`）。先結束默契，清掉舊的授權紀錄，再重新打開並授權：

```sh
pkill -f /Applications/Moqi.app/Contents/MacOS/handy || true
tccutil reset Accessibility tw.yuyin.dictation
open /Applications/Moqi.app
```

### 每次裝新版，都跳出鑰匙圈密碼視窗

這是正常的。沒有 Apple 的 Team ID，macOS 會把每個新版本當成不同的程式，第一次讀取存在鑰匙圈裡的 API key 前要你允許一次。默契會在打開時就讀取，所以視窗會在一打開時出現，而不是說話途中。按「永遠允許」即可。

### 內部名稱為什麼是 `handy`、`yuyin`

- `handy`：執行檔名稱沿用 Handy，避免大量改動上游程式碼。
- `yuyin`（語音）：默契定名前的內部代號。App 識別碼 `tw.yuyin.dictation` 也保留，換掉的話所有人的權限和資料都要重來。

分支的規則見 [FORK.md](FORK.md)。

## Windows

### 需要的工具

在 PowerShell 裡用 winget 安裝（已經有的可以跳過）：

| 工具                           | 安裝                                                                                           |
| ------------------------------ | ---------------------------------------------------------------------------------------------- |
| Git                            | `winget install Git.Git`                                                                       |
| Rust（stable、MSVC）           | `winget install Rustlang.Rustup`，再執行 `rustup default stable`                               |
| Bun                            | `powershell -c "irm bun.sh/install.ps1 \| iex"`                                                |
| Visual Studio 2022 Build Tools | `winget install Microsoft.VisualStudio.2022.BuildTools`，在安裝程式裡勾「使用 C++ 的桌面開發」 |
| CMake                          | `winget install Kitware.CMake`                                                                 |
| Vulkan SDK                     | `winget install KhronosGroup.VulkanSDK`（裝完要開新的終端機）                                  |

辨識在顯示卡上跑（Vulkan）。NVIDIA、AMD、Intel 的顯示卡都可以；沒有的話會改用 CPU，比較慢。

### 編譯

```powershell
git clone https://github.com/DDChen666/moqi-voice-typing.git C:\src\moqi
cd C:\src\moqi
bun install
.\scripts\windows\env.ps1              # 每開一個新的 PowerShell 都要先跑一次
bun run tauri build --bundles nsis
```

- 產出在 `src-tauri\target\release\bundle\nsis\Moqi_x.y.z_x64-setup.exe`。
- 第一次編譯要編 transcribe.cpp 和 Vulkan 著色器，約 30 分鐘；之後改程式再編約 8–10 分鐘（大部分是最後的連結最佳化）。
- 開發：跑完 `env.ps1` 後 `bun run tauri dev`。
- 測試：跑完 `env.ps1` 後 `cd src-tauri; cargo test --release --lib`。

`scripts\windows\env.ps1` 做這些事（只影響這個 PowerShell 視窗）：

- 下載微軟官方的 ONNX Runtime 1.24.2（靜音偵測用），檢查 SHA-256 後改用動態連結，`build.rs` 會把 `onnxruntime.dll` 放進安裝檔。不用 ort 預設下載的靜態版本，因為它用 AVX2 編譯，在 2013 年以前的 CPU 上一啟動就會當掉。
- 找到 Visual Studio 附的 VC++ 執行階段，放進安裝檔，沒裝過 VC++ 的電腦也能開。
- 補上 `VULKAN_SDK` 和最新的 `PATH`（剛裝完工具、還沒重開終端機時也能編）。
- 把 `onnxruntime.dll` 複製到 `src-tauri\target\` 底下，讓 `tauri dev` 和測試用到的是這一份，而不是 Windows 內建的舊版（`C:\Windows\System32\onnxruntime.dll`）。

### 資料放在哪裡

| 東西                       | 位置                                                                                  |
| -------------------------- | ------------------------------------------------------------------------------------- |
| 設定、歷史、錄音、計時紀錄 | `%APPDATA%\tw.yuyin.dictation\`（`history.db`、`recordings\`、`yuyin_timings.jsonl`） |
| 程式紀錄                   | `%LOCALAPPDATA%\tw.yuyin.dictation\logs\handy.log`                                    |
| 辨識模型                   | `%USERPROFILE%\.cache\huggingface\hub\models--handy-computer--Qwen3-ASR-1.7B-gguf`    |
| API key                    | Windows 認證管理員（一般認證，名稱含 `tw.yuyin.dictation`）                           |
| 程式本身                   | `%LOCALAPPDATA%\Moqi\`（安裝給目前的使用者，不需要系統管理員）                        |

### 常見錯誤

**`error C2065: 'sp_space_id': 未宣告的識別項`（編 transcribe-cpp-sys 時）**
系統語言是中文的 Windows，編譯器預設用 Big5 讀原始碼，一個 UTF-8 字元把整行吃掉。`src-tauri\.cargo\config.toml` 已經讓 C/C++ 用 `/utf-8` 編譯；如果自己設了 `CFLAGS`／`CXXFLAGS`，記得也加上 `/utf-8`。

**`LNK2001: 無法解析的外部符號 __std_find_last_of_trivial_pos_1`（ort_sys）**
沒有先跑 `env.ps1`，ort 下載了需要更新版 MSVC 的靜態 ONNX Runtime。跑 `.\scripts\windows\env.ps1` 再編。

**`MSB3491`、`FTK1011` 或其他路徑太長的錯誤**
把程式碼 clone 到 `C:\src\moqi` 這種短路徑。

**`VULKAN_SDK is not set`**
Vulkan SDK 裝完要開新的終端機，或直接跑 `env.ps1`。

**makensis：`Invalid command` 出現在 `TradChinese.nsh` 第 1 行**
`src-tauri\nsis\TradChinese.nsh` 要存成沒有 BOM 的 UTF-8；Tauri 複製時會自己加 BOM。

**`cargo test` 的 `matches_the_evaluated_prompt_v3` 失敗**
測試用的提示詞檔被轉成 CRLF 換行。`.gitattributes` 已經指定保持 LF；如果是在加入它之前 clone 的，刪掉 `src-tauri\src\yuyin\testdata\*.txt` 再 `git checkout -- src-tauri/src/yuyin/testdata`。

**`bun run format:check` 說兩百多個檔案格式不對**
Git for Windows 預設會把取出的檔案換成 CRLF 換行（`core.autocrlf=true`），Prettier 要的是 LF。提交時 Git 會換回 LF，repo 裡的檔案沒有問題。只檢查格式、不管換行可以用 `npx prettier --check --end-of-line auto .`；想完全避開，clone 時加上 `-c core.autocrlf=false`（只影響這一份 repo）。

**App 打開後，第一次說話比較慢**
顯示卡在第一次辨識時才編譯運算程式（RTX 4070 上約 17 秒）。默契在 Windows 上會在啟動時就載入模型並在背景先跑一次，所以只有「剛打開的前 20 秒內就說話」才會等比較久。
