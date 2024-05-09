use leetcode_daily::{respond, schedule_daily_reset, setup, SharedState, State};
use serenity::{async_trait, model::prelude::*, prelude::*};
use std::{collections::HashMap, env::var, error::Error, fs::OpenOptions, io::Read, sync::Arc};
use tokio::{main, spawn};

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        if let Err(why) = setup(&ctx, ready).await {
            println!("Error setting up {why:?}");
        }
        spawn(async move {
            if let Err(why) = schedule_daily_reset(ctx).await {
                println!("Error scheduling {why:?}");
            }
        });
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if let Err(why) = respond(ctx, msg).await {
            println!("Error responding to messages {why:?}");
        }
    }
}

#[main]
async fn main() -> Result<(), Box<dyn Error>> {
    let token = var("DISCORD_TOKEN")?;
    let mut client = Client::builder(
        token,
        GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_MEMBERS,
    )
    .event_handler(Handler)
    .await?;

    {
        let mut database = OpenOptions::new()
            .read(true)
            .write(true)
            .open("user_data.json")?;
        let mut contents = String::new();
        database.read_to_string(&mut contents)?;
        let mut data = client.data.write().await;
        data.insert::<State>(SharedState {
            guild: Arc::new(Mutex::new(HashMap::new())),
            database,
            user_data: serde_json::from_str(&contents)?,
        });
    }

    client.start().await?;
    Ok(())
}
