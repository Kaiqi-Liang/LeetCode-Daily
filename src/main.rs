use serenity::{
    all::{Message, Ready},
    async_trait,
    prelude::*,
};
use std::env::var;

const CHANNEL_ID: usize = 1235882148959883286;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        println!("{}", msg.content);
        if let Err(why) = msg.channel_id.say(ctx.http, format!("#{CHANNEL_ID}")).await {
            println!("Error sending message: {why:?}");
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    let token = var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let mut client = Client::builder(token, GatewayIntents::DIRECT_MESSAGES)
        .event_handler(Handler)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
