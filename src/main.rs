use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use serenity::model::prelude::*;
use poise::serenity_prelude as serenity;

use tracing::{error, info, debug};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

mod util;
mod commands;
mod api;
mod interactions;
mod db;
mod tasks;

use crate::util::cobe::Cobe;

pub struct Data {
    pub cobe: Arc<Mutex<Cobe>>,
    pub db: db::Db,
    pub multiworld_sessions: Arc<RwLock<HashMap<MessageId, interactions::multiworld::MultiworldSession>>>,
    pub multiworld_settings: Arc<RwLock<HashMap<MessageId, MessageId>>>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!("Error in command '{}': {:?}", ctx.command().name, error);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                error!("Error while handling error: {}", e);
            }
        }
    }
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    match event {
        serenity::FullEvent::Ready { data_about_bot } => {
            info!("Connected as {}", data_about_bot.user.name);
        }
        serenity::FullEvent::InteractionCreate { interaction } => {
            interactions::multiworld::interaction_create_multiworld(ctx, interaction, data).await?;
            interactions::speedrun::interaction_create_speedrun(ctx, interaction, data).await?;
        }
        serenity::FullEvent::Message { new_message } => {
            if let Err(e) = util::cobe::message_hook(ctx, new_message, data).await {
                debug!("Cobe message handler error: {:?}", e);
            }
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().expect("Failed to load .env-file");

    let subscriber = FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    tracing::subscriber::set_global_default(subscriber).expect("Failed to start the logger");

    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
    let prefix = env::var("COMMAND_PREFIX").unwrap_or_else(|_| "%".to_string());
    let db_path = env::var("DATABASE_PATH").unwrap_or_else(|_| "shaktool.db".to_string());

    let db = db::Db::connect(&db_path).await.expect("Failed to open the database");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::general::help(),
                commands::general::version(),
                commands::general::strat(),
                commands::general::wiki(),
                commands::general::card(),
                commands::time::time(),
                commands::leaderboard::top(),
                commands::leaderboard::records(),
                commands::quad::quad(),
                commands::quad::quad_options(),
                commands::smz3::smz3(),
                commands::config::config(),
                commands::speedrun::speedrun(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(prefix),
                ..Default::default()
            },
            on_error: |error| Box::pin(on_error(error)),
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            let ctx = ctx.clone();
            Box::pin(async move {
                let mut slash_commands =
                    poise::builtins::create_application_commands(&framework.options().commands);
                slash_commands.push(interactions::multiworld::create_multiworld_command());
                match env::var("GUILD_ID").ok().and_then(|id| id.parse::<u64>().ok()) {
                    // Guild commands register instantly, so use one for development.
                    Some(guild_id) => {
                        serenity::GuildId::new(guild_id)
                            .set_commands(&ctx.http, slash_commands)
                            .await?;
                        info!("Registered slash commands to guild {}", guild_id);
                    }
                    // Global commands can take up to an hour to propagate.
                    None => {
                        serenity::Command::set_global_commands(&ctx.http, slash_commands).await?;
                        info!("Registered global slash commands");
                    }
                }
                tasks::start(ctx, db.clone());
                Ok(Data {
                    cobe: Arc::new(Mutex::new(Cobe::new())),
                    db,
                    multiworld_sessions: Arc::new(RwLock::new(HashMap::new())),
                    multiworld_settings: Arc::new(RwLock::new(HashMap::new())),
                })
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::ClientBuilder::new(&token, intents)
        .framework(framework)
        .await
        .expect("Error creating client");

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Could not register ctrl+c handler");
        shard_manager.shutdown_all().await;
    });

    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}
