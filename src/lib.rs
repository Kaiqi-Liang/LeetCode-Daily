mod leetcode;
use chrono::{Datelike, Month, TimeDelta, TimeZone, Utc, Weekday};
use leetcode::send_leetcode_daily_question_message;
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
    cmp::Ordering,
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

#[derive(Serialize, Deserialize, Clone)]
pub struct Data {
    users: UserInfo,
    channel_id: Option<ChannelId>,
    thread_id: Option<ChannelId>,
    weekly_id: Option<ChannelId>,
    poll_id: Option<MessageId>,
    active_weekly: bool,
    active_daily: bool,
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct Status {
    voted_for: Option<UserId>,
    submitted: Option<String>,
    weekly_submissions: usize,
    monthly_record: u32,
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
const FORMAT_MESSAGE: &str = "Share your code in the format below to submit your solution\n";

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
    ($ctx:ident, $guilds:expr, $guild_id:ident, $channel_id:expr, $users:expr, $message:expr) => {
        $channel_id
            .say(
                &$ctx.http,
                construct_leaderboard($users, $guilds, $guild_id, &mut $message).build(),
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
                .push_line(" questions every single day 🤓\n\nI operate on a default channel and I create a thread in that channel when a new daily question comes out"),
            $bot,
            $default_channel,
            $thread
        )
        .push_line("\n\nSome other commands you can run are")
        .push_line("* `/scores`: Shows the current leaderboard, has to be run in either today's thread or the default channel\n* `/help`: Shows this help message, can be run anywhere\n* `/poll`: Start a poll for today's submissions or reply to an existing one if it has already started, has to be run in the current thread\n* `/active [weekly|daily] [toggle]`: Check whether some features of the bot are currently active or toggle them on and off")
        .push("\nTo submit your code you have to put it in a spoiler tag and wrap it with ")
        .push_safe("```code```")
        .push_line(" so others can't immediately see your solution. You can start from the template below and replace the language and code with your own. If you didn't follow the format strictly simply send it again")
        ).build()).await?
    };
}

macro_rules! construct_format_message {
    ($message:expr) => {
        $message
            .push("``")
            .push_line(r"||```language")
            .push_line("code")
            .push("```||")
            .push("``")
    };
}

macro_rules! construct_badge_message {
    ($message:expr, $month:expr) => {
        $message.push_line(format!(
            " for earning the Daily Challenge badge for {:?}",
            Month::try_from(TryInto::<u8>::try_into($month.month())?)?
        ))
    };
}

macro_rules! send_daily_message_with_leaderboard {
    ($ctx:ident, $state:ident, $guild_id:ident, $data:ident, $message:expr) => {
        let channel_id = get_channel_from_guild!($data);
        let message_id = send_leetcode_daily_question_message($ctx, channel_id)
            .await?
            .id;
        $data.thread_id = channel_id
            .create_thread_from_message(
                &$ctx.http,
                message_id,
                CreateThread::new(Utc::now().format("%d/%m/%Y").to_string())
                    .kind(ChannelType::PublicThread)
                    .auto_archive_duration(AutoArchiveDuration::OneDay),
            )
            .await
            .map(|channel| channel.id)
            .ok();
        send_message_with_leaderboard!(
            $ctx,
            &$state.guilds,
            $guild_id,
            $data.thread_id.ok_or("Failed to create thread")?,
            &$data.users,
            construct_format_message!($message.push(FORMAT_MESSAGE)).push_line("\n")
        )
    };
}

macro_rules! construct_congrats_message {
    ($message:expr, $state:ident, $guild_id:ident, $user_id:ident) => {
        $message
            .push("Congrats to ")
            .mention(get_user_from_id!($state.guilds, $guild_id, $user_id))
            .push(" for ")
    };
}

macro_rules! construct_thread_message {
    ($message:expr, $thread:expr) => {
        if let Some(thread_id) = $thread {
            $message.push("Today's thread is ").channel(thread_id)
        } else {
            $message.push("Daily is not active")
        }
    };
}

