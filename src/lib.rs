use chrono::{TimeDelta, TimeZone, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serenity::{
    all::{
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage,
        CreateSelectMenuKind, CreateThread, EditMessage,
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
    time::Duration,
};
use tokio::time::sleep;

type Guilds = HashMap<GuildId, Users>;
type Users = HashMap<UserId, User>;
type Database = HashMap<GuildId, Data>;
type UserInfo = HashMap<UserId, Status>;

#[derive(Serialize, Deserialize)]
pub struct Data {
    users: UserInfo,
    channel_id: Option<ChannelId>,
    thread_id: Option<ChannelId>,
    poll_id: Option<MessageId>,
    active: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Status {
    voted_for: Option<UserId>,
    submitted: Option<String>,
    score: usize,
}

pub struct SharedState {
    pub guilds: Guilds,
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

macro_rules! get_thread_from_guild {
    ($guild:expr) => {
        $guild.thread_id.ok_or("No default thread")?
    };
}

macro_rules! send_message_with_leaderboard {
    ($ctx:ident, $guilds:expr, $guild_id:ident, $guild:expr, $message:expr) => {
        get_thread_from_guild!($guild)
            .say(
                &$ctx.http,
                construct_leaderboard(&$guild.users, $guilds, $guild_id, &mut $message).build(),
            )
            .await?
    };
}

macro_rules! send_help_message {
    ($ctx:ident, $message:expr, $bot:ident, $channel:expr, $default_channel:expr, $thread:expr) => {
        $channel.say(&$ctx.http, construct_format_message!(
        construct_channel_message!(
            $message
                .push("Hi I'm LeetCode Daily, here to motivate you to do ")
                .push_named_link("LeetCode", "https://leetcode.com/problemset")
                .push(" questions every single day 🤓\n\nI operate on a default channel and I create a thread in that channel every time a new daily question comes out\n"),
            $bot,
            $default_channel,
            $thread
        )
        .push("\nSome other commands you can run are")
        .push("\n* `/scores`: Shows the current leaderboard, has to be run in either today's thread or the default channel\n* `/help`: Shows this help message, can be run anywhere\n* `/poll`: Start a poll for today's submissions or reply to an existing one if it has already started, has to be run in the current thread\n* `/active [toggle]`: Check if the bot is currently active or toggle it on and off\n")
        .push("\nTo submit your code you have to put it a spoiler tag and wrap it with ")
        .push_safe("```code```")
        .push(" so others can't immediately see your solution. You can start from the template below and replace the language and code with your own. If you didn't follow the format strictly simply send it again\n")
        ).build()).await?;
    };
}

macro_rules! construct_format_message {
    ($message:expr) => {
        $message
            .push("``")
            .push(r"||```language")
            .push("\n")
            .push("code")
            .push("\n")
            .push("```||")
            .push("``")
    };
}

macro_rules! construct_daily_message {
    ($message:expr) => {
        construct_format_message!($message
            .push("\nShare your code in the format below to confirm your completion of today's ")
            .push_named_link("LeetCode", "https://leetcode.com/problemset")
            .push(" Daily @everyone\n"))
    };
}

macro_rules! construct_channel_message {
    ($message:expr, $bot:ident, $channel:expr, $thread:expr) => {
        $message
            .push("The default channel for ")
            .mention(&$bot)
            .push(" is ")
            .channel($channel)
            .push(" and today's thread is ")
            .channel($thread)
            .push("\nYou can change it by using the following command")
            .push_codeblock("/channel channel_id", None)
    };
}

macro_rules! write_to_database {
    ($state:ident) => {
        $state.file.seek(SeekFrom::Start(0))?;
        $state.file.set_len(0)?;
        $state
            .file
            .write_all(serde_json::to_string_pretty(&$state.database)?.as_bytes())?
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
    ($users:expr, $user_id:expr) => {
        $users.entry($user_id).or_insert(Status::default())
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
        $component
            .create_response(
                &$ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content($content)
                        .ephemeral(true),
                ),
            )
            .await
            .map_err(|e| e.into())
    };
}

macro_rules! send_invalid_channel_id_message {
    ($ctx:ident, $msg:ident) => {
        $msg.channel_id
            .say(&$ctx.http, "Invalid channel ID")
            .await?;
    };
}

macro_rules! send_channel_usage_message {
    ($ctx:ident, $channel:expr) => {
        $channel
            .say(
                &$ctx.http,
                MessageBuilder::new()
                    .push("Usage:")
                    .push_codeblock("/channel channel_id", None)
                    .build(),
            )
            .await?;
    };
}

macro_rules! create_thread {
    ($ctx:ident, $guild:ident) => {
        $guild.thread_id = get_channel_from_guild!($guild)
            .create_thread(
                &$ctx.http,
                CreateThread::new(Utc::now().format("%d/%m/%Y").to_string())
                    .kind(ChannelType::PublicThread)
                    .auto_archive_duration(AutoArchiveDuration::OneDay),
            )
            .await
            .map(|channel| channel.id)
            .ok();
    };
}

fn construct_leaderboard<'a>(
    users: &UserInfo,
    guilds: &Guilds,
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

fn time_till_utc_midnight() -> TimeDelta {
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

async fn initialise_guilds(
    ctx: &Context,
    guild_id: &GuildId,
    state: &mut SharedState,
) -> Result<(), Box<dyn Error>> {
    let members = guild_id.members(&ctx.http, None, None).await?;
    state.guilds.insert(
        *guild_id,
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
    Ok(())
}

pub async fn setup(ctx: &Context, ready: Ready) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    println!("Setting up guilds {:?}", ready.guilds);
    for guild in ready.guilds {
        initialise_guilds(ctx, &guild.id, get_shared_state!(data)).await?;
    }
    Ok(())
}

pub async fn schedule_daily_reset(ctx: Context) -> Result<(), Box<dyn Error>> {
    loop {
        let mut duration: u64 = time_till_utc_midnight().num_seconds().try_into()?;
        println!("{duration} seconds until next daily");
        if duration > NUM_SECS_IN_AN_HOUR {
            sleep(Duration::from_secs(duration - NUM_SECS_IN_AN_HOUR)).await;
            duration = NUM_SECS_IN_AN_HOUR;
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);

            for (guild_id, data) in state.database.iter_mut() {
                if data.poll_id.is_some() {
                    get_thread_from_guild!(data)
                        .say(&ctx.http, "An hour remaining before voting ends")
                        .await?;
                }
                data.poll_id = Some(poll(&ctx, data, &state.guilds, guild_id).await?.id);
            }
        }

        println!("Scheduled for next daily in {duration} seconds");
        sleep(Duration::from_secs(duration)).await;
        println!("It is now {:?}", Utc::now());
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        for (guild_id, guild) in state.database.iter_mut() {
            guild.poll_id = None;
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
                    message.mention(get_user_from_id!(state.guilds, guild_id, user_id));
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
                    "did not complete the challenge 😭 each lost 1 point as a penalty"
                } else {
                    "everyone completed the challenge! Awesome job to start a new day!"
                })
                .push("\nThe number of votes received:\n");
            let mut votes = votes.iter().collect::<Vec<_>>();
            votes.sort_by(|a, b| a.1.cmp(b.1));
            for (user_id, votes) in votes {
                get_user_from_id!(guild.users, *user_id).score += votes;
                message
                    .mention(get_user_from_id!(state.guilds, guild_id, user_id))
                    .push(format!(": {votes}\n"));
            }
            create_thread!(ctx, guild);
            send_message_with_leaderboard!(
                ctx,
                &state.guilds,
                guild_id,
                &guild,
                construct_daily_message!(message).push('\n')
            );
        }
        write_to_database!(state);
    }
}

pub async fn respond(ctx: Context, msg: Message, bot: UserId) -> Result<(), Box<dyn Error>> {
    if let Some(guild_id) = &msg.guild_id {
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        let user_id = &msg.author.id;
        let data = get_guild_from_id!(state, guild_id);
        let thread = get_thread_from_guild!(data);
        let channel = get_channel_from_guild!(data);
        let code_block = Regex::new(r"(?s)\|\|```.+```\|\|")?;
        let mut message = MessageBuilder::new();
        if msg.content.starts_with("/active") {
            let args = msg.content.split(' ').collect::<Vec<&str>>();
            msg.channel_id
                .say(
                    &ctx.http,
                    (if let Some(message_builder) = match args.len() {
                        1 => {
                            if msg.content == "/active" {
                                Some(message.mention(&bot).push(format!(
                                    " is {}active",
                                    if data.active { "" } else { "not " }
                                )))
                            } else {
                                None
                            }
                        }
                        2 => {
                            if msg.content == "/active toggle" {
                                data.active = !data.active;
                                Some(message.mention(&bot).push(format!(
                                    " is now {}",
                                    if data.active { "active" } else { "paused" }
                                )))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    } {
                        message_builder
                    } else {
                        message
                            .push("Usage:")
                            .push_codeblock("/active [toggle]", None)
                    })
                    .build(),
                )
                .await?;
        } else if !data.active {
        } else if msg.content == "/help" {
            send_help_message!(ctx, message, bot, msg.channel_id, channel, thread);
        } else if msg.content.starts_with("/channel") {
            let channel_id = msg.content.split(' ').last().ok_or("Empty message")?;
            if let Ok(channel_id) = channel_id.parse::<u64>() {
                let channel_id = ChannelId::new(channel_id);
                if let Ok(Channel::Guild(channel)) = channel_id.to_channel(&ctx.http).await {
                    if channel.kind != ChannelType::Text {
                        send_invalid_channel_id_message!(ctx, msg);
                    } else {
                        message
                            .push("Successfully set channel to be ")
                            .channel(channel_id);
                        msg.channel_id.say(&ctx.http, message.build()).await?;
                        data.channel_id = Some(channel_id);
                    }
                } else {
                    send_invalid_channel_id_message!(ctx, msg);
                }
            } else if msg.channel_id != channel && msg.channel_id != thread {
                msg.channel_id
                    .say(
                        &ctx.http,
                        construct_channel_message!(
                            message,
                            bot,
                            channel,
                            get_thread_from_guild!(data)
                        )
                        .build(),
                    )
                    .await?;
            } else {
                send_channel_usage_message!(ctx, msg.channel_id);
            }
        } else if thread != msg.channel_id {
            if channel == msg.channel_id {
                if msg.content == "/poll" || code_block.is_match(&msg.content) {
                    channel
                        .say(
                            &ctx.http,
                            message
                                .push("Please send your command and code in today's ")
                                .channel(thread)
                                .build(),
                        )
                        .await?;
                } else if msg.content == "/scores" {
                    msg.channel_id
                        .say(
                            &ctx.http,
                            construct_leaderboard(
                                &data.users,
                                &state.guilds,
                                guild_id,
                                &mut message,
                            )
                            .build(),
                        )
                        .await?;
                }
            }
        } else if code_block.is_match(&msg.content) {
            let user = get_user_from_id!(data.users, *user_id);
            if user.submitted.is_some() {
                message.push("You have already submitted today");
            } else {
                user.submitted = Some(msg.link());
                let score: usize = (time_till_utc_midnight().num_hours() / 10 + 1).try_into()?;
                user.score += score;
                message
            .push("Congrats to ")
            .mention(get_user_from_id!(state.guilds, guild_id, user_id))
            .push(format!(" for completing today's challenge! You have gained {score} points today your current score is {}\n", user.score));
                let users_not_yet_completed = data
                    .users
                    .iter()
                    .filter_map(|(id, user)| {
                        if user.submitted.is_some() {
                            None
                        } else {
                            Some(get_user_from_id!(state.guilds, guild_id, id))
                        }
                    })
                    .collect::<Vec<_>>();
                if let Some(poll_id) = data.poll_id {
                    msg.channel_id
                        .edit_message(
                            &ctx.http,
                            poll_id,
                            EditMessage::new().content(build_submission_message(
                                data,
                                &state.guilds,
                                guild_id,
                            )),
                        )
                        .await?;
                }
                if users_not_yet_completed.is_empty() {
                    message.push("Everyone has finished today's challenge, let's Grow Together!");
                } else {
                    message.push("Still waiting for ");
                    for user in users_not_yet_completed {
                        message.mention(user);
                    }
                }
            }
            send_message_with_leaderboard!(
                ctx,
                &state.guilds,
                guild_id,
                &data,
                message.push("\n\n")
            );
            data.poll_id = Some(poll(&ctx, data, &state.guilds, guild_id).await?.id);
        } else if msg.content == "/scores" {
            send_message_with_leaderboard!(ctx, &state.guilds, guild_id, &data, message);
        } else if msg.content == "/poll" {
            data.poll_id = Some(poll(&ctx, data, &state.guilds, guild_id).await?.id);
        }
        write_to_database!(state);
    } else {
        msg.channel_id
            .say(&ctx.http, "Please don't slide into my dm 😜")
            .await?;
    }
    Ok(())
}

fn build_submission_message(guild: &Data, guilds: &Guilds, guild_id: &GuildId) -> String {
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
    message.build()
}

async fn poll(
    ctx: &Context,
    guild: &Data,
    guilds: &Guilds,
    guild_id: &GuildId,
) -> Result<Message, Box<dyn Error>> {
    let thread = get_thread_from_guild!(guild);
    if let Some(poll_id) = guild.poll_id {
        if let Ok(message) = thread.message(&ctx.http, poll_id).await {
            message
                .reply(&ctx.http, "You can vote via this poll")
                .await?;
            Ok(message)
        } else {
            thread
                .say(&ctx.http, "Poll message is not in this channel")
                .await?;
            Err("Poll message is not in this channel".into())
        }
    } else {
        thread
            .send_message(
                &ctx.http,
                CreateMessage::new()
                    .content(build_submission_message(guild, guilds, guild_id))
                    .select_menu(
                        CreateSelectMenu::new(
                            CUSTOM_ID,
                            CreateSelectMenuKind::User {
                                default_users: None,
                            },
                        )
                        .placeholder("No submission selected"),
                    ),
            )
            .await
            .map_err(|e| e.into())
    }
}

pub async fn vote(ctx: Context, interaction: Interaction) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if let Interaction::Component(component) = interaction {
        let guild_id = &component
            .guild_id
            .ok_or("This interaction was not received over the gateway")?;
        let data = get_guild_from_id!(state, guild_id);
        if component.data.custom_id == CUSTOM_ID
            && data.active
            && data
                .poll_id
                .is_some_and(|poll_id| poll_id == component.message.id)
        {
            if let ComponentInteractionDataKind::UserSelect { values } = &component.data.kind {
                let voted_for = values.first().ok_or("Did not select a single value")?;
                if let Some(voted_for_status) = data.users.get(voted_for) {
                    if voted_for_status.submitted.is_none() {
                        return acknowledge_interaction!(
                            ctx,
                            component,
                            "Cannot vote for someone who hasn't completed the challenge"
                        );
                    }
                } else {
                    return acknowledge_interaction!(
                        ctx,
                        component,
                        "Cannot vote for someone who is not participating in the challenge"
                    );
                }
                let user_id = component.user.id;
                if *voted_for == user_id {
                    return acknowledge_interaction!(ctx, component, "Cannot vote for yourself");
                }
                let user = get_user_from_id!(data.users, user_id);
                user.voted_for = Some(*voted_for);
                write_to_database!(state);
                return acknowledge_interaction!(
                    ctx,
                    component,
                    format!(
                        "Successfully submitted your vote for {}",
                        get_user_from_id!(state.guilds, guild_id, voted_for)
                    )
                );
            }
        }
    }
    Ok(())
}

pub async fn initialise_guild(
    ctx: Context,
    guild: Guild,
    bot: UserId,
) -> Result<(), Box<dyn Error>> {
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
            thread_id: None,
            poll_id: None,
            active: true,
        };
        for channel in guild.channels.values() {
            if channel.kind == ChannelType::Text {
                data.channel_id = Some(channel.id);
                let guild_id = &guild.id;
                let mut message = MessageBuilder::new();
                send_help_message!(
                    ctx,
                    MessageBuilder::new(),
                    bot,
                    channel,
                    channel.id,
                    get_thread_from_guild!(data)
                );
                construct_daily_message!(message.push("\n"));
                create_thread!(ctx, data);
                state.database.insert(*guild_id, data);
                write_to_database!(state);
                initialise_guilds(&ctx, guild_id, state).await?;
                send_message_with_leaderboard!(
                    ctx,
                    &state.guilds,
                    guild_id,
                    get_guild_from_id!(state, guild_id),
                    message.push("\n\n")
                );
                return Ok(());
            }
        }
        Err("No available channel".into())
    } else {
        Ok(())
    }
}
