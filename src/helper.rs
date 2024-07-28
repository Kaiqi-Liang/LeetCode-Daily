#[macro_export]
macro_rules! save_to_database {
    ($ctx:ident) => {
        if let Err(why) = save_to_database($ctx).await {
            log!("Error saving to database: {why}");
        }
    };
}

#[macro_export]
macro_rules! schedule_thread {
    ($ctx:ident, $schedule:ident) => {{
        let ctx = $ctx.clone();
        spawn(async move {
            if let Err(why) = $schedule(&ctx).await {
                log!("Error scheduling: {why}");
            }
        });
    }};
}

#[macro_export]
macro_rules! log {
    ($message:expr) => {
        println!("[{}] {}", Utc::now(), format!($message));
    };
}

#[macro_export]
macro_rules! get_channel_from_guild {
    ($guild:expr) => {
        $guild.channel_id.ok_or("No default channel")?
    };
}

#[macro_export]
macro_rules! get_thread_from_guild {
    ($guild:expr) => {
        $guild.thread_id.ok_or("No default thread")?
    };
}

#[macro_export]
macro_rules! write_to_database {
    ($state:ident) => {
        $state.file.seek(SeekFrom::Start(0))?;
        $state.file.set_len(0)?;
        $state
            .file
            .write_all(serde_json::to_string_pretty(&$state.database)?.as_bytes())?
    };
}

#[macro_export]
macro_rules! get_shared_state {
    ($data:ident) => {{
        $data
            .get_mut::<State>()
            .ok_or("Failed to get share data from context")?
    }};
}

#[macro_export]
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

#[macro_export]
macro_rules! get_guild_from_id {
    ($state:ident, $guild_id:ident) => {
        &mut $state
            .database
            .get_mut(&$guild_id)
            .ok_or("Guild does not exist in database")?
    };
}

#[macro_export]
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

#[macro_export]
macro_rules! get_active {
    ($data:ident, $arg:expr) => {
        if $arg == "weekly" {
            &mut $data.active_weekly
        } else {
            &mut $data.active_daily
        }
    };
}
