# tg-rs

[![Crates.io](https://img.shields.io/crates/v/tg-cli)](https://crates.io/crates/tg-cli)
[![license](https://img.shields.io/crates/l/tg-cli)](https://github.com/Garfield1002/tg-rs/blob/HEAD/LICENSE.md)


> ⚠️ This is a potentially dangerous vibe coded project. Use at your own risk.

Send yourself Telegram messages from the command line.

```sh
tg "Hello, World!"
echo "Hello, World!" | tg
```

## Install

Requires Rust. Clone the repo and build:

```sh
git clone https://github.com/Garfield1002/tg-rs
cd tg-rs
cargo install --path .
```

## Setup

Run the interactive setup once.
You will need a Telegram bot token, you can create one via [@BotFather](https://t.me/BotFather).

```sh
tg setup
```

## Usage

```
tg [OPTIONS] [MESSAGE...]
tg <SUBCOMMAND>
```

### Send a message

```sh
# Positional args (joined with spaces)
tg hello world
tg "I'm a fun message"

# From stdin
echo "hello" | tg
cat LICENSE.md | tg
tg < /etc/hostname
```

If no message args are given, `tg` reads from stdin until EOF.

### Options

| Flag      | Long                              | Description                                           |
| --------- | --------------------------------- | ----------------------------------------------------- |
| `-e`      |                                   | Interpret escape sequences (`\n`, `\t`, `\\`)         |
| `-n`      |                                   | Strip trailing newline from input                     |
| `-m MODE` | `--parse-mode MODE`               | Telegram formatting: `markdown` or `html`             |
| `-i`      | `--interactive`                   | Stream stdin updates by editing one message (max 1/s) |
| `-f`      | `--interactive-frequency SECONDS` | Seconds between interactive updates (default: `1`)    |
| `-s`      | `--silent`                        | Send without device notification                      |
| `-q`      | `--quiet`                         | Suppress non-error output                             |
| `-h`      | `--help`                          | Show help                                             |
| `-V`      | `--version`                       | Show version                                          |

### Escape sequences (`-e`)

```sh
tg -e "line one\nline two"
tg -e "col1\tcol2"
```

### Telegram formatting (`-m`)

```sh
# MarkdownV2
tg -m markdown "*bold* _italic_ \`code\`"

# HTML
tg -m html "<b>bold</b> <i>italic</i> <code>code</code>"
```

> Note: MarkdownV2 requires special characters to be escaped. See the
> [Telegram docs](https://core.telegram.org/bots/api#markdownv2-style) for details.
> `tg` automatically escapes these characters for you

### Silent notifications (`-s`)

Sends the message without triggering a sound or notification banner on the recipient's device. Useful for low-priority automated alerts:

```sh
tg -s "nightly backup finished"
```

### Combining flags

```sh
# Multiline silent message
tg -e -s "backup complete\nfiles: 1,042\nsize: 4.2 GB"

# Piped log with no notification
cat /etc/hostname | tg -s

# HTML alert
tg -m html "<b>ERROR</b>: service crashed"
```

### Interactive updates (`-i`)

Use interactive mode for progress-like output. The first update sends a message, then further updates edit that same message with a max rate of 1 update per second.

Use `--interactive-frequency` (`-f`) to change the update interval.

```sh
# Example progress stream
some-command-that-prints-progress | tg -i

# Update every 2 seconds
some-command-that-prints-progress | tg -i --interactive-frequency 2

# ASCII progress bar stream (bash)
for i in $(seq 0 20); do
    filled=$(printf '%*s' "$i" '' | tr ' ' '#');
    empty=$(printf '%*s' "$((20 - i))" '' | tr ' ' '-');
    printf '\r`[%s%s] %3d%%`' "$filled" "$empty" "$((i * 5))";
    sleep 0.5;
done | tg -i
```

### Attach files (`tg attach`)

Send one or more files as Telegram document attachments. Shell globbing works as expected.

```sh
# Single file
tg attach report.pdf

# Multiple files
tg attach foo.txt bar.c

# Glob pattern
tg attach *.log

# Silent attachment
tg attach -s backup.tar.gz
```

### Listen mode (`tg listen`)

Listen for new incoming messages in the configured chat and write them to stdout. Listening stops when `/eof` is received.

```sh
tg listen > out.txt
```

## Subcommands

### `tg setup`

Run the interactive setup to configure your chat ID. Only needed once.

```sh
tg setup
```

### `tg listen`

Listen for new incoming messages in the configured chat and write them to stdout until `/eof` is received.

```sh
tg listen > out.txt
```

### `tg attach`

Send files as document attachments. Accepts one or more file paths; shell globbing is handled by the shell before `tg` sees the arguments.

```sh
tg attach *.png
tg attach -s large-file.zip   # silent
tg attach -q data.csv         # quiet (no "Sent:" output)
```

### `tg config show`

Print the config file path and its current contents:

```sh
tg config show
# Config path: /home/user/.config/tg/config.toml
# chat_id = 123456789
```

### `tg config reset`

Delete the config file so you can run setup again:

```sh
tg config reset
tg setup
```

## Configuration

Config is stored at `~/.config/tg/config.toml`:

```toml
token = "123456:ABC-your-token-here"
chat_id = 123456789
```

Both values are written during `tg setup`. To change either, run `tg config reset` and go through setup again.

## Examples

```sh
# Send a quick note
tg "Hello from the terminal"

# Results from a script
ls | tg

# Multiline message
tg -e "Line 1\nLine 2\nLine 3"

# Bold alert via HTML
tg -m html "<b>ALERT</b>"
```

## Library Macro

The crate exports a `telegram!` macro for quick fire-and-forget sends in Rust code:

```rust
use tg_cli::telegram;

#[tokio::main]
async fn main() {
    telegram!("build {} complete", 42);
}
```

Notes:

- `telegram!` uses Markdown parse mode and non-silent notifications.
- With default features, `telegram!` is non-blocking and does not need `.await`.
- Non-blocking behavior is controlled by the `non-blocking` feature.
- If `non-blocking` is disabled, `telegram!` uses a blocking send path.
- Send failures are printed to stderr.
- For explicit error handling or custom parse/silent options, use `send_tg_message(...)` directly.

### Blocking API

If you want a synchronous API, call `send_tg_message_blocking(...)`:

```rust
use tg_cli::{ParseMode, send_tg_message_blocking};

fn main() {
    if let Err(err) = send_tg_message_blocking("hello".to_string(), ParseMode::Markdown, false) {
        eprintln!("{err}");
    }
}
```

### Feature Flags

```toml
[dependencies]
tg = { version = "0.1", default-features = false }
```

- Enable `non-blocking` to make `telegram!` use `tokio::spawn`.
- Disable `non-blocking` to make `telegram!` use blocking sends.

# License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software, either in source code form or as a compiled
binary, for any purpose, commercial or non-commercial, and by any
means.

In jurisdictions that recognize copyright laws, the author or authors
of this software dedicate any and all copyright interest in the
software to the public domain. We make this dedication for the benefit
of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of
relinquishment in perpetuity of all present and future rights to this
software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <https://unlicense.org/>
