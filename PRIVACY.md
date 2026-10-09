# Privacy and credential handling — Outcasts Lorekeel

[日本語](PRIVACY_jp.md)

This document describes what Outcasts Lorekeel stores on your computer, what it sends over the network, and how to remove it. It applies to **version 0.7.0 and later**. Differences in earlier versions are noted where they matter.

Lorekeel is a desktop application. It has no user accounts and no server that stores your games. You bring your own API key for the AI service you choose, and the application talks to that service directly from your computer.

## Summary

- **Lorekeel does not collect telemetry, analytics, crash reports, or usage data.** It contains no analytics or crash-reporting SDK.
- **API keys are stored in your operating system's credential store**, not in plain-text files.
- **Game content is sent to the AI provider you configure**, and only to it (plus the optional features listed below, each of which you turn on yourself). That provider's privacy policy and data-retention terms apply.
- **Uninstalling does not delete your data or your saved keys.** See [Deleting your data](#deleting-your-data).

## API keys

### Where they are stored

When you enter an API key in **Settings**, Lorekeel saves it in the operating system's credential store:

| OS | Credential store |
|---|---|
| Windows | Credential Manager (Windows Credentials → Generic Credentials) |
| macOS | Keychain |
| Linux | Secret Service (for example GNOME Keyring or KWallet) |

All entries use the service name `jp.lorekeel.app`. On Windows they appear as `<name>.jp.lorekeel.app`, for example `LLM_API_KEY.jp.lorekeel.app`. The keys stored this way are:

- the key for the AI model used by the game master (`LLM_API_KEY`)
- the optional keys for the synopsis model and the AI editing model (`SUMMARY_LLM_API_KEY`, `EDITOR_LLM_API_KEY`)
- the optional image-generation keys for OpenAI, Google Gemini, Meta and xAI (`IMAGE_API_KEY_OPENAI`, `IMAGE_API_KEY_GEMINI`, `IMAGE_API_KEY_META`, `IMAGE_API_KEY_XAI`)
- the optional Jev consistency-check token (`JEV_API_TOKEN`)
- one key per model you register in **Settings → AI Model** (`profile:<id>`)

Non-secret settings — endpoint URLs, model names, the Jev account ID, and similar options — are stored in a `.env` file in the application's data folder.

### How they are protected

- On disk, keys are protected by your operating system's credential store and your user account.
- While Lorekeel is running, the keys it needs are held in the application's memory so it can make requests.
- A key is sent only to the endpoint you configured for it (for example `https://api.anthropic.com/v1`). Lorekeel never sends your API keys to Outcasts servers. In multiplayer, only the host's application calls the AI provider; guests never receive the host's keys.
- If the credential store is unavailable (for example, on a Linux system without a Secret Service), Lorekeel falls back to saving the key in plain text in the `.env` file and says so in **Settings → AI Model**.

**Versions before 0.7.0** stored API keys in plain text in the `.env` file and in the application's settings (`settings.json`). When you first start 0.7.0 or later, Lorekeel moves those keys into the credential store and removes them from those files, but only after it has read each key back and confirmed it was stored correctly.

## What is sent over the network

### To the AI provider you configure

To play, Lorekeel sends a request to the AI service whose endpoint and key you entered. Each request can contain:

- the content of the scenario package you are playing (world description, characters, locations, rules)
- the current game state (location, inventory, flags, stats)
- what you type as your action
- recent narration, a summary of earlier events, and any facts you have added

The optional **synopsis model** receives the log of earlier turns so it can summarize them. The optional **AI editing** feature sends the package file you are editing, along with other files in the same package if the model reads them.

These requests go directly from your computer to that provider. **The provider's own privacy policy and data-retention terms apply**, including whether it logs requests or uses them for training. Check them before you choose a provider.

### Optional features (off unless you turn them on)

| Feature | Sent to | What is sent |
|---|---|---|
| Image generation (off by default) | OpenAI, Google Gemini, Meta, xAI, or a ComfyUI server you specify | A text prompt describing the scene (built from the recent narration, the location, and the characters present), plus up to three reference images from the package or that you added |
| Jev consistency check (developer mode only, needs its own account and token) | Cloudflare Workers AI (TypeSafe Jev) | That turn's narration, the preceding narration, character profiles, inventories, and similar game data |
| Text-to-speech (off by default) | Your operating system's voices, or an engine running on your own computer | The narration text. VOICEVOX, AivisSpeech, and OpenAI-compatible engines can only be reached at `localhost`. The built-in engine uses the voices your operating system or web engine provides; if you choose a voice that is delivered as an online service, its provider processes the text. |
| Price table import (only when you press the button) | `betyourluck.github.io` | A request for the price table. Nothing about you or your game is sent. |

### To Outcasts servers

Lorekeel contacts servers operated by Outcasts for these purposes only:

| When | Server | What is sent |
|---|---|---|
| At startup | `lorekeel.outcasts.jp` | A request for the latest version number. Your version is compared on your computer; it is not sent. |
| When you open the package archive, download a package, or open the local package list | `lorekeel.outcasts.jp` | Requests for package listings, package files, and update checks for packages you downloaded from the archive (the package IDs). |
| When you host or join a multiplayer table | `knock.outcasts.jp` | Signaling: the room code and the WebRTC connection details (including IP addresses) needed to connect players. |
| When players cannot connect directly | `turn.outcasts.jp` | Relayed connection traffic. It is encrypted end to end (DTLS/SRTP), so the relay cannot read game data or voice. |
| When you host a table | `lorekeel.outcasts.jp` | The host's scenario package, uploaded temporarily so guests can download it. It is not listed or published, it is deleted when the game starts, and any copy left behind expires automatically within a few hours. |

These requests contain no account information, API keys, or game narration. Like any network request, they reveal your IP address to the server. Game data and voice chat between players travel directly between the players' computers whenever possible, encrypted by WebRTC. Voice chat is off until you turn on your microphone.

You can point Lorekeel at other servers, for example ones you run yourself: the archive address (also used for the update check) in the package list, and the knock server address in the multiplayer dialog.

## What is stored on your computer

| What | Where |
|---|---|
| API keys | The OS credential store (see above) |
| Settings, saves, downloaded packages, reference images, generated images, logs | The application data folder (below) |
| Interface settings used by the web view | The web view's storage folder (below) |

Application data folder:

| OS | Application data | Web view storage |
|---|---|---|
| Windows | `%APPDATA%\jp.lorekeel.app` | `%LOCALAPPDATA%\jp.lorekeel.app` |
| macOS | `~/Library/Application Support/jp.lorekeel.app` | `~/Library/WebKit/jp.lorekeel.app` |
| Linux | `~/.local/share/jp.lorekeel.app` (typical) | depends on the web engine (not verified) |

Inside the application data folder:

- `saves/` — autosaves and save slots, including the full narration of your games
- `packages/` — packages downloaded from the archive or received at a multiplayer table
- `refs/`, `images/` — reference images and saved illustrations (the image folder can be changed in Settings)
- `logs/usage.jsonl` — token counts per request, for the usage meter. **No narration or prompt text.**
- `logs/consistency.jsonl` — Jev check scores (developer mode only). **No narration text.**
- `.env` — non-secret settings (endpoints, model names, options)
- `settings.json` — a backup copy of your interface settings

Conversation logs are written to a text file only when you press the save-log button, to the folder you choose in Settings.

If you used Lorekeel when it was still named **Kataribe**, a folder named `jp.kataribe.app` may also exist in the same locations. It holds a copy of your data from before the rename, and its `.env` file may contain API keys in plain text. Lorekeel does not delete it automatically; you can delete it yourself.

## Deleting your data

**Uninstalling Lorekeel does not delete your data or your saved API keys.** To remove them:

1. **API keys**: open **Settings → AI Model** and press **Delete all saved API keys**. This removes every key Lorekeel saved, including the image-generation and Jev keys and the keys of all registered models. You can also delete the entries yourself in the OS credential store: look for entries containing `jp.lorekeel.app`.
2. **Everything else**: delete the application data folder and the web view storage folder listed above (and `jp.kataribe.app`, if present). Close Lorekeel first.
3. **Data held by AI providers**: Lorekeel cannot delete what a provider has already received. Use the provider's own tools or contact the provider.

## Contact

Questions or problems: [GitHub Issues](https://github.com/betyourluck/Lorekeel/issues).
