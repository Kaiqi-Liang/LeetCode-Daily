use chrono::{TimeDelta, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serenity::{
    all::{
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
        CreateSelectMenuKind,
    },
    builder::CreateSelectMenu,
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
use tokio::{
    sync::{Mutex, MutexGuard},
    time,
};

type Guilds = HashMap<GuildId, Users>;
type Users = HashMap<UserId, User>;
type Database = HashMap<GuildId, Data>;
type UserInfo = HashMap<UserId, Status>;

#[derive(Serialize, Deserialize)]
pub struct Data {
    users: UserInfo,
    channel_id: Option<ChannelId>,
    poll_id: Option<MessageId>,
}

#[derive(Serialize, Deserialize)]
struct Status {
    pub voted_for: Option<UserId>,
    pub submitted: Option<String>,
    pub score: usize,
}

pub struct SharedState {
    pub guilds: Arc<Mutex<Guilds>>,
    pub file: File,
    pub database: Database,
}

pub struct State;
impl TypeMapKey for State {
    type Value = SharedState;
}

const CUSTOM_ID: &str = "favourite_submission";
const NUM_SECS_IN_AN_HOUR: u64 = 3600;

macro_rules! get_channel_from_guild {
    ($guild:expr) => {
        $guild.channel_id.ok_or("No default channel")?
    };
}

macro_rules! send_message_with_leaderboard {
    ($ctx:ident, $guilds:expr, $guild_id:ident, $guild:expr, $message:expr) => {
        get_channel_from_guild!($guild)
            .say(
                $ctx.clone().http,
                construct_leaderboard(&$guild.users, $guilds, $guild_id, &mut $message).build(),
            )
            .await?;
    };
}

macro_rules! write_to_database {
    ($state:ident) => {
        $state.file.seek(SeekFrom::Start(0))?;
        $state.file.set_len(0)?;
        $state
            .file
            .write_all(serde_json::to_string_pretty(&$state.database)?.as_bytes())?;
    };
}

macro_rules! get_shared_state {
    ($data:ident) => {{
        $data
            .get_mut::<State>()
            .ok_or("Failed to get share data from context")?
    }};
}

macro_rules! get_user_from_id {
    ($guilds:expr, $guild_id:ident, $user_id:ident) => {
        $guilds
            .get($guild_id)
            .expect("Guild does not exist")
            .get($user_id)
            .expect("User does not exist")
    };
    ($users:expr, $user_id:ident) => {
        $users.get_mut(&$user_id).ok_or("No user in guild")?
    };
}

macro_rules! get_guild_from_id {
    ($state:ident, $guild_id:ident) => {
        &mut $state
            .database
            .get_mut(&$guild_id)
            .ok_or("Guild does not exist in database")?
    };
}

macro_rules! acknowledge_interaction {
    ($ctx:ident, $component:ident, $content:expr) => {
        Ok($component
            .create_response(
                &$ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content($content)
                        .ephemeral(true),
                ),
            )
            .await?)
    };
}

fn construct_leaderboard<'a>(
    users: &UserInfo,
    guilds: MutexGuard<Guilds>,
    guild_id: &GuildId,
    message: &'a mut MessageBuilder,
) -> &'a mut MessageBuilder {
    message.push("The current leaderboard:\n");
    let mut leaderboard = users
        .iter()
        .map(|(id, user)| (get_user_from_id!(guilds, guild_id, id), user.score))
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
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    for guild in ready.guilds {
        let members = guild.id.members(&ctx.http, None, None).await?;
        state.guilds.lock().await.insert(
            guild.id,
            members
                .into_iter()
                .filter_map(|member| {
                    let user = member.user;
                    if user.bot {
                        None
                    } else {
                        Some((user.id, user))
                    }
                })
                .collect::<Users>(),
        );
    }
    Ok(())
}

