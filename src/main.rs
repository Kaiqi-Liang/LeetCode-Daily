use chrono::{TimeDelta, TimeZone, Utc};
use serde::Deserialize;
use serenity::{
    async_trait,
    model::prelude::*,
    prelude::*,
    utils::{EmbedMessageBuilding, MessageBuilder},
    Error,
};
use std::{
    collections::HashMap, env::var, fs::File, io::Read, sync::Arc, time::Duration as StdDuration,
};
use tokio::{main, spawn, time};

type Users = HashMap<UserId, UserStatus>;
type Guild = HashMap<GuildId, Users>;
type SharedMemberMap = Arc<Mutex<Guild>>;
type UserData = HashMap<GuildId, HashMap<UserId, Data>>;

#[derive(Deserialize)]
struct Data {
    completed: bool,
    score: usize,
}

struct UserStatus {
    user: User,
    completed: bool,
    score: usize,
}

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
        if let Some(member_map) = ctx.data.write().await.get::<MemberList>() {
            let guild_local = read_user_data();
            for guild in ready.guilds {
                let guild_id = guild.id;
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
                                    Some((
                                        user.id,
                                        UserStatus {
                                            user: user.clone(),
                                            completed: false,
                                            score: if let Some(user) = guild_local
                                                .get(&guild_id)
                                                .and_then(|users| users.get(&user.id))
                                            {
                                                user.score
                                            } else {
                                                0
                                            },
                                        },
                                    ))
                                }
                            })
                            .collect::<HashMap<UserId, UserStatus>>(),
                    );
                }
            }
        }
        schedule_daily_reset(ctx).await;
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if let Some(member_map) = ctx.data.write().await.get_mut::<MemberList>() {
            let mut member_map = member_map.lock().await;
            let guild_id = &msg
                .guild_id
                .expect("This message was not received over the gateway");
            if let Some(users) = member_map.get_mut(guild_id) {
                let mut message = MessageBuilder::new();
                if msg.content.contains("||") && msg.content.contains("```") {
                    if let Some(user) = users.get_mut(&msg.author.id) {
                        let mut guild_local = read_user_data();
                        if !user.completed {
                            user.completed = true;
                            let score: usize = (time_till_utc_midnight().num_hours() / 10 + 1)
                                .try_into()
                                .expect("Next midnight UTC is in the past");
                            user.score += score;
                            guild_local
                                .entry(*guild_id)
                                .and_modify(|guild| {
                                    guild.insert(
                                        msg.author.id,
                                        Data {
                                            completed: true,
                                            score: user.score,
                                        },
                                    );
                                })
                                .or_insert(HashMap::new());
                            message
                                .push("Congrats to ")
                                .mention(&user.user)
                                .push(format!(" for completing today's challenge! You have gained {score} points today your current score is {}\n", user.score));
                        }
                    }
                    let users_not_yet_completed = users
                        .values()
                        .filter_map(|user| {
                            if user.completed {
                                None
                            } else {
                                Some(&user.user)
                            }
                        })
                        .collect::<Vec<_>>();
                    if users_not_yet_completed.is_empty() {
                        construct_leader_board(
                            users,
                            message.push(
                                "Everyone has finished today's challenge, let's Grow Together!\n",
                            ),
                        );
                    } else {
                        message.push("Still waiting for ");
                        for user in users_not_yet_completed {
                            message.mention(user);
                        }
                    }
                    construct_leader_board(users, message.push("\n"));
                } else if msg.content == "/scores" {
                    construct_leader_board(users, &mut message);
                } else {
                    return;
                }
                if let Err(why) = ChannelId::new(LEETCODE_CHANNEL_ID)
                    .say(ctx.clone().http, message.build())
                    .await
                {
                    println!("Error sending reply message: {why:?}");
                }
            }
        }
    }
}

fn read_user_data() -> UserData {
    let mut user_status = File::open("user_data.json").expect("Failed to read user data");
    let mut contents = String::new();
    user_status
        .read_to_string(&mut contents)
        .expect("Data is not valid UTF-8");
    serde_json::from_str(&contents).expect("Malform data")
}

fn construct_leader_board<'a>(
    users: &Users,
    message: &'a mut MessageBuilder,
) -> &'a mut MessageBuilder {
    message.push("The current leaderboard:\n");
    let mut leaderboard = users
        .values()
        .map(|user| (&user.user, user.score))
        .collect::<Vec<_>>();
    leaderboard.sort_by(|a, b| b.1.cmp(&a.1));
    for (user, score) in leaderboard {
        message.push(format!("{}: {score}\n", user.name));
    }
    message
}

fn time_till_utc_midnight() -> TimeDelta {
    Utc.from_utc_datetime(
        &Utc::now()
            .naive_utc()
            .date()
            .succ_opt()
            .expect("Failed to get the next date")
            .and_hms_opt(0, 0, 0)
            .expect("Failed to get midnight time"),
    )
    .signed_duration_since(Utc::now())
}

async fn schedule_daily_reset(ctx: Context) {
    spawn(async move {
        loop {
            // Add 1 to make sure by the time the loop tries to schedule it again it will be the next day
            time::sleep(StdDuration::from_secs(
                (time_till_utc_midnight().num_seconds() + 1)
                    .try_into()
                    .expect("Next midnight UTC is in the past"),
            ))
            .await;

            if let Some(member_map) = ctx.data.write().await.get_mut::<MemberList>() {
                let mut member_map = member_map.lock().await;
                let mut guild_local = read_user_data();
                for (guild_id, users) in member_map.iter_mut() {
                    let mut message = MessageBuilder::new();
                    message.push("Yesterday ");
                    let mut penalties = false;
                    for (user_id, user) in users.iter_mut() {
                        if !user.completed {
                            penalties = true;
                            message.mention(&user.user);
                            if user.score > 0 {
                                user.score -= 1;
                            }
                        } else {
                            user.completed = false;
                        }
                        guild_local
                            .entry(*guild_id)
                            .and_modify(|guild| {
                                guild.insert(
                                    *user_id,
                                    Data {
                                        completed: true,
                                        score: user.score,
                                    },
                                );
                            })
                            .or_insert(HashMap::new());
                    }
                    (message.push(if penalties {
                        " did not complete the challenge :( each lost 1 point as a penalty"
                    } else {
                        " everyone completed the challenge! Awesome job to start a new day!"
                    }))
                    .push("\n\n");
                    construct_leader_board(users, message
                            .push("Share your code in the format below to confirm your completion of today's ")
                            .push_named_link("LeetCode", "https://leetcode.com/problemset")
                            .push(" Daily @everyone\n")
                            .push_safe("||```code```||\n"));
                    if let Err(why) = ChannelId::new(LEETCODE_CHANNEL_ID)
                        .say(ctx.clone().http, message.build())
                        .await
                    {
                        println!("Error sending daily message: {why:?}");
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
