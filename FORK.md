# 關於這個分支：默契 Moqi

> 產品名稱是「默契」（英文 Moqi）。程式碼裡的 `yuyin`、註解裡的 `Yuyin fork:` 是早期的內部代號，保留不改。

這個程式是從 [Handy](https://github.com/cjpais/Handy) 分出來的（作者 CJ Pais，MIT 授權）。
分出的版本：upstream `main` 的 `29bd2c0`（2026-09-28，Handy 0.9.7）。

## 為什麼從 Handy 改，不從頭寫

Handy 已經做好 Mac 上最難的部分：

- 抓得到 Fn、右 Option 的全域快捷鍵
- 不搶焦點的懸浮窗
- 按住說話、切換的狀態機
- transcribe.cpp 加 Qwen3-ASR
- 靜音偵測
- 歷史紀錄
- 權限引導

我們要加的是產品層的東西：情境判斷、焦點檢查、依 App 調整貼上方式、潤飾程度、失敗後一鍵重貼等等。
選模型和整理方式的評估見 [docs/評測.md](docs/評測.md)，架構見 [docs/架構.md](docs/架構.md)。

## 維護方式

- **硬分支**：不追著 upstream 合併。每個月看一次 upstream 的修正，需要的用 `git cherry-pick` 挑進來。
  - `git fetch upstream && git log main..upstream/main --oneline`
- **我們的程式碼盡量放在新檔案**，只在 Handy 原本的流程裡加最少的接點。
  - Handy 最常改的檔案（`actions.rs`、`settings.rs`、`overlay.rs`、`lib.rs`、`shortcut/`）我們改得越少，挑修正時越不會衝突。
- 每個對 Handy 原本檔案的改動，都在註解標 `Yuyin fork:`，方便搜尋。
- `src-tauri/vendor/handy-keys` 是 Handy 的鍵盤套件 handy-keys 0.3.4 的複本，只多一個 Windows 修正：按住右 Alt 時，鍵盤的自動重複會漏給前景的 App，App 以為單按了 Alt，就把焦點移到選單，貼上落空。upstream 修好後就刪掉這份複本，改回 crates.io 的版本（`src-tauri/Cargo.toml` 的 `[patch.crates-io]`）。

## 授權

- 保留原本的 `LICENSE`（Copyright (c) 2025 CJ Pais），再加上默契的版權行。App 的「關於」頁面（`src/yuyin/YuyinAbout.tsx`）會顯示 Handy 的致謝和完整授權條款，條款內容直接讀這個 `LICENSE` 檔。
- Handy 的「新功能」彈窗拿掉了（內容是 Handy 的版本紀錄）。版本號從 0.1.0 重新開始。
- MIT 不包含商標，所以不能用「Handy」這個名字。App 名稱（默契／Moqi）、識別碼（`tw.yuyin.dictation`）都已經換掉，Handy 原本的自動更新也關了。
  - Handy 的更新檢查在 `settings.rs` 的 `update_checks_forced_disabled()`。默契自己的更新只看默契的 GitHub Release（`src/yuyin/update.ts`，發布方式見 [docs/發布新版.md](docs/發布新版.md)）。
- 辨識模型 Qwen3-ASR 1.7B 從 Hugging Face 的 `handy-computer/Qwen3-ASR-1.7B-gguf` 下載，版本和檔案雜湊都固定寫在 `src-tauri/src/catalog/catalog.json`。
  - 靜音偵測模型 `silero_vad_v4.onnx` 打包在 App 裡。
  - Handy 其他舊模型還是從 `blob.handy.computer` 下載，但新介面已經不顯示它們。