macro_rules! construct_channel_message {
    ($message:expr, $bot:ident, $channel:expr, $thread:expr) => {
        construct_thread_message!(
            $message
                .push("The default channel for ")
                .mention(&$bot)
                .push(" is ")
                .channel($channel)
                .push("\nYou can change it by using the following command")
                .push_codeblock("/channel channel_id", None),
            $thread
        )
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
    ($ctx:ident, $guild:ident, $title:expr) => {
        get_channel_from_guild!($guild)
            .create_thread(
                &$ctx.http,
                CreateThread::new($title)
                    .kind(ChannelType::PublicThread)
                    .auto_archive_duration(AutoArchiveDuration::OneDay),
            )
            .await
            .map(|channel| channel.id)
            .ok()
    };
}

pub async fn save_to_database(ctx: Context) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    write_to_database!(state);
    Ok(())
}

fn construct_leaderboard<'a>(
    users: &UserInfo,
    guilds: &Guilds,
    guild_id: &GuildId,
    message: &'a mut MessageBuilder,
) -> &'a mut MessageBuilder {
    message.push_line("The current leaderboard:");
    let mut leaderboard = users
        .iter()
        .map(|(id, user)| {
            (
                get_user_from_id!(guilds, guild_id, id),
                user.score,
                user.monthly_record,
            )
        })
        .collect::<Vec<_>>();
    leaderboard.sort_by(|a, b| {
        let cmp = b.1.cmp(&a.1);
        if let Ordering::Equal = cmp {
            b.2.cmp(&a.2)
        } else {
            cmp
        }
    });
    let mut has_score = false;
    let mut place = 1;
    for (user, score, monthly_record) in leaderboard {
        if score > 0 {
            has_score = true;
            message
                .push_line(format!("{place}. {}", user.name))
                .push_bold(format!("\t{score}"))
                .push_line(if score > 1 { " points" } else { " point" })
                .push_bold(format!("\t{monthly_record}"))
                .push(if monthly_record > 1 {
                    " questions"
                } else {
                    " question"
                })
                .push_line(" completed this month");
            place += 1;
        }
    }
    if !has_score {
        message.push("No one has any points yet");
    }
    message
}

fn time_till_utc_midnight() -> Result<TimeDelta, Box<dyn Error>> {
    Ok(Utc
        .from_utc_datetime(
            &Utc::now()
                .naive_utc()
                .date()
                .succ_opt()
                .ok_or("Invalid date")?
                .and_hms_opt(0, 1, 0)
                .ok_or("Invalid time")?,
        )
        .signed_duration_since(Utc::now()))
}

fn num_days_curr_month() -> Result<u32, Box<dyn Error>> {
    let now = Utc::now();
    let this_month = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .unwrap();
    let next_month = Utc
        .with_ymd_and_hms(now.year(), now.month() + 1, 1, 0, 0, 0)
        .unwrap();
    Ok(TryInto::<u32>::try_into(
        next_month.signed_duration_since(this_month).num_days(),
    )?)
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
    println!("Setting up guilds {:?}", ready.guilds);
    let mut data = ctx.data.write().await;
    for guild in ready.guilds {
        initialise_guilds(ctx, &guild.id, get_shared_state!(data)).await?;
    }
    Ok(())
}

