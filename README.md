# Nishikanto (নিশিকান্তর) — AI Writing Assistant

Nishikanto is an ultra-lightweight, high-performance desktop writing assistant engineered with **Tauri v2 (2.12+)**, **React 19**, **TypeScript**, and **Tailwind CSS**. Optimized for ultra-low RAM usage (<55MB working set) and continuous 60fps smoothness.

---

## Architecture Overview

### 1. Dual-Window Desktop Architecture
Configured natively in `src-tauri/tauri.conf.json`:
- **`main` Window** (`1100x750`):
  - Standard desktop window titled *"Nishikanto — AI Writing Assistant"*.
  - Full document editor and assistant settings dashboard.
  - Intercepts close requests to hide to the system tray (`hide_main_window`) instead of killing the background daemon.
- **`widget` Window** (`340x440` max, dynamic height):
  - Transparent, frameless (`decorations: false`), always-on-top floating overlay window titled *"Nishikanto Widget"*.
  - Configured with `focusable: false` and `skipTaskbar: true`.
  - **Non-Focusable Design**: Never steals keyboard input from external apps (Microsoft Word, VS Code, Google Docs, Slack, etc.).

### 2. Two-Tier Writing Engine

#### Tier 1 — Instant Offline Engine (Rust + Harper Core)
- Powered by `harper-core 3.0` with the `concurrent` feature enabled for async/Tokio safety.
- Exposes Tauri command `check_text_local(text: String)` executing in **<5ms** without network calls.
- Categorizes issues with structured JSON:
  - 🔴 **Correctness**: Spelling, Typos, Subject-Verb Agreement, Grammar, Punctuation, Articles.
  - 🔵 **Clarity**: Passive voice (`was written by`, etc.), Redundancies (`basic fundamentals`), Wordiness (`in regards to`).
  - 🟣 **Style**: Initialisms (`ASAP`), formatting, word choice, clichéd phrases.
- Returns exact character start/end offsets, flagged substrings, explanation messages, and one-click replacement suggestions.

#### Tier 2 — Free-Tier Gemini AI Layer (Rust + Reqwest)
- Exposes Tauri command `analyze_with_gemini(text: String, mode: String)`.
- Calls the Gemini REST API with model `gemini-3.5-flash-lite` (with intelligent fallback to `gemini-2.5-flash-lite` and `gemini-2.0-flash`).
- Reads `GEMINI_API_KEY` from `.env`.
- Supported Modes:
  - `"tone_and_score"`: Real-time writing score (0-100) and nuanced tone badge with emojis.
  - `"fix_all"`: Full proofreading and error elimination.
  - `"make_formal"`: Executive, authoritative, polished business tone.
  - `"make_casual"`: Approachable, friendly, modern conversational style.
  - `"make_concise"`: Strips fluff and passive constructions for high impact.
- **Resilience**: Gracefully handles HTTP `429` rate limits and missing keys without crashing.

### 3. Native System Tray Integration
- Tooltip: `"Nishikanto"`.
- Context Menu:
  - **"Open Nishikanto Editor"**: Shows and focuses the `main` window.
  - **"Pause Nishikanto" / "Resume Nishikanto"**: Atomic checkable toggle synced with Rust state, tray labels, and frontend event emitters.
  - **"Quit"**: Cleanly shuts down the app.
- Left-click on tray icon immediately summons the main editor.

---

## Getting Started

### Prerequisites
- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (v1.85+)
- C++ / MinGW-w64 toolchain

### Installation
```bash
# Clone the repository
git clone <repo-url>
cd Nishikanto

# Install npm dependencies
npm install

# Configure Gemini API key (optional for Tier 2 cloud features)
cp .env.example .env
# Edit .env and insert your GEMINI_API_KEY
```

### Development
```bash
# Run in desktop development mode (launches both windows & system tray)
npm run tauri dev

# Run Vite web preview in browser
npm run dev
```

### Verification & Testing
```bash
# Run Rust compiler verification
cargo check

# Run Rust engine unit tests (verifies Harper Core on test paragraphs)
cargo test test_harper_local_lints_on_sample_paragraph -- --nocapture

# Build frontend production bundle
npm run build
```

---

## License
MIT License. Crafted for high-performance writing productivity.
