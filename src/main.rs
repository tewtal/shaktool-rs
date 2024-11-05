use std::{collections::{HashMap, HashSet}, env, sync::Arc};
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

pub struct ShardManagerContainer;

impl TypeMapKey for ShardManagerContainer {
    type Value = Arc<Mutex<serenity::ShardManager>>;
}

struct Handler;

#[poise::async_trait]
impl poise::EventHandler for Handler {
    async fn interaction_create(&self, ctx: serenity::Context, interaction: serenity::Interaction) {
        // Return if handler returns true to skip further processing since this interaction has been handled already
        if interactions::multiworld::interaction_create_multiworld(&ctx, &interaction).await.unwrap_or(false) { return; }
    }

    async fn ready(&self, ctx: serenity::Context, ready: serenity::Ready) {
        info!("Connected as {}", ready.user.name);
        // let a = Activity::streaming("VGM", "https://twitch.tv/fmfunk");
        // let _ = ctx.set_activity(a).await;
        
        let _ = serenity::Command::set_global_application_commands(&ctx.http, |commands| {
            commands
                // Multiworld command is disabled for now - was only used for testing stuff
                //.create_application_command(interactions::multiworld::create_multiworld_command)
        }).await;
    }

    async fn resume(&self, _: serenity::Context, _: serenity::ResumedEvent) {
        info!("Resumed");
    }

    async fn reaction_add(&self, _ctx: serenity::Context, _reaction: serenity::Reaction) {
    }

    async fn reaction_remove(&self, _ctx: serenity::Context, _reaction: serenity::Reaction) {
    }
}



#[poise::command]
#[individual_command_tip = "To get help with an individual command, pass its name as an argument to this command."]
#[strikethrough_commands_tip_in_guild = " "]
#[strikethrough_commands_tip_in_dm = " "]
#[lacking_permissions = "Hide"]
#[lacking_role = "Hide"]
#[wrong_channel = "Strike"]
async fn my_help(
    context: &serenity::Context,
    msg: &serenity::Message,
    args: poise::Args,
    help_options: &'static poise::HelpOptions,
    groups: &[&'static poise::CommandGroup],
    owners: HashSet<serenity::UserId>,
) -> poise::CommandResult {
    let _ = poise::help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}


#[poise::hook]
async fn dispatch_error(ctx: &serenity::Context, msg: &serenity::Message, error: poise::DispatchError, _command_name: &str) {
    if let poise::DispatchError::Ratelimited(info) = error {
        // We notify them only once.
        if info.is_first_try {
            let _ = msg
                .channel_id
                .say(&ctx, &format!("Try this again in {} seconds.", info.as_secs()))
                .await;
        }
    }
}


#[poise::hook]
async fn normal_message_hook(ctx: &serenity::Context, msg: &serenity::Message) {    
    // Call the COBE message handler
    if let Err(e) = util::cobe::message_hook(ctx, msg).await {
        debug!("Cobe message handler error: {:?}", e);
    }
    
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
                owners.insert(info.owner.id);
            }
            match http.get_current_user().await {
                Ok(bot_id) => (owners, bot_id.id),
                Err(why) => panic!("Could not access the bot id: {:?}", why),
            }
        },
        Err(why) => panic!("Could not access application info: {:?}", why),
    };

    let framework = Framework::new()
        .configure(|c| c
            .with_whitespace(true)
            .on_mention(Some(bot_id))
            .prefix(env::var("COMMAND_PREFIX").unwrap_or_else(|_| "%".to_string()))
            .owners(owners))
        .on_dispatch_error(dispatch_error)
        .normal_message(normal_message_hook)
        .help(&MY_HELP)
        .group(&GENERAL_GROUP)
        .group(&LEADERBOARD_GROUP)
        .group(&RANDOMIZER_GROUP);

    let application_id: u64 = env::var("APPLICATION_ID")
        .expect("Expected an application id in the environment")
        .parse()
        .expect("application id is not a valid id");

    let intents = serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
    let mut client = serenity::Client::builder(&token, intents)
        .framework(framework)
        .event_handler(Handler)
        .application_id(application_id)
        .await
        .expect("Error creating client");
    
    {
        let mut data = client.data.write().await;
        data.insert::<ShardManagerContainer>(client.shard_manager.clone());
        data.insert::<interactions::multiworld::MultiworldSessionKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<interactions::multiworld::MultiworldSettingsSessionKey>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<Cobe>(Arc::new(Mutex::new(Cobe::new())));
    }

    let shard_manager = client.shard_manager.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Could not register ctrl+c handler");
        shard_manager.lock().await.shutdown_all().await;
    });

    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
    }
}