pub async fn schedule_daily_question(ctx: &Context) -> Result<(), Box<dyn Error>> {
    loop {
        let mut duration: u64 = time_till_utc_midnight()?.num_seconds().try_into()?;
        println!("{duration} seconds until next daily");
        let num_secs_in_an_hour: u64 = chrono::Duration::minutes(60).num_seconds().try_into()?;
        if duration > num_secs_in_an_hour {
            sleep(Duration::from_secs(duration - num_secs_in_an_hour)).await;
            duration = num_secs_in_an_hour;
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);

            for (guild_id, data) in state.database.iter_mut() {
                if !data.active_daily {
                    continue;
                }
                data.poll_id = Some(poll(ctx, data, &state.guilds, guild_id).await?.id);
                if data.poll_id.is_some() {
                    get_thread_from_guild!(data)
                        .say(&ctx.http, "An hour remaining before voting ends")
                        .await?;
                }
            }
        }

        sleep(Duration::from_secs(duration)).await;
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        for (guild_id, data) in state.database.iter_mut() {
            data.poll_id = None;
            data.thread_id = None;
            if !data.active_daily {
                continue;
            }
            let mut message = MessageBuilder::new();
            message.push("Yesterday ");
            let mut penalties = 0;
            let mut votes = HashMap::new();
            if Utc::now().day0() == 0 {
                if let Some(status) = data
                    .users
                    .values()
                    .max_by_key(|status| status.monthly_record)
                {
                    let highest_monthly_record = status.monthly_record;
                    let last_month = Utc::now().date_naive().pred_opt().ok_or("Invalid date")?;
                    if highest_monthly_record > 0 {
                        let mut message = MessageBuilder::new();
                        message.push("Welcome to a new month! Last month ");
                        for (user_id, status) in
                            data.users.iter_mut().filter(|(_, monthly_record)| {
                                monthly_record.monthly_record == highest_monthly_record
                            })
                        {
                            message.mention(user_id);
                            status.score += 5;
                            if highest_monthly_record == last_month.day() {
                                status.score += 10;
                            }
                        }
                        message.push_line(format!("completed {highest_monthly_record} questions which is the highest in this server! You have all been rewarded 5 points"));
                        if highest_monthly_record == last_month.day() {
                            construct_badge_message!(
                                message.push("And another 10 points"),
                                last_month
                            );
                        }
                        send_message_with_leaderboard!(
                            ctx,
                            &state.guilds,
                            guild_id,
                            get_channel_from_guild!(data),
                            &data.users,
                            message
                        );
                    }
                }
            }
            for user in data.users.values_mut() {
                if Utc::now().day0() == 0 {
                    user.monthly_record = 0;
                }
                if let Some(voted_for) = user.voted_for {
                    votes
                        .entry(voted_for)
                        .and_modify(|votes| *votes += 1)
                        .or_insert(1);
                }
                if user.submitted.is_none() {
                    penalties += 1;
                    if user.score > 0 {
                        user.score -= 1;
                    }
                } else {
                    user.submitted = None;
                }
                user.voted_for = None;
            }
            message
                .push_line(if penalties > 0 {
                    format!("{penalties} {} did not complete the challenge 😭 each lost 1 point as a penalty", if penalties > 1 { "people" } else { "person" })
                } else {
                    "everyone completed the challenge! Awesome job to start a new day!".to_string()
                })
                .push_line("\nThe number of votes received:");
            let mut votes = votes.iter().collect::<Vec<_>>();
            votes.sort_by(|a, b| a.1.cmp(b.1));
            for (user_id, &votes) in votes.iter() {
                get_user_from_id!(data.users, **user_id).score += votes;
                message
                    .mention(get_user_from_id!(state.guilds, guild_id, user_id))
                    .push_line(format!(": {votes}"));
            }
            if votes.is_empty() {
                message.push_line("There are no votes");
            }
            send_daily_message_with_leaderboard!(ctx, state, guild_id, data, message.push('\n'));
        }
        write_to_database!(state);
    }
}

pub async fn schedule_weekly_contest(ctx: &Context) -> Result<(), Box<dyn Error>> {
    loop {
        let now = Utc::now();
        let num_days_from_sunday: i64 = Weekday::Sun.days_since(now.weekday()).into();
        let same_day_until_contest_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), now.day(), 2, 30, 0)
            .unwrap()
            .time()
            .signed_duration_since(now.time())
            .num_seconds();
        let duration = Duration::from_secs(
            (if num_days_from_sunday == 0 {
                if same_day_until_contest_start.is_positive() {
                    0
                } else {
                    chrono::Duration::weeks(1).num_seconds()
                }
            } else {
                chrono::Duration::days(num_days_from_sunday).num_seconds()
            } + same_day_until_contest_start)
                .try_into()?,
        );
        println!("{num_days_from_sunday} days / {duration:?} until next contest");
        sleep(duration).await;
        {
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);
            for (guild_id, data) in state.database.iter_mut() {
                if data.active_weekly {
                    data.weekly_id =
                        create_thread!(ctx, data, format!("{:?}", Utc::now().iso_week()));
                    send_message_with_leaderboard!(
                        ctx,
                        &state.guilds,
                        guild_id,
                        data.weekly_id.ok_or("Failed to create thread")?,
                        &data.users,
                        construct_format_message!(MessageBuilder::new()
                            .push_line("Weekly contest starting now!")
                            .push_line(FORMAT_MESSAGE))
                        .push(
                            "The first 3 to finish all 4 questions will get bonus points @everyone"
                        )
                    );
                }
                for user in data.users.values_mut() {
                    user.weekly_submissions = 0;
                }
            }
            write_to_database!(state);
        }
        sleep(Duration::from_secs(
            chrono::Duration::minutes(90).num_seconds().try_into()?,
        ))
        .await;
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        for data in state.database.values_mut() {
            data.weekly_id = None;
            for user in data.users.values_mut() {
                user.weekly_submissions = 0;
            }
        }
        write_to_database!(state);
    }
}

