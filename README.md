<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/banner-dark.png">
  <img src="docs/media/banner-light.png" width="100%" alt="默契 Moqi：讓默契幫你打字。中英夾雜，一聽就懂。聲音在你的 Mac 上辨識，不會上傳。">
</picture>

<br>

<p align="center">
  <a href="https://github.com/DDChen666/moqi-voice-typing/releases/latest"><img src="docs/media/download.png" width="300" alt="下載 macOS 版"></a>
</p>

<p align="center">
  <a href="docs/安裝教學-Mac.md"><b>安裝教學</b></a> &nbsp;·&nbsp;
  <a href="docs/使用指南.md"><b>使用指南</b></a> &nbsp;·&nbsp;
  <a href="docs/隱私.md"><b>隱私</b></a> &nbsp;·&nbsp;
  <a href="docs/評測.md"><b>評測</b></a> &nbsp;·&nbsp;
  <a href="CHANGELOG.md"><b>更新紀錄</b></a> &nbsp;·&nbsp;
  <a href="README.en.md"><b>English</b></a>
</p>

<p align="center">
  <img alt="版本 1.0.0" src="https://img.shields.io/badge/%E7%89%88%E6%9C%AC-1.0.0-0a84ff?style=flat-square">
  <img alt="macOS 15+" src="https://img.shields.io/badge/macOS-15%2B-1d1d1f?style=flat-square&logo=apple&logoColor=white">
  <img alt="Apple 晶片" src="https://img.shields.io/badge/Apple-%E6%99%B6%E7%89%87-1d1d1f?style=flat-square">
  <img alt="本機辨識" src="https://img.shields.io/badge/%E8%81%B2%E9%9F%B3-%E4%B8%8D%E4%B8%8A%E5%82%B3-34c759?style=flat-square">
  <img alt="MIT" src="https://img.shields.io/badge/%E6%8E%88%E6%AC%8A-MIT-8e8e93?style=flat-square">
</p>

<br>

https://github.com/user-attachments/assets/d8b08c03-3a8e-45ee-86c3-d20c30ad6946

<br>

按住右 Option 說話，放開，字就出現在游標上。默契專為**中英夾雜**的說話方式設計，辨識全程在你的 Mac 上完成；要不要再交給 AI 整理，由你決定。

<br>

<img src="docs/media/feature-mixed.png" width="100%" alt="中英夾雜，一聽就懂：「呃、嗯、那個」被去掉，PR、description、breaking changes 照原樣留下">

**英文照原樣留下，贅字自動拿掉。** 「幫我把這個 PR 的 description 改短一點」不會被翻成中文，也不會整句變成英文。辨識模型是用 30 段真人中英夾雜錄音、比較 8 個本機模型後選出來的。

<br>

<img src="docs/media/feature-context.png" width="100%" alt="它知道你在哪裡打字：聊天、對 AI 下指令、筆記，同一句話三種格式">

**看場合整理。** 在 LINE 像聊天，句尾不加句號、不會誤送出；在 Claude Code 和終端機，指令和檔名一字不漏；在備忘錄，口頭列點變成清單。判斷在你的電腦上完成，送出的只有場合類別。

<br>

<img src="docs/media/feature-speed.png" width="100%" alt="放開之後 1.80 秒；不邊說邊轉的話要等 4.3 秒">

**邊說，邊轉。** 你停頓的時候，默契就先辨識前面那段。說 30 秒以上，放開後中位數 1.8 秒出字（含 AI 整理，實際使用的紀錄）。

<br>

<img src="docs/media/feature-privacy.png" width="100%" alt="辨識，全程在本機；隱私卡片顯示上傳的聲音 0 秒">

**聲音不離開你的電腦。** 沒有帳號、沒有追蹤。有新版本時會先問你，不會自己安裝，也可以關掉檢查。API key 存在 macOS 鑰匙圈。選「原話」時，連文字都不會送出。

<br>

<img src="docs/media/feature-history.png" width="100%" alt="每一次送出了什麼，都查得到：歷史紀錄列出送出與沒送出的資料">

**每一次送出了什麼，都查得到。** 歷史紀錄保留辨識原文、錄音和「這次送出了什麼」。貼錯地方可以一鍵重貼；說到一半換了視窗，默契會改成複製，不會貼錯。

<br>

## 開始使用

1. [下載](https://github.com/DDChen666/moqi-voice-typing/releases/latest) `Moqi_1.0.0_aarch64.dmg`，把「默契」拖進「應用程式」。
2. 第一次打開會被 macOS 擋下：默契是免費開源軟體，沒有付費取得 Apple 簽章。到「系統設定」→「隱私權與安全性」，按「**強制打開**」。
3. 跟著畫面完成三步：允許權限、選「原話」或「整理」、試說一句。

需要 M1 以後的 Mac、macOS 15 以後。遇到「已損毀」等狀況，見 [安裝教學](docs/安裝教學-Mac.md)。

|                  | 做什麼                                     | 需要                                |
| ---------------- | ------------------------------------------ | ----------------------------------- |
| **原話**         | 只加標點、轉繁體、套用字典                 | 什麼都不用，全程本機                |
| **整理**（推薦） | 去贅字、改口只留最後的說法、口頭列點變清單 | 潤稿服務的 API key（預設 DeepSeek） |
| **潤飾**         | 重組句子，讀起來像寫的                     | 同上                                |

按住右 ⌥ Option 說話；快按兩下進入免持；Esc 取消。更多操作見 [使用指南](docs/使用指南.md)。

## 接下來

- [ ] **自動學詞**：你改過一次的字，默契下次就寫對。辨識端的字典已在 `v1.1` 分支完成，英文詞寫對從 71% 提升到 87%。
- [ ] **Windows 版**
- [ ] **本機整理**：用小型語言模型在電腦上整理，連文字都不送出。
- [ ] **學你的打字習慣**：空格還是換行、標點習慣，預設關閉。

## 參與

歡迎回報問題、提供「你說的 vs 默契寫的」例子，或送出修改。請先看 [參與指南](CONTRIBUTING.md)。開發環境與編譯見 [BUILD.md](BUILD.md)，架構見 [docs/架構.md](docs/架構.md)。

## 致謝

默契建立在這些開源專案之上：[Handy](https://github.com/cjpais/Handy)（快捷鍵、懸浮窗、錄音與辨識的基礎，見 [FORK.md](FORK.md)）、[Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR)、[transcribe.cpp](https://github.com/handy-computer/transcribe.cpp)、[Silero VAD](https://github.com/snakers4/silero-vad)、[OpenCC](https://github.com/BYVoid/OpenCC)、[Tauri](https://tauri.app/)。

<br>

<p align="center">
  <img src="yuyin/brand/app-icon-1024.png" width="64" alt=""><br>
  <sub>默契 Moqi · <a href="LICENSE">MIT 授權</a></sub>
</p>
