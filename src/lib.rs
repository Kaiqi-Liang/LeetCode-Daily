use chrono::{TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serenity::{
    model::prelude::*,
    prelude::*,
    utils::{EmbedMessageBuilding, MessageBuilder},
};
use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time};

type Users = HashMap<UserId, UserStatus>;
type Guild = HashMap<GuildId, Users>;
pub type UserData = HashMap<GuildId, HashMap<UserId, Data>>;

#[derive(Serialize, Deserialize, Debug)]
pub struct Data {
    pub completed: bool,
    pub score: usize,
}

pub struct UserStatus {
    pub user: User,
    pub completed: bool,
    pub score: usize,
}

pub struct SharedState {
    pub guild: Arc<Mutex<Guild>>,
    pub database: File,
    pub user_data: UserData,
}

pub struct State;
impl TypeMapKey for State {
    type Value = SharedState;
}

macro_rules! write_to_database {
    ($state:ident) => {
        let data = serde_json::to_string_pretty(&$state.user_data)?;
        $state.database.seek(SeekFrom::Start(0))?;
        $state.database.set_len(0)?;
        $state.database.write_all(data.as_bytes())?;
    };
}

macro_rules! send_message_with_leaderboard {
    ($ctx:ident, $users:ident, $message:ident) => {
        ChannelId::new(1235529498770935840) // TODO: Make this configurable
            .say(
                $ctx.clone().http,
                construct_leaderboard($users, &mut $message).build(),
            )
            .await?;
    };
}

pub fn construct_leaderboard<'a>(
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

pub fn time_till_utc_midnight() -> TimeDelta {
    Utc.from_utc_datetime(
        &Utc::now()
            .naive_utc()
            .date()
            .succ_opt()
            .expect("Invalid date")
            .and_hms_opt(0, 0, 0)
            .expect("Invalid hour, minute and/or second"),
    )
    .signed_duration_since(Utc::now())
}

pub async fn setup(ctx: &Context, ready: Ready) -> Result<(), Box<dyn Error>> {
    let data = ctx.data.write().await;
    let state = data
        .get::<State>()
        .ok_or("Failed to get share data from context")?;
    for guild in ready.guilds {
        let guild_id = guild.id;
        let members = guild.id.members(&ctx.http, None, None).await?;
        state.guild.lock().await.insert(
            guild.id,
            members
                .into_iter()
                .filter_map(|member| {
                    let user = member.user;
                    let user_data = state
                        .user_data
                        .get(&guild_id)
                        .and_then(|users| users.get(&user.id));
                    if user.bot {
                        None
                    } else {
                        Some((
                            user.id,
                            UserStatus {
                                user: user.clone(),
                                completed: user_data.map_or(false, |data| data.completed),
                                score: user_data.map_or(0, |data| data.score),
                            },
                        ))
                    }
                })
                .collect::<Users>(),
        );
    }
    Ok(())
}

pub async fn schedule_daily_reset(ctx: Context) -> Result<(), Box<dyn Error>> {
    loop {
        // Add 1 to make sure by the time the loop tries to schedule it again it will be the next day
        time::sleep(Duration::from_secs(
            (time_till_utc_midnight().num_seconds() + 1).try_into()?,
        ))
        .await;

        let mut data = ctx.data.write().await;
        let state = data
            .get_mut::<State>()
            .ok_or("Failed to get share data from context")?;
        let mut member_map = state.guild.lock().await;
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
                state
                    .user_data
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
                write_to_database!(state);
            }
            message
                .push(if penalties {
                    " did not complete the challenge :( each lost 1 point as a penalty"
                } else {
                    " everyone completed the challenge! Awesome job to start a new day!"
                })
                .push("\n\n");
            message
                .push("Share your code in the format below to confirm your completion of today's ")
                .push_named_link("LeetCode", "https://leetcode.com/problemset")
                .push(" Daily @everyone\n")
                .push_safe("||```code```||\n");
            send_message_with_leaderboard!(ctx, users, message);
        }
    }
}

pub async fn respond(ctx: Context, msg: Message) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = data
        .get_mut::<State>()
        .ok_or("Failed to get share data from context")?;
    let mut member_map = state.guild.lock().await;
    let guild_id = &msg
        .guild_id
        .ok_or("This message was not received over the gateway")?;
    let users = member_map
        .get_mut(guild_id)
        .ok_or("No guild in member map")?;
    let mut message = MessageBuilder::new();
    if msg.content.contains("||") && msg.content.contains("```") {
        let user = users.get_mut(&msg.author.id).ok_or("No user in guild")?;
        if !user.completed {
            user.completed = true;
            let score: usize = (time_till_utc_midnight().num_hours() / 10 + 1).try_into()?;
            user.score += score;
            state
                .user_data
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
        write_to_database!(state);
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
            message.push("Everyone has finished today's challenge, let's Grow Together!\n");
        } else {
            message.push("Still waiting for ");
            for user in users_not_yet_completed {
                message.mention(user);
            }
        }
    } else if msg.content != "/scores" {
        return Ok(());
    }
    send_message_with_leaderboard!(ctx, users, message);
    Ok(())
}
