use crate::{
    command::{guild_register, hangout, interested, leave},
    database::setup_db,
};
use async_sqlite::rusqlite::OpenFlags;
use async_sqlite::{ClientBuilder, JournalMode};
use poise::Framework;
use poise::FrameworkOptions;
use rootcause::Report;
use rootcause::prelude::*;
use serenity::prelude::*;
use std::env;

mod command;
mod database;

/// Shared application state.
struct AppSate {
    /// Connection to the database.
    pub db: async_sqlite::Client,
}

type CommandError = Box<dyn std::error::Error + Send + Sync>;
type CommandContext<'a> = poise::Context<'a, AppSate, CommandError>;

async fn setup() -> Result<(), Report> {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    // Set gateway intents, which decides what events the bot will be notified about
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let db = ClientBuilder::new()
        .path("./db.sqlite3")
        .journal_mode(JournalMode::Wal)
        .flags(OpenFlags::default())
        .open()
        .await
        .context("Unable to connect or create database")?;

    setup_db(&db).await?;

    let framework = Framework::builder()
        .options(FrameworkOptions {
            commands: vec![guild_register(), hangout(), interested(), leave()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("£".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands)
                    .await
                    .context("Unable to register global commands")?;

                Ok(AppSate { db })
            })
        })
        .build();

    // Create a new instance of the Client, logging in as a bot. This will automatically prepend
    // your bot token with "Bot ", which is a requirement by Discord for bot users.
    let mut client = Client::builder(&token, intents)
        .framework(framework)
        .await
        .context("Error creating client")?;

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform exponential backoff until
    // it reconnects.
    Ok(client.start().await.context("Unable to start client")?)
}

#[tokio::main]
async fn main() {
    if let Err(err) = setup().await {
        eprintln!("{err}\n");
        eprintln!("• Lines with ● are contexts (.context()) - what operation failed");
        eprintln!("• Lines without ● are attachments (.attach()) - debugging data");
    }
}
