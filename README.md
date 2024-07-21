# LeetCode Daily

## Introduction

Hi I'm LeetCode Daily, here to motivate you to do LeetCode questions every single day 🤓

I operate on a default channel and I create a thread in that channel when a new daily question comes out

You can change it by running the following command

```discord
/channel channel_id
```

Some other commands you can run are

* `/help`: Shows this help message
* `/reset`: Reset the database for this discord server
* `/random [free | paid | easy | medium | hard] ...`: Send a random question with optional fields to filter by difficulty or whether it is subscription only, if not run in a thread it will create a thread for it
* `/scores`: Shows the current leaderboard
* `/top [number]`: Shows the top 3 or any number up to 10 scores and monthly records across all servers
* `/poll`: Start a poll for today's submissions or reply to an existing one if it has already started, has to be run in the current daily thread
* `/daily`: Resend the daily remainder
* `/active [weekly|daily] [toggle]`: Check whether some features of the bot are currently active or toggle them on and off

To share your code you have to put it a spoiler tag and wrap it with \```code\``` so others can't immediately see your solution. You can start from the template below and replace the language and code with your own. If you didn't follow the format strictly simply send it again

```discord
||```language
code
```||
```

## Setup Instructions

Add the discord bot to your server via this [invite link](https://discord.com/oauth2/authorize?client_id=1235892312463245322&permissions=8&scope=bot).

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

To update the code and run it, use the [run.sh](scripts/run.sh) script which fetches the latest commit and runs the [restart.sh](scripts/restart.sh) script to start up a new process and `kill` the existing one

```bash
sh scripts/run.sh
```
