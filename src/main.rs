use chrono::Utc;
use leetcode_daily::{
    initialise_guild, log, respond, save_to_database, schedule_daily_question, schedule_thread,
    schedule_weekly_contest, setup, vote, write_to_database, Database, SharedState, State,
};
use serenity::{async_trait, model::prelude::*, prelude::*};
use std::{
    collections::HashMap,
    env::var,
    error::Error,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
};
use tokio::{main, spawn};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        let bot = ctx.cache.current_user().id;
        if _is_new == Some(true) {
            if let Err(why) = initialise_guild(&ctx, guild, bot).await {
                log!("Error initialising guild: {why}");
            }
            save_to_database!(ctx);
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        if let Ok(()) = setup(&ctx, ready).await {
            schedule_thread!(ctx, schedule_daily_question);
            schedule_thread!(ctx, schedule_weekly_contest);
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let bot = ctx.cache.current_user().id;
        if msg.author.id != bot {
            if let Err(why) = respond(&ctx, msg, bot).await {
                log!("Error responding to messages: {why}");
            }
            save_to_database!(ctx);
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(why) = vote(&ctx, interaction).await {
            log!("Error responding to vote interaction: {why}");
        }
        save_to_database!(ctx);
    }
}

fn default_database(file: File, contents: &mut str) -> Result<Database, Box<dyn Error>> {
    let mut state = SharedState {
        ready: false,
        guilds: HashMap::new(),
        file,
        database: HashMap::new(),
    };
    write_to_database!(state);
    serde_json::from_str(contents).map_err(|err| err.into())
}

#[main]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = var("DISCORD_TOKEN")?;
    let mut client = Client::builder(
        token,
        GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_MEMBERS,
    )
    .event_handler(Handler)
    .await?;
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("database.json")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut data = client.data.write().await;
        data.insert::<State>(SharedState {
            ready: false,
            guilds: HashMap::new(),
            file: file.try_clone()?,
            database: serde_json::from_str(&contents)
                .unwrap_or(default_database(file, &mut contents)?),
        });
    }
    client.start().await.map_err(|e| e.into())
}