pub async fn schedule_daily_reset(ctx: Context) -> Result<(), Box<dyn Error>> {
    loop {
        let duration: u64 = time_till_utc_midnight().num_seconds().try_into()?;
        if duration > NUM_SECS_IN_AN_HOUR {
            time::sleep(Duration::from_secs(duration - NUM_SECS_IN_AN_HOUR)).await;
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);

            for (guild_id, guild) in state.database.iter_mut() {
                if guild.poll_id.is_none() {
                    guild.poll_id = Some(
                        poll(&ctx, guild, state.guilds.lock().await, guild_id)
                            .await?
                            .id,
                    );
                }
            }
        }

        time::sleep(Duration::from_secs(NUM_SECS_IN_AN_HOUR)).await;
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        for (guild_id, guild) in state.database.iter_mut() {
            guild.channel_id = None;
            let guilds = state.guilds.lock().await;
            let mut message = MessageBuilder::new();
            message.push("Yesterday ");
            let mut penalties = false;
            let mut votes = HashMap::new();
            for (user_id, user) in guild.users.iter_mut() {
                if let Some(voted_for) = user.voted_for {
                    votes
                        .entry(voted_for)
                        .and_modify(|votes| *votes += 1)
                        .or_insert(1);
                }
                if user.submitted.is_none() {
                    penalties = true;
                    message.mention(get_user_from_id!(guilds, guild_id, user_id));
                    if user.score > 0 {
                        user.score -= 1;
                    }
                } else {
                    user.submitted = None;
                }
                user.voted_for = None;
            }
            message
                .push(if penalties {
                    "did not complete the challenge :( each lost 1 point as a penalty"
                } else {
                    "everyone completed the challenge! Awesome job to start a new day!"
                })
                .push(
                    "\nShare your code in the format below to confirm your completion of today's ",
                )
                .push_named_link("LeetCode", "https://leetcode.com/problemset")
                .push(" Daily @everyone\n")
                .push_safe("||```code```||\n\n");
            for (user_id, votes) in votes {
                get_user_from_id!(guild.users, user_id).score += votes;
            }
            send_message_with_leaderboard!(ctx, guilds, guild_id, &guild, message);
        }
        write_to_database!(state);
    }
}

pub async fn respond(ctx: Context, msg: Message) -> Result<(), Box<dyn Error>> {
    let mut should_poll = false;
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    let user_id = &msg.author.id;
    let guild_id = &msg
        .guild_id
        .ok_or("This message was not received over the gateway")?;
    let guild = get_guild_from_id!(state, guild_id);
    let mut message = MessageBuilder::new();
    if msg.content.starts_with("/channel") {
        let channel_id = msg.content.split(' ').last().ok_or("Empty message")?;
        if let Ok(channel_id) = channel_id.parse::<u64>() {
            let channel_id = ChannelId::new(channel_id);
            guild.channel_id = Some(channel_id);
            write_to_database!(state);
            message
                .push("Successfully set channel to be ")
                .channel(channel_id);
            msg.channel_id.say(&ctx.http, message.build()).await?;
        } else {
            msg.channel_id
                .say(ctx.http, "Usage: /channel channel_id")
                .await?;
        }
        return Ok(());
    } else if msg.content.contains("||") && msg.content.contains("```") {
        let user = get_user_from_id!(guild.users, user_id);
        if user.submitted.is_some() {
            return Ok(());
        }
        user.submitted = Some(msg.content);
        let score: usize = (time_till_utc_midnight().num_hours() / 10 + 1).try_into()?;
        user.score += score;
        message
            .push("Congrats to ")
            .mention(get_user_from_id!(state.guilds.lock().await, guild_id, user_id))
            .push(format!(" for completing today's challenge! You have gained {score} points today your current score is {}\n", user.score));
        let guilds = state.guilds.lock().await.clone();
        let users_not_yet_completed = guild
            .users
            .iter()
            .filter_map(|(id, user)| {
                if user.submitted.is_some() {
                    None
                } else {
                    Some(get_user_from_id!(guilds, guild_id, id))
                }
            })
            .collect::<Vec<_>>();
        if users_not_yet_completed.is_empty() {
            message.push("Everyone has finished today's challenge, let's Grow Together!");
            should_poll = true;
        } else {
            message.push("Still waiting for ");
            for user in users_not_yet_completed {
                message.mention(user);
            }
        }
        message.push("\n\n");
    } else if msg.content != "/scores" {
        if msg.content == "/poll" {
            if guild.poll_id.is_none() {
                msg.channel_id
                    .say(&ctx.http, "There is no poll yet")
                    .await?;
                return Ok(());
            } else {
                guild.poll_id = Some(
                    poll(&ctx, guild, state.guilds.lock().await, guild_id)
                        .await?
                        .id,
                );
                write_to_database!(state);
            }
        }
        return Ok(());
    }
    send_message_with_leaderboard!(ctx, state.guilds.lock().await, guild_id, &guild, message);
    if should_poll {
        guild.poll_id = Some(
            poll(&ctx, &guild, state.guilds.lock().await, guild_id)
                .await?
                .id,
        );
    }
    write_to_database!(state);
    Ok(())
}

