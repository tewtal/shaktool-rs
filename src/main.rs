use std::{collections::HashSet, env, sync::Arc};
use ::serenity::all::ApplicationId;
use tokio::sync::Mutex;
use poise::serenity_prelude as serenity;
use tracing::{error, info, debug};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

mod util;
mod commands;
mod api;
mod interactions;

use crate::util::cobe::Cobe;

// Types used by all command functions
type Error = Box<dyn std::error::Error + Send + Sync>;
#[allow(unused)]
type Context<'a> = poise::Context<'a, Data, Error>;

// Custom user data passed to all command functions
pub struct Data {
    pub cobe: Arc<Mutex<Cobe>>,
}

async fn event_handler(ctx: &serenity::Context, event: &serenity::FullEvent, _framework: poise::FrameworkContext<'_, Data, Error>, data: &Data) -> Result<(), Error> 
{
    match event {
        serenity::FullEvent::InteractionCreate { interaction: _ } => {
            // Return if handler returns true to skip further processing since this interaction has been handled already
            //interactions::multiworld::interaction_create_multiworld(ctx, interaction).await.unwrap_or(false)
        },
        serenity::FullEvent::Ready { data_about_bot, .. } => {
            info!("Connected as {}", data_about_bot.user.name);
        },
        serenity::FullEvent::Message { new_message, .. } => {
            if !new_message.author.bot && new_message.mentions_me(ctx).await? {
                if let Err(e) = util::cobe::message_hook(ctx, new_message, data).await {
                    debug!("Cobe message handler error: {:?}", e);
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

    let (_owners, _bot_id) = match http.get_current_application_info().await {
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
            commands.extend(commands::smz3::smz3_commands());
            commands.extend(commands::leaderboard::leaderboard_commands());
            commands.extend(vec![commands::time::time()]);
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
        .setup(|_ctx, _ready, _framework| {
            Box::pin(async move {
                let data = Data {
                    cobe: Arc::new(Mutex::new(Cobe::new())),
                };
                Ok(data)
            })
        })
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
    
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Could not register ctrl+c handler");
    });

    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}
