mod helper;
mod leetcode;
mod messages;
use chrono::{Datelike, Month, TimeDelta, TimeZone, Utc, Weekday};
use leetcode::{send_leetcode_daily_question_message, send_random_leetcode_question_message};
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
type UserInfo = HashMap<UserId, Status>;
pub type Database = HashMap<GuildId, Data>;

#[derive(Default, Serialize, Deserialize, Clone)]
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
    days_missed: u32,
    score: usize,
}

pub struct SharedState {
    pub ready: bool,
    pub guilds: Guilds,
    pub file: File,
    pub database: Database,
}

pub struct State;
impl TypeMapKey for State {
    type Value = SharedState;
}

const CUSTOM_ID: &str = "favourite_submission";
const POLL_ERROR_MESSAGE: &str = "Poll message is not in this channel";
const NUM_SECS_IN_AN_HOUR: u64 = chrono::Duration::minutes(60).num_seconds() as _;
const NUM_MONTHS_IN_A_YEAR: u32 = 12;

pub async fn save_to_database(ctx: Context) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    write_to_database!(state);
    Ok(())
}

fn construct_leaderboard<'a>(
    users: &UserInfo,
    guilds: &mut Guilds,
    guild_id: &GuildId,
    message: &'a mut MessageBuilder,
) -> &'a mut MessageBuilder {
    message.push_line("The current leaderboard:");
    let mut leaderboard = users
        .iter()
        .map(|(id, status)| {
            (
                get_user_from_id!(guilds, guild_id, id).clone(),
                status.score,
                status.monthly_record,
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
    for (place, (user, score, monthly_record)) in leaderboard.into_iter().enumerate() {
        if score > 0 {
            has_score = true;
            message
                .push_line(format!("{}. {}", place + 1, user.name))
                .push_bold(format!("\t{score} "))
                .push_line(if score > 1 { "points" } else { "point" })
                .push_bold(format!("\t{monthly_record} "))
                .push(if monthly_record > 1 {
                    "questions"
                } else {
                    "question"
                })
                .push_line(" completed this month");
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
    let curr_month = now.month();
    let this_month = Utc
        .with_ymd_and_hms(now.year(), curr_month, 1, 0, 0, 0)
        .unwrap();
    let next_month = Utc
        .with_ymd_and_hms(
            now.year() + (curr_month / NUM_MONTHS_IN_A_YEAR) as i32,
            (curr_month + 1) % NUM_MONTHS_IN_A_YEAR,
            1,
            0,
            0,
            0,
        )
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
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if state.ready {
        Err("Already done setup".into())
    } else {
        state.ready = true;
        let guilds = ready.guilds;
        log!("Setting up guilds {guilds:?}");
        for guild in guilds {
            if state.database.contains_key(&guild.id) {
                initialise_guilds(ctx, &guild.id, state).await?;
            }
        }
        Ok(())
    }
}

pub async fn schedule_daily_question(ctx: &Context) -> Result<(), Box<dyn Error>> {
    loop {
        let mut duration: u64 = time_till_utc_midnight()?.num_seconds().try_into()?;
        log!("{duration} seconds until next daily");
        if duration > NUM_SECS_IN_AN_HOUR {
            sleep(Duration::from_secs(duration - NUM_SECS_IN_AN_HOUR)).await;
            duration = NUM_SECS_IN_AN_HOUR;
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);

            for (guild_id, data) in state.database.iter_mut() {
                if !data.active_daily {
                    continue;
                }
                data.poll_id = Some(poll(ctx, data, &mut state.guilds, guild_id).await?.id);
                if data.poll_id.is_some() {
                    send_message_with_leaderboard!(
                        ctx,
                        &mut state.guilds,
                        guild_id,
                        get_thread_from_guild!(data),
                        &data.users,
                        MessageBuilder::new().push_line("An hour left to make your submission for today's question if you haven't already\n")
                    );
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
            let mut penalties = 0;
            let mut votes = HashMap::new();
            if Utc::now().day0() == 0 {
                if let Some(status) = data
                    .users
                    .values()
                    .max_by_key(|status| status.monthly_record)
                {
                    let highest_monthly_record = status.monthly_record;
                    if highest_monthly_record > 0 {
                        message.push("Welcome to a new month! Last month ");
                        let last_month =
                            Utc::now().date_naive().pred_opt().ok_or("Invalid date")?;
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
                        construct_reward_message!(
                            message
                                .push(" completed ")
                                .push_bold(highest_monthly_record.to_string())
                                .push(" questions which is the highest in this server!"),
                            5
                        );
                        if highest_monthly_record == last_month.day() {
                            construct_badge_message!(
                                message.push(", and another 10 points"),
                                last_month
                            );
                        } else {
                            message.push_line("");
                        }
                        message.push_line("");
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
                    user.days_missed += 1;
                    if user.score > 0 {
                        user.score -= 1;
                    }
                } else {
                    user.submitted = None;
                }
                user.voted_for = None;
            }
            message.push("Yesterday ")
                .push_line(if penalties > 0 {
                    format!("{penalties} {} did not complete the challenge 😭 each lost 1 point as a penalty", if penalties > 1 { "people" } else { "person" })
                } else {
                    "everyone completed the challenge! Awesome job to start a new day!".to_string()
                })
                .push_line("\nThe number of votes received:");
            let mut votes = votes.iter().collect::<Vec<_>>();
            if votes.is_empty() {
                message.push_line("No one voted 😞");
            } else {
                votes.sort_by(|a, b| b.1.cmp(a.1));
                for (place, (user_id, &votes)) in votes.into_iter().enumerate() {
                    let user = get_user_from_id!(state.guilds, guild_id, user_id);
                    get_user_from_id!(data.users, *user_id).score += votes;
                    message
                        .push((place + 1).to_string())
                        .push(". ")
                        .mention(user)
                        .push(": ")
                        .push_bold(votes.to_string())
                        .push_line("");
                }
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
        log!("{num_days_from_sunday} days / {duration:?} until next contest");
        sleep(duration).await;
        {
            let mut data = ctx.data.write().await;
            let state = get_shared_state!(data);
            for (guild_id, data) in state.database.iter_mut() {
                if data.active_weekly {
                    let channel_id = get_channel_from_guild!(data);
                    let message_id = channel_id.say(ctx.clone(),
                        MessageBuilder::new()
                            .push_named_link("Weekly Contest", "https://leetcode.com/contest/")
                            .push(" starting now! The first 3 people to finish all 4 questions will get bonus points @everyone")
                            .build())
                        .await?
                        .id;
                    create_thread_from_message!(
                        ctx,
                        state,
                        guild_id,
                        data,
                        MessageBuilder::new(),
                        channel_id,
                        message_id,
                        data.weekly_id,
                        format!("Week {}", Utc::now().iso_week().week0())
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
        for (guild_id, guild) in state.database.iter_mut() {
            let mut message = MessageBuilder::new();
            message.push_line("Weekly contest just ended, the results are:");
            let mut submissions = guild
                .users
                .iter()
                .filter_map(|(user_id, status)| {
                    if status.weekly_submissions == 0 {
                        None
                    } else {
                        Some((
                            get_user_from_id!(state.guilds, guild_id, user_id).clone(),
                            status.weekly_submissions,
                        ))
                    }
                })
                .collect::<Vec<_>>();
            if submissions.is_empty() {
                message.push_line("No one participated in the contest 😩");
            } else {
                submissions.sort_by(|a, b| b.1.cmp(&a.1));
                for (place, (user, submission)) in submissions.into_iter().enumerate() {
                    message
                        .push((place + 1).to_string())
                        .push(". ")
                        .mention(&user)
                        .push(" completed ")
                        .push_bold(submission.to_string())
                        .push_line(if submission > 1 {
                            " questions"
                        } else {
                            " question"
                        });
                }
            }
            for user in guild.users.values_mut() {
                user.weekly_submissions = 0;
            }
            if let Some(weekly_id) = guild.weekly_id {
                send_message_with_leaderboard!(
                    ctx,
                    &mut state.guilds,
                    guild_id,
                    weekly_id,
                    &guild.users,
                    message.push_line("")
                );
            }
            guild.weekly_id = None;
        }
        write_to_database!(state);
    }
}

pub async fn respond(
    ctx: &Context,
    msg: Message,
    current_user_id: UserId,
) -> Result<(), Box<dyn Error>> {
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
                                Some(message.mention(&current_user_id).push(
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
                            if args[0] != "/active" || (args[1] != "weekly" && args[1] != "daily") {
                                None
                            } else {
                                let active = get_active!(data, args[1]);
                                construct_active_message!(
                                    message,
                                    active,
                                    args[1],
                                    current_user_id,
                                    false
                                )
                            }
                        }
                        3 => {
                            if args[0] != "/active" && args[2] != "toggle" {
                                None
                            } else if args[1] == "weekly" || args[1] == "daily" {
                                let active = get_active!(data, args[1]);
                                *active = !*active;
                                if args[1] == "daily" && *active && data.thread_id.is_none() {
                                    send_daily_message_with_leaderboard!(
                                        ctx,
                                        state,
                                        guild_id,
                                        data,
                                        MessageBuilder::new()
                                    );
                                }
                                construct_active_message!(
                                    message,
                                    active,
                                    args[1],
                                    current_user_id,
                                    true
                                )
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
            send_help_message!(
                ctx,
                message,
                current_user_id,
                msg.channel_id,
                channel,
                data.thread_id
            );
        } else if msg.content == "/reset" {
            let channel_id = data.channel_id;
            **data = default_data(data.users.keys().copied().collect::<Vec<_>>());
            data.channel_id = channel_id;
            msg.channel_id.say(ctx, "Database has been reset").await?;
        } else if msg.content == "/daily" {
            for status in data.users.values_mut() {
                status.submitted = None;
            }
            send_daily_message_with_leaderboard!(ctx, state, guild_id, data, message);
        } else if msg.content.starts_with("/random") {
            send_random_leetcode_question_message(
                ctx,
                msg.channel_id,
                msg.content.split(' ').skip(1).collect::<Vec<_>>(),
            )
            .await?;
        } else if msg.content.starts_with("/top") {
            let top = msg.content.split(' ').nth(1);
            let mut leaderboard = Vec::new();
            for data in state.database.values() {
                leaderboard.append(
                    &mut data
                        .users
                        .values()
                        .map(|users| (users.score, users.monthly_record))
                        .collect::<Vec<_>>(),
                );
            }
            leaderboard.sort_by(|a, b| {
                let cmp = b.0.cmp(&a.0);
                if let Ordering::Equal = cmp {
                    b.1.cmp(&a.1)
                } else {
                    cmp
                }
            });
            let mut has_score = false;
            for (place, (score, monthly_record)) in leaderboard
                .into_iter()
                .take(top.unwrap_or("3").parse::<usize>()?.min(10))
                .enumerate()
            {
                if score > 0 || monthly_record > 0 {
                    has_score = true;
                    message
                        .push(format!("{}. ", place + 1))
                        .push_bold(score.to_string())
                        .push(if score > 1 {
                            " points and "
                        } else {
                            " point and "
                        })
                        .push_bold(monthly_record.to_string())
                        .push(if monthly_record > 1 {
                            " questions"
                        } else {
                            " question"
                        })
                        .push_line(" completed this month");
                }
            }
            if !has_score {
                message.push("No one has done any questions yet");
            }
            msg.channel_id.say(ctx, message.build()).await?;
        } else if msg.content == "/scores" {
            send_message_with_leaderboard!(
                ctx,
                &mut state.guilds,
                guild_id,
                msg.channel_id,
                &data.users,
                message
            );
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
            } else if msg.channel_id != channel {
                msg.channel_id
                    .say(
                        &ctx.http,
                        construct_channel_message!(
                            message,
                            current_user_id,
                            channel,
                            data.thread_id
                        )
                        .build(),
                    )
                    .await?;
            } else {
                send_channel_usage_message!(ctx, msg.channel_id);
            }
        } else if code_block.is_match(&msg.content) {
            if data.active_daily && msg.channel_id == data.thread_id.unwrap_or_default() {
                let user = get_user_from_id!(data.users, *user_id);
                if user.submitted.is_none() {
                    user.submitted = Some(msg.link());
                    let hour = time_till_utc_midnight()?.num_hours();
                    let mut score: usize = if hour == 23 {
                        5
                    } else if hour >= 21 {
                        4
                    } else if hour >= 16 {
                        3
                    } else if hour >= 8 {
                        2
                    } else {
                        1
                    };
                    user.monthly_record += 1;
                    construct_summary_message!(
                        construct_reward_message!(
                            construct_congrats_message!(message, state, guild_id, user_id).push(
                                format!(
                                    "completing today's challenge{}!",
                                    if user.days_missed > 7 {
                                        score += 5;
                                        user.score += score;
                                        format!(
                                            " and welcome back after missing it for {} days",
                                            user.days_missed
                                        )
                                    } else {
                                        user.score += score;
                                        String::new()
                                    }
                                )
                            ),
                            score
                        ),
                        user
                    );
                    user.days_missed = 0;
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
                                Some(get_user_from_id!(state.guilds, guild_id, id).clone())
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
                                    &mut state.guilds,
                                    guild_id,
                                )),
                            )
                            .await?;
                    }
                    if users_not_yet_completed.is_empty() {
                        message
                            .push("Everyone has finished today's challenge, let's Grow Together!");
                    }
                    data.poll_id = Some(poll(ctx, data, &mut state.guilds, guild_id).await?.id);
                    msg.channel_id.say(&ctx.http, message.build()).await?;
                }
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
                        0 => (4, "1st"),
                        1 => (3, "2nd"),
                        2 => (2, "3rd"),
                        _ => (1, "after top 3"),
                    };
                    let user = get_user_from_id!(data.users, *user_id);
                    if user.weekly_submissions < 4 {
                        user.weekly_submissions += 1;
                        let (score, result, bold_text, end) = if user.weekly_submissions == 4 {
                            (reward, "coming ", String::from(place), "")
                        } else {
                            (
                                1,
                                "finishing question ",
                                user.weekly_submissions.to_string(),
                                "/4",
                            )
                        };
                        user.score += score;
                        construct_reward_message!(
                            construct_congrats_message!(message, state, guild_id, user_id)
                                .push(result)
                                .push_bold(bold_text)
                                .push(format!("{end} in the contest!")),
                            score
                        )
                        .push(". Your current score is ")
                        .push_bold(user.score.to_string());
                        weekly_id.say(&ctx.http, message.build()).await?;
                    }
                }
            }
        } else if let Some(thread) = data.thread_id {
            if channel == msg.channel_id {
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
            } else if msg.content == "/poll" && msg.channel_id == thread && data.active_daily {
                data.poll_id = Some(poll(ctx, data, &mut state.guilds, guild_id).await?.id);
            }
        }
    } else {
        msg.channel_id
            .say(&ctx.http, "Please don't slide into my dm 😜")
            .await?;
    }
    Ok(())
}

fn build_submission_message(guild: &Data, guilds: &mut Guilds, guild_id: &GuildId) -> String {
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
    guilds: &mut Guilds,
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
            thread.say(&ctx.http, POLL_ERROR_MESSAGE).await?;
            Err(POLL_ERROR_MESSAGE.into())
        }
    } else {
        let message = thread
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
            .await?;
        message.pin(&ctx.http).await?;
        Ok(message)
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
                        "Successfully voted for {}",
                        get_user_from_id!(state.guilds, guild_id, voted_for)
                    )
                );
            }
        }
    }
    Ok(())
}

