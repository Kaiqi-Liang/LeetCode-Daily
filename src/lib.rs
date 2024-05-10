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

type Guild = HashMap<GuildId, Users>;
type Users = HashMap<UserId, User>;
type UserData = HashMap<GuildId, Data>;
type Data = HashMap<UserId, Status>;

#[derive(Serialize, Deserialize)]
pub struct Status {
    pub voted_for: Option<UserId>,
    pub completed: bool,
    pub score: usize,
}

pub struct UserStatus {
    pub user: User,
    pub status: Status,
}

pub struct SharedState {
    pub guild: Arc<Mutex<Guild>>,
    pub database: File,
    pub poll_id: Option<MessageId>,
    pub user_data: UserData,
}

pub struct State;
impl TypeMapKey for State {
    type Value = SharedState;
}

const CUSTOM_ID: &str = "favourite_solution";
const LEETCODE_CHANNEL_ID: u64 = 1235529498770935840; // TODO: Make this configurable
const NUM_SECS_IN_AN_HOUR: u64 = 3600;

macro_rules! send_message_with_leaderboard {
    ($ctx:ident, $guild:expr, $guild_id:ident, $users:ident, $message:expr) => {
        ChannelId::new(LEETCODE_CHANNEL_ID)
            .say(
                $ctx.clone().http,
                construct_leaderboard($users, $guild, $guild_id, &mut $message).build(),
            )
            .await?;
    };
}

macro_rules! write_to_database {
    ($state:ident) => {
        let data = serde_json::to_string_pretty(&$state.user_data)?;
        $state.database.seek(SeekFrom::Start(0))?;
        $state.database.set_len(0)?;
        $state.database.write_all(data.as_bytes())?;
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
    ($guild:expr, $guild_id:ident, $user_id:ident) => {
        $guild
            .get($guild_id)
            .expect("Guild does not exist in database")
            .get($user_id)
            .expect("User does not exist in database")
    };
    ($users:ident, $user_id:ident) => {
        $users.get_mut(&$user_id).ok_or("No user in guild")?
    };
}

macro_rules! get_users_from_guild_id {
    ($state:ident, $guild_id:ident) => {
        $state
            .user_data
            .get_mut(&$guild_id)
            .ok_or("No guild in member map")?
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

pub fn construct_leaderboard<'a>(
    users: &Data,
    guild: MutexGuard<Guild>,
    guild_id: &GuildId,
    message: &'a mut MessageBuilder,
) -> &'a mut MessageBuilder {
    message.push("The current leaderboard:\n");
    let mut leaderboard = users
        .iter()
        .map(|(id, user)| (get_user_from_id!(guild, guild_id, id), user.score))
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
        state.guild.lock().await.insert(
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
        time::sleep(Duration::from_secs(duration - NUM_SECS_IN_AN_HOUR)).await;

        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        if state.poll_id.is_none() {
            state.poll_id = Some(poll(&ctx, state).await?.id);
        }
        time::sleep(Duration::from_secs(NUM_SECS_IN_AN_HOUR)).await;

        for (guild_id, users) in state.user_data.iter_mut() {
            let guild = state.guild.lock().await;
            let mut message = MessageBuilder::new();
            message.push("Yesterday ");
            let mut penalties = false;
            let mut votes = HashMap::new();
            for (user_id, user) in users.iter_mut() {
                if let Some(voted_for) = user.voted_for {
                    votes
                        .entry(voted_for)
                        .and_modify(|votes| *votes += 1)
                        .or_insert(1);
                }
                if !user.completed {
                    penalties = true;
                    message.mention(get_user_from_id!(guild, guild_id, user_id));
                    if user.score > 0 {
                        user.score -= 1;
                    }
                } else {
                    user.completed = false;
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
                get_user_from_id!(users, user_id).score += votes;
            }
            send_message_with_leaderboard!(ctx, guild, guild_id, users, message);
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
    let users = get_users_from_guild_id!(state, guild_id);
    let mut message = MessageBuilder::new();
    if msg.content.contains("||") && msg.content.contains("```") {
        let user = get_user_from_id!(users, user_id);
        if user.completed {
            return Ok(());
        }
        user.completed = true;
        let score: usize = (time_till_utc_midnight().num_hours() / 10 + 1).try_into()?;
        user.score += score;
        message
            .push("Congrats to ")
            .mention(get_user_from_id!(state.guild.lock().await, guild_id, user_id))
            .push(format!(" for completing today's challenge! You have gained {score} points today your current score is {}\n", user.score));
        let guild = state.guild.lock().await.clone();
        let users_not_yet_completed = users
            .iter()
            .filter_map(|(id, user)| {
                if user.completed {
                    None
                } else {
                    Some(get_user_from_id!(guild, guild_id, id))
                }
            })
            .collect::<Vec<_>>();
        if users_not_yet_completed.is_empty() {
            message.push("Everyone has finished today's challenge, let's Grow Together!\n");
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
            state.poll_id = Some(poll(&ctx, state).await?.id);
        }
        return Ok(());
    }
    send_message_with_leaderboard!(ctx, state.guild.lock().await, guild_id, users, message);
    if should_poll {
        state.poll_id = Some(poll(&ctx, state).await?.id);
    }
    write_to_database!(state);
    Ok(())
}

async fn poll(ctx: &Context, state: &mut SharedState) -> Result<Message, Box<dyn Error>> {
    Ok(if let Some(poll_id) = state.poll_id {
        let message = ChannelId::new(LEETCODE_CHANNEL_ID)
            .message(&ctx.http, poll_id)
            .await?;
        message.reply(ctx, "You can vote via this poll")
            .await?;
		message
    } else {
        ChannelId::new(LEETCODE_CHANNEL_ID)
            .send_message(
                ctx,
                CreateMessage::new()
                    .content("Choose your favourite solution")
                    .select_menu(
                        CreateSelectMenu::new(
                            CUSTOM_ID,
                            CreateSelectMenuKind::User {
                                default_users: None,
                            },
                        )
                        .placeholder("No solution selected"),
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
        if component.data.custom_id == CUSTOM_ID
            && state
                .poll_id
                .is_some_and(|poll_id| poll_id == component.message.id)
        {
            if let ComponentInteractionDataKind::UserSelect { values } = &component.data.kind {
                if values.len() != 1 {
                    return Err("Did not select a single value".into());
                }
                let voted_for = values[0];
                let users = get_users_from_guild_id!(state, guild_id);
                if let Some(voted_for) = users.get(&voted_for) {
                    if !voted_for.completed {
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
                let user = get_user_from_id!(users, user_id);
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
