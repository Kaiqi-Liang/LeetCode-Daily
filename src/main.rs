use chrono::Utc;
use leetcode_daily::{
    initialise_guild, respond, save_to_database, schedule_daily_question, schedule_weekly_contest,
    setup, vote, SharedState, State,
};
use serenity::{async_trait, model::prelude::*, prelude::*};
use std::{collections::HashMap, env::var, error::Error, fs::OpenOptions, io::Read};
use tokio::{main, spawn};

struct Handler;
#[async_trait]
impl EventHandler for Handler {
    async fn guild_create(&self, ctx: Context, guild: Guild, _is_new: Option<bool>) {
        let bot = ctx.cache.current_user().id;
        if _is_new == Some(true) {
            if let Err(why) = initialise_guild(&ctx, guild, bot).await {
                println!("[{}] Error initialising guild {why}", Utc::now());
            }
            if let Err(why) = save_to_database(ctx).await {
                println!("[{}] Error saving to database {why}", Utc::now());
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        if let Err(why) = setup(&ctx, ready).await {
            println!("[{}] Error setting up {why}", Utc::now());
        }
        {
            let ctx = ctx.clone();
            spawn(async move {
                if let Err(why) = schedule_daily_question(&ctx).await {
                    println!("[{}] Error scheduling {why}", Utc::now());
                }
            });
        }
        spawn(async move {
            if let Err(why) = schedule_weekly_contest(&ctx).await {
                println!("[{}] Error scheduling {why}", Utc::now());
            }
        });
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let bot = ctx.cache.current_user().id;
        if msg.author.id != bot {
            if let Err(why) = respond(&ctx, msg, bot).await {
                println!("[{}] Error responding to messages {why}", Utc::now());
            }
            if let Err(why) = save_to_database(ctx).await {
                println!("[{}] Error saving to database {why}", Utc::now());
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(why) = vote(&ctx, interaction).await {
            println!(
                "[{}] Error responding to vote interaction {why}",
                Utc::now(),
            );
        }
        if let Err(why) = save_to_database(ctx).await {
            println!("[{}] Error saving to database {why}", Utc::now());
        }
    }
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
            .open("database.json")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut data = client.data.write().await;
        data.insert::<State>(SharedState {
            guilds: HashMap::new(),
            file,
            database: serde_json::from_str(&contents)?,
        });
    }
    client.start().await.map_err(|e| e.into())
}
