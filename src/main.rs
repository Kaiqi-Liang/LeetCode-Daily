use chrono::{TimeZone, Utc};
use serenity::{
    async_trait,
    model::prelude::*,
    prelude::*,
    utils::{EmbedMessageBuilding, MessageBuilder},
    Error,
};
use std::{collections::HashMap, env::var, sync::Arc, time::Duration as StdDuration};
use tokio::{main, spawn, time};

type SharedMemberMap = Arc<Mutex<HashMap<GuildId, HashMap<UserId, (User, bool)>>>>;
struct MemberList;
impl TypeMapKey for MemberList {
    type Value = SharedMemberMap;
}

const LEETCODE_CHANNEL_ID: u64 = 1235529498770935840;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        {
            if let Some(member_map) = ctx.data.write().await.get::<MemberList>() {
                for guild in ready.guilds {
                    if let Ok(members) = guild.id.members(&ctx.http, None, None).await {
                        member_map.lock().await.insert(
                            guild.id,
                            members
                                .into_iter()
                                .filter_map(|member| {
                                    let user = member.user;
                                    if user.bot {
                                        None
                                    } else {
                                        Some((user.id, (user, false)))
                                    }
                                })
                                .collect::<HashMap<UserId, (User, bool)>>(),
                        );
                    }
                }
            }
        }
        schedule_daily_reset(ctx).await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content.contains("||") && msg.content.contains("```") {
            let guild_id = &msg
                .guild_id
                .expect("This message was not received over the gateway");
            if let Some(member_map) = ctx.data.write().await.get_mut::<MemberList>() {
                let mut member_map = member_map.lock().await;
                let mut message = MessageBuilder::new();
                if let Some(user_ids) = member_map.get_mut(guild_id) {
                    if let Some((user, completed)) = user_ids.get_mut(&msg.author.id) {
                        message
                            .push("Congrats to ")
                            .mention(user)
                            .push(" for completing today's challenge\n");
                        *completed = true;
                    }
                }
                if let Some(user_ids) = member_map.get(guild_id) {
                    let users_not_yet_completed = user_ids
                        .values()
                        .filter_map(
                            |(user, completed)| {
                                if *completed {
                                    None
                                } else {
                                    Some(user)
                                }
                            },
                        )
                        .collect::<Vec<_>>();
                    if users_not_yet_completed.is_empty() {
                        message
                            .push("Everyone has finished today's challenge, let's Grow Together!");
                    } else {
                        message.push("Still waiting for ");
                        for user in users_not_yet_completed {
                            message.mention(user);
                        }
                    }
                    if let Err(why) = ChannelId::new(LEETCODE_CHANNEL_ID)
                        .say(ctx.clone().http, message.build())
                        .await
                    {
                        println!("Error sending message: {why:?}");
                    }
                }
            }
        }
    }
}

async fn schedule_daily_reset(ctx: Context) {
    spawn(async move {
        loop {
            let num_seconds_until_utc_midnight: u64 = Utc
                .from_utc_datetime(
                    &Utc::now()
                        .naive_utc()
                        .date()
                        .succ_opt()
                        .expect("Failed to get the next date")
                        .and_hms_opt(0, 0, 0)
                        .expect("Failed to get midnight time"),
                )
                .signed_duration_since(Utc::now())
                .num_seconds()
                .try_into()
                .expect("Next midnight UTC is in the past");
            // Add 1 to make sure by the time the loop tries to schedule it again it will be the next day
            time::sleep(StdDuration::from_secs(num_seconds_until_utc_midnight + 1)).await;

            if let Err(why) = ChannelId::new(LEETCODE_CHANNEL_ID)
                .say(ctx.clone().http, MessageBuilder::new()
                    .push("Share your code in the format below to confirm your completion of today's ")
                    .push_named_link("LeetCode", "https://leetcode.com/problemset\n")
                    .push(" Daily @everyone\n")
                    .push_safe("||```code```||")
                    .build())
                .await
            {
                println!("Error sending message: {why:?}");
            }
            // Reset everyone's completion's status when it's midnight UTC
            if let Some(member_map) = ctx.data.write().await.get_mut::<MemberList>() {
                let mut member_map = member_map.lock().await;
                for guild_map in member_map.values_mut() {
                    for (_, completed) in guild_map.values_mut() {
                        *completed = false;
                    }
                }
            }
        }
    });
}

#[main]
async fn main() -> Result<(), Error> {
    let token = var("DISCORD_TOKEN").expect("Expected a discord token in the environment");
    let mut client = Client::builder(
        token,
        GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_MEMBERS,
    )
    .event_handler(Handler)
    .await
    .expect("Err creating client");

    {
        let mut data = client.data.write().await;
        data.insert::<MemberList>(Arc::new(Mutex::new(HashMap::new())));
    }

    client.start().await?;
    Ok(())
}