pub async fn respond(ctx: &Context, msg: Message, bot: UserId) -> Result<(), Box<dyn Error>> {
    if let Some(guild_id) = &msg.guild_id {
        let mut data = ctx.data.write().await;
        let state = get_shared_state!(data);
        let user_id = &msg.author.id;
        let data = get_guild_from_id!(state, guild_id);
        let channel = get_channel_from_guild!(data);
        let code_block = Regex::new(r"(?s)```.+```")?;
        let mut message = MessageBuilder::new();
        if msg.content.starts_with("/active") {
            let args = msg.content.split(' ').collect::<Vec<&str>>();
            msg.channel_id
                .say(
                    &ctx.http,
                    (if let Some(message_builder) = match args.len() {
                        1 => {
                            if msg.content == "/active" {
                                Some(message.mention(&bot).push(
                                    if data.active_weekly && data.active_daily {
                                        " is active for both weekly and daily"
                                    } else if data.active_weekly {
                                        " is only active for weekly"
                                    } else if data.active_daily {
                                        " is only active for daily"
                                    } else {
                                        " is not active"
                                    },
                                ))
                            } else {
                                None
                            }
                        }
                        2 => {
                            if args[0] != "/active" || args[1] != "weekly" || args[1] != "daily" {
                                None
                            } else {
                                Some(
                                    message
                                        .mention(&bot)
                                        .push(format!(" is active for {}", args[1])),
                                )
                            }
                        }
                        3 => {
                            if args[0] != "/active" && args[2] != "toggle" {
                                None
                            } else if args[1] == "weekly" || args[1] == "daily" {
                                let active = if args[1] == "weekly" {
                                    &mut data.active_weekly
                                } else {
                                    &mut data.active_daily
                                };
                                *active = !*active;
                                if args[1] == "daily" && *active && data.thread_id.is_none() {
                                    send_daily_message_with_leaderboard!(
                                        ctx,
                                        state,
                                        guild_id,
                                        data,
                                        &mut MessageBuilder::new()
                                    );
                                }
                                Some(message.mention(&bot).push(format!(
                                    " is now {} for {}",
                                    if *active { "active" } else { "paused" },
                                    args[1]
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
                            .push_codeblock("/active [weekly|daily] [toggle]", None)
                    })
                    .build(),
                )
                .await?;
        } else if msg.content == "/help" {
            send_help_message!(ctx, message, bot, msg.channel_id, channel, data.thread_id);
        } else if msg.content == "/scores"
            && (msg.channel_id == channel
                || msg.channel_id == data.thread_id.unwrap_or_default()
                || msg.channel_id == data.weekly_id.unwrap_or_default())
        {
            send_message_with_leaderboard!(
                ctx,
                &state.guilds,
                guild_id,
                msg.channel_id,
                &data.users,
                message
            );
        } else if let Some(thread) = data.thread_id {
            if msg.content.starts_with("/channel") {
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
                            construct_channel_message!(message, bot, channel, Some(thread)).build(),
                        )
                        .await?;
                } else {
                    send_channel_usage_message!(ctx, msg.channel_id);
                }
            } else if !data.active_daily && !data.active_weekly {
            } else if channel == msg.channel_id {
                message.push("Please send your ");
                if channel == msg.channel_id {
                    if msg.content == "/poll" {
                        if !data.active_daily {
                            return Ok(());
                        }
                        message.push("command in today's ").channel(thread);
                    } else if code_block.is_match(&msg.content) {
                        if data.active_daily {
                            message
                                .push("code in today's daily thread ")
                                .channel(thread);
                            if data.active_weekly {
                                if let Some(weekly) = data.weekly_id {
                                    message
                                        .push("or this week's weekly thread ")
                                        .channel(weekly);
                                }
                            }
                        } else if let Some(weekly) = data.weekly_id {
                            message
                                .push("code in this week's weekly thread ")
                                .channel(weekly);
                        }
                    } else {
                        return Ok(());
                    }
                    channel.say(&ctx.http, message.build()).await?;
                }
            } else if code_block.is_match(&msg.content) {
                if data.active_daily && msg.channel_id == thread {
                    let user = get_user_from_id!(data.users, *user_id);
                    if user.submitted.is_some() {
                        message.push("You have already submitted today");
                    } else {
                        user.submitted = Some(msg.link());
                        let score: usize =
                            (time_till_utc_midnight()?.num_hours() / 10 + 1).try_into()?;
                        user.score += score;
                        user.monthly_record += 1;
                        construct_congrats_message!(message, state, guild_id, user_id)
                            .push("completing today's challenge! You have been rewarded ")
                            .push_bold(score.to_string())
                            .push(" points, your current score is ")
                            .push_bold(user.score.to_string())
                            .push(". This month you have completed ")
                            .push_bold(user.monthly_record.to_string())
                            .push_line(" questions!");
                        if user.monthly_record == num_days_curr_month()? {
                            construct_badge_message!(message.push("Great job"), Utc::now());
                        }
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
                            message.push(
                                "Everyone has finished today's challenge, let's Grow Together!",
                            );
                        }
                    }
                    data.poll_id = Some(poll(ctx, data, &state.guilds, guild_id).await?.id);
                    thread.say(&ctx.http, message.build()).await?;
                } else if data.active_weekly {
                    if let Some(weekly_id) = data
                        .weekly_id
                        .filter(|&weekly_id| weekly_id == msg.channel_id)
                    {
                        let (reward, place) = match data
                            .users
                            .values()
                            .filter(|user| user.weekly_submissions == 4)
                            .count()
                        {
                            0 => (4, "first"),
                            1 => (3, "second"),
                            2 => (2, "third"),
                            _ => (1, "after top 3"),
                        };
                        let user = get_user_from_id!(data.users, *user_id);
                        if user.weekly_submissions < 4 {
                            user.weekly_submissions += 1;
                            user.score += if user.weekly_submissions == 4 {
                                construct_congrats_message!(message, state, guild_id, user_id)
                                    .push(format!(
                                    "coming {} in the contest, you have been rewarded {} points",
                                    place, reward
                                ));
                                reward
                            } else {
                                construct_congrats_message!(message, state, guild_id, user_id)
                                .push("finishing one question in the contest, you just received 1 point for your weekly contest submission");
                                1
                            };
                            send_message_with_leaderboard!(
                                ctx,
                                &state.guilds,
                                guild_id,
                                weekly_id,
                                &data.users,
                                message.push_line('\n')
                            );
                        } else {
                            weekly_id
                                .say(&ctx.http, "You have already completed the contest")
                                .await?;
                        }
                    }
                }
            } else if msg.content == "/poll" && msg.channel_id == thread && data.active_daily {
                data.poll_id = Some(poll(ctx, data, &state.guilds, guild_id).await?.id);
            }
        }
    } else {
        msg.channel_id
            .say(&ctx.http, "Please don't slide into my dm 😜")
            .await?;
    }
    Ok(())
}

fn build_submission_message(guild: &Data, guilds: &Guilds, guild_id: &GuildId) -> String {
    let mut message = MessageBuilder::new();
    message.push_line("Choose your favourite submission");
    for (id, status) in guild.users.iter() {
        if let Some(submitted) = &status.submitted {
            message
                .mention(get_user_from_id!(guilds, guild_id, id))
                .push_line(submitted);
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

pub async fn vote(ctx: &Context, interaction: Interaction) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if let Interaction::Component(component) = interaction {
        let guild_id = &component
            .guild_id
            .ok_or("This interaction was not received over the gateway")?;
        let data = get_guild_from_id!(state, guild_id);
        if component.data.custom_id == CUSTOM_ID
            && data.active_daily
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
    ctx: &Context,
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
                                weekly_submissions: 0,
                                monthly_record: 0,
                                score: 0,
                            },
                        ))
                    }
                })
                .collect(),
            channel_id: None,
            thread_id: None,
            weekly_id: None,
            poll_id: None,
            active_weekly: true,
            active_daily: true,
        };
        for channel in guild.channels.values() {
            if channel.kind == ChannelType::Text {
                data.channel_id = Some(channel.id);
                let guild_id = &guild.id;
                state.database.insert(*guild_id, data.clone());
                initialise_guilds(ctx, guild_id, state).await?;
                let mut message = MessageBuilder::new();
                send_help_message!(ctx, message, bot, channel, channel.id, data.thread_id);
                send_daily_message_with_leaderboard!(
                    ctx,
                    state,
                    guild_id,
                    data,
                    MessageBuilder::new().push_line('\n')
                );
                return Ok(());
            }
        }
        Err("No available channel".into())
    } else {
        Ok(())
    }
}