async fn poll(
    ctx: &Context,
    guild: &Data,
    guilds: MutexGuard<'_, Guilds>,
    guild_id: &GuildId,
) -> Result<Message, Box<dyn Error>> {
    let channel = get_channel_from_guild!(guild);
    Ok(if let Some(poll_id) = guild.poll_id {
        let message = channel.message(&ctx.http, poll_id).await?;
        message.reply(ctx, "You can vote via this poll").await?;
        message
    } else {
        let mut message = MessageBuilder::new();
        message.push("Choose your favourite submission\n");
        for (id, status) in guild.users.iter() {
            if let Some(submitted) = &status.submitted {
                message
                    .mention(get_user_from_id!(guilds, guild_id, id))
                    .push(submitted)
                    .push("\n");
            }
        }
        channel
            .send_message(
                ctx,
                CreateMessage::new().content(message.build()).select_menu(
                    CreateSelectMenu::new(
                        CUSTOM_ID,
                        CreateSelectMenuKind::User {
                            default_users: None,
                        },
                    )
                    .placeholder("No submission selected"),
                ),
            )
            .await?
    })
}

pub async fn vote(ctx: Context, interaction: Interaction) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if let Interaction::Component(component) = interaction {
        let guild_id = &component
            .guild_id
            .ok_or("This interaction was not received over the gateway")?;
        let guild = get_guild_from_id!(state, guild_id);
        if component.data.custom_id == CUSTOM_ID
            && guild
                .poll_id
                .is_some_and(|poll_id| poll_id == component.message.id)
        {
            if let ComponentInteractionDataKind::UserSelect { values } = &component.data.kind {
                if values.len() != 1 {
                    return Err("Did not select a single value".into());
                }
                let voted_for = values[0];
                if let Some(voted_for) = guild.users.get(&voted_for) {
                    if voted_for.submitted.is_none() {
                        return acknowledge_interaction!(
                            ctx,
                            component,
                            "You cannot vote for someone who hasn't completed the challenge"
                        );
                    }
                } else {
                    return acknowledge_interaction!(
                        ctx,
                        component,
                        "You cannot vote for someone who is not participating in the challenge"
                    );
                }
                let user_id = component.user.id;
                let user = get_user_from_id!(guild.users, user_id);
                if voted_for == user_id {
                    return acknowledge_interaction!(
                        ctx,
                        component,
                        "You cannot vote for yourself"
                    );
                }
                user.voted_for = Some(voted_for);
                write_to_database!(state);
                return acknowledge_interaction!(
                    ctx,
                    component,
                    "You have successfully submitted your vote"
                );
            }
        }
    }
    Ok(())
}

pub async fn initialise_guild(ctx: Context, guild: Guild) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if !state.database.contains_key(&guild.id) {
        let mut data = Data {
            users: guild
                .id
                .members(&ctx.http, None, None)
                .await?
                .into_iter()
                .filter_map(|member| {
                    let user = member.user;
                    if user.bot {
                        None
                    } else {
                        Some((
                            user.id,
                            Status {
                                voted_for: None,
                                submitted: None,
                                score: 0,
                            },
                        ))
                    }
                })
                .collect(),
            channel_id: None,
            poll_id: None,
        };
        let mut channel = guild.channels.iter();
        while let Some((&id, guild_channel)) = channel.next() {
            if guild_channel.kind == ChannelType::Text {
                data.channel_id = Some(id);
                state.database.insert(guild.id, data);
                write_to_database!(state);
                return Ok(());
            }
        }
        Err("No available channel".into())
    } else {
        Ok(())
    }
}
