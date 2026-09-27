# wemi-coder

A small terminal coding assistant written in Rust. It opens a project directory,
shows a simple TUI, and lets a model inspect files, search text, propose edits,
and run shell commands with confirmation for risky actions.

## Usage

Create API settings at your platform config directory:

- Linux: `~/.config/wemi-coder/api/settings.json`
- macOS: `~/Library/Application Support/wemi-coder/api/settings.json`
- Windows: `%APPDATA%/wemi-coder/api/settings.json`

Example:

```json
{
  "api_key": "YOUR_API_KEY",
  "base_url": "https://api.openai.com/v1",
  "model": "gpt-4o-mini"
}
```

Run the app from a project directory:

```sh
cargo run -- .
```

Inside the TUI, type a request and press Enter. Use `/open <path>` to switch
projects, arrow keys to scroll the conversation, and `Ctrl-C` or `Esc` to quit.
