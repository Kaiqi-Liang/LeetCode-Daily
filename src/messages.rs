#[macro_export]
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

#[macro_export]
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
        .push_line("\
* `/help`: Shows this help message
* `/random [free | paid | easy | medium | hard] ...`: Send a random question with optional fields to filter by difficulty or whether it is subscription only, if not run in a thread it will create a thread for it
* `/scores`: Shows the current leaderboard
* `/top [number]`: Shows the top 3 or any number up to 10 scores and monthly records across all servers
* `/poll`: Start a poll for today's submissions or reply to an existing one if it has already started, has to be run in the current daily thread
* `/active [weekly|daily] [toggle]`: Check whether some features of the bot are currently active or toggle them on and off
        \n")
        .push("To share your code you have to put it in a spoiler tag and wrap it with ")
        .push_safe("```code```")
        .push_line(" so others can't immediately see your solution. You can start from the template below and replace the language and code with your own. If you didn't follow the format strictly simply send it again")
        ).build()).await?
    };
}

#[macro_export]
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

#[macro_export]
macro_rules! construct_badge_message {
    ($message:expr, $month:expr) => {
        $message.push_line(format!(
            " for earning the {:?} Daily Challenge badge!",
            Month::try_from(TryInto::<u8>::try_into($month.month())?)?
        ))
    };
}

#[macro_export]
macro_rules! send_daily_message_with_leaderboard {
    ($ctx:ident, $state:expr, $guild_id:ident, $data:ident, $message:expr) => {
        let channel_id = get_channel_from_guild!($data);
        $data.poll_id = None;
        let message_id = send_leetcode_daily_question_message($ctx, channel_id)
            .await?
            .id;
        create_thread_from_message!(
            $ctx,
            $state,
            $guild_id,
            $data,
            $message,
            channel_id,
            message_id,
            $data.thread_id,
            Utc::now().format("%d/%m/%Y").to_string()
        )
    };
}

#[macro_export]
macro_rules! send_invalid_channel_id_message {
    ($ctx:ident, $msg:ident) => {
        $msg.channel_id
            .say(&$ctx.http, "Invalid channel ID")
            .await?;
    };
}

#[macro_export]
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

#[macro_export]
macro_rules! create_thread_from_message {
    ($ctx:ident, $channel_id:ident, $message_id:ident, $thread_name:expr) => {
        $channel_id
            .create_thread_from_message(
                &$ctx.http,
                $message_id,
                CreateThread::new($thread_name)
                    .kind(ChannelType::PublicThread)
                    .auto_archive_duration(AutoArchiveDuration::OneDay),
            )
            .await
            .map(|channel| channel.id)
            .ok()
    };
    ($ctx:ident, $state:expr, $guild_id:ident, $data:ident, $message:expr, $channel_id:ident, $message_id:ident, $thread_id:expr, $thread_name:expr) => {
        $thread_id = create_thread_from_message!($ctx, $channel_id, $message_id, $thread_name);
        send_message_with_leaderboard!(
            $ctx,
            &mut $state.guilds,
            $guild_id,
            $thread_id.ok_or("Failed to create thread")?,
            &$data.users,
            construct_format_message!(
                $message.push_line("Share your solution in the format below to earn points")
            )
            .push_line('\n')
        )
    };
}

#[macro_export]
macro_rules! construct_congrats_message {
    ($message:expr, $state:ident, $guild_id:ident, $user_id:ident) => {
        $message
            .push("Congrats to ")
            .mention(get_user_from_id!($state.guilds, $guild_id, $user_id))
            .push(" for ")
    };
}

#[macro_export]
macro_rules! construct_summary_message {
    ($message:expr, $user:ident) => {
        $message
            .push("\nYour current score is ")
            .push_bold($user.score.to_string())
            .push(". This month you have completed ")
            .push_bold($user.monthly_record.to_string())
            .push_line(if $user.monthly_record > 1 {
                " questions"
            } else {
                " question"
            });
    };
}

#[macro_export]
macro_rules! construct_reward_message {
    ($message:expr, $reward:expr) => {
        $message
            .push(" You have been rewarded ")
            .push_bold($reward.to_string())
            .push(if $reward > 1 { " points" } else { " point" })
    };
}

#[macro_export]
macro_rules! construct_thread_message {
    ($message:expr, $thread:expr) => {
        if let Some(thread_id) = $thread {
            $message.push("Today's thread is ").channel(thread_id)
        } else {
            $message.push("Daily is not active")
        }
    };
}

#[macro_export]
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

#[macro_export]
macro_rules! construct_active_message {
    ($message:ident, $active:ident, $arg:expr, $bot:ident, $now:expr) => {
        Some($message.mention(&$bot).push(format!(
            " is {}{} for {}",
            if $now { "now " } else { "" },
            if *$active { "active" } else { "paused" },
            $arg
        )))
    };
}
