# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**tg-rs** (published as `tg-cli`) is a Unix-like CLI tool for sending Telegram messages from the terminal. It is a standard Cargo package exposing both a binary and a library crate.

## Commands

```bash
cargo build --release       # Build
cargo test --lib            # Unit tests only (integration tests require a real Telegram bot)
cargo clippy                # Lint
cargo install --path .      # Install locally
```

## Architecture

The project is split into a CLI binary and a reusable library so that other Rust projects can embed Telegram notifications via the `telegram!` macro. Feature flags in `Cargo.toml` gate CLI-only dependencies and control whether the macro blocks or spawns asynchronously.

**Key design tension**: the Secret Service API (used for secure token storage) is blocking, while the rest of the codebase is async (tokio). This requires care at the boundary — read `secret_store.rs` before touching anything that loads or saves the bot token.

**Config** lives at `~/.config/tg/config.toml`. The bot token is stored preferentially in the OS Secret Service (GNOME Keyring / KWallet) and falls back to plaintext in the config file with a warning.

**Setup flow** is interactive: the user provides a bot token, the app listens for a Telegram `/start` message, exchanges a pairing code, then persists the token and chat ID.
