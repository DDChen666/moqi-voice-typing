# 從原始碼編譯

> English: requirements are Xcode Command Line Tools, Rust, Bun and CMake on an Apple Silicon Mac. `bun install`, then `bun run tauri dev` or `bun run tauri build --bundles app`. Sign every build with the same identity (`scripts/signing_identity.sh`) so macOS keeps the permissions.

目前正式支援 **macOS（Apple 晶片）**。Windows 版正在移植，Linux 沒有計畫。

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