fn default_data(users: Vec<UserId>) -> Data {
    Data {
        users: users
            .iter()
            .map(|&user_id| (user_id, Status::default()))
            .collect(),
        active_daily: true,
        active_weekly: true,
        ..Default::default()
    }
}

pub async fn initialise_guild(
    ctx: &Context,
    guild: Guild,
    current_user_id: UserId,
) -> Result<(), Box<dyn Error>> {
    let mut data = ctx.data.write().await;
    let state = get_shared_state!(data);
    if !state.database.contains_key(&guild.id) {
        let mut data = default_data(
            guild
                .id
                .members(&ctx.http, None, None)
                .await?
                .into_iter()
                .filter_map(|member| {
                    let user = member.user;
                    if user.bot {
                        None
                    } else {
                        Some(user.id)
                    }
                })
                .collect::<Vec<_>>(),
        );
        for channel in guild.channels.values() {
            if channel.kind == ChannelType::Text {
                data.channel_id = Some(channel.id);
                let guild_id = &guild.id;
                initialise_guilds(ctx, guild_id, state).await?;
                let mut message = MessageBuilder::new();
                send_help_message!(
                    ctx,
                    message,
                    current_user_id,
                    channel,
                    channel.id,
                    data.thread_id
                );
                send_daily_message_with_leaderboard!(
                    ctx,
                    state,
                    guild_id,
                    data,
                    MessageBuilder::new()
                );
                send_random_leetcode_question_message(ctx, channel.id, vec![]).await?;
                state.database.insert(*guild_id, data);
                return Ok(());
            }
        }
        Err("No available channel".into())
    } else {
        Ok(())
    }
}
