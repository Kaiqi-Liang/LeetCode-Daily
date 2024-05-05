# LeetCode Daily

## Setup Instructions

Add the discord bot to your server by navigating to this [URL](https://discord.com/oauth2/authorize?client_id=1235892312463245322&permissions=8&scope=bot).

## Running Locally

Create an app in the developer [portal](https://discord.com/developers/applications?new_application=true).

On the **Bot** page under **TOKEN**, click "Reset Token" to generate a new bot token then copy it to a environment variable

```bash
export DISCORD_TOKEN=discord_token
```

The minimum supported Rust version (MSRV) is Rust 1.74, check `cargo` is installed and meet the version requirement

```bash
cargo --version
```

If not the latest stable version of Rust can be installed via `rustup`

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

If a previous version of Rust installed via `rustup` already exists, it can be updated by simply running

```bash
rustup update
```

Run the bot in debug mode

```bash
cargo r
```

Or compile it with optimisation and run in release mode

```bash
cargo b --release
target/release/leetcode_daily
```
