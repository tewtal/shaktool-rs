use std::{collections::{HashMap, HashSet}, env, sync::{atomic::AtomicU32, Arc}};
use ::serenity::{all::ApplicationId, prelude::TypeMapKey};
use tokio::sync::{Mutex, RwLock};
use poise::serenity_prelude as serenity;
use poise::Framework;
use tracing::{error, info, debug};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

mod util;
mod commands;
mod api;
mod interactions;

use commands::general::*;
use commands::leaderboard::*;
use commands::smz3::*;

use crate::util::cobe::Cobe;

// Types used by all command functions
type Error = Box<dyn std::error::Error + Send + Sync>;
#[allow(unused)]
type Context<'a> = poise::Context<'a, Data, Error>;

// Custom user data passed to all command functions
pub struct Data {
    poise_mentions: AtomicU32,
}

async fn event_handler(ctx: &serenity::Context, event: &serenity::FullEvent, _framework: poise::FrameworkContext<'_, Data, Error>, data: &Data) -> Result<(), Error> 
{
    match event {
        serenity::FullEvent::InteractionCreate { interaction } => {
            // Return if handler returns true to skip further processing since this interaction has been handled already
            //interactions::multiworld::interaction_create_multiworld(ctx, interaction).await.unwrap_or(false)
        },
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            info!("Connected as {}", data_about_bot.user.name);
        },
        serenity::FullEvent::Message { new_message, .. } => {
            if !new_message.author.bot {
                if new_message.content.contains("<@") {
                    if let Err(e) = util::cobe::message_hook(ctx, new_message).await {
                        debug!("Cobe message handler error: {:?}", e);
                    }
                }
            }
        },
        _ => {}
    }
    Ok(())
}


#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().expect("Failed to load .env-file");

    let subscriber = FmtSubscriber::builder().with_env_filter(EnvFilter::from_default_env()).finish();
    tracing::subscriber::set_global_default(subscriber).expect("Failed to start the logger");
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let http = serenity::Http::new(&token);

    let (owners, bot_id) = match http.get_current_application_info().await {
        Ok(info) => {
            let mut owners = HashSet::new();
            if let Some(team) = info.team {
                owners.insert(team.owner_user_id);
            } else {
                owners.insert(info.owner.unwrap().id);
            }
            match http.get_current_user().await {
                Ok(bot_id) => (owners, bot_id.id),
                Err(why) => panic!("Could not access the bot id: {:?}", why),
            }
        },
        Err(why) => panic!("Could not access application info: {:?}", why),
    };

    let options = poise::FrameworkOptions {
        commands: {
            let mut commands = vec![help()];
            commands.extend(commands::general::general_commands());
            commands
        },
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(env::var("COMMAND_PREFIX").unwrap_or_else(|_| "%".to_string())),
            case_insensitive_commands: true,
            ..Default::default()
        },
        event_handler: |ctx, event, framework, data| {
            Box::pin(event_handler(ctx, event, framework, data))
        },
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        .build();

    let application_id: ApplicationId = env::var("APPLICATION_ID")
        .expect("Expected an application id in the environment")
        .parse()
        .expect("application id is not a valid id");

    let intents = serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
    
    let mut client = serenity::Client::builder(&token, intents)
        .framework(framework)
        .application_id(application_id)
        .await
        .expect("Error creating client");
    
    {
        let mut data = client.data.write().await;
        data.insert::<interactions::multiworld::MultiworldSessionKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<interactions::multiworld::MultiworldSettingsSessionKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<Cobe>(Arc::new(Mutex::new(Cobe::new())));
    }

    let shard_manager = client.shard_manager.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Could not register ctrl+c handler");
    });

    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}
