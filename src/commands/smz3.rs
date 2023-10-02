use std::str::FromStr;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::builder::CreateEmbed;
use serenity::framework::standard::{Args, CommandResult, macros::{group, command}};
use crate::api::smz3::{RandomizerRequest, GameMode, GanonVulnerable, Goal, KeyShuffle, MorphLocation, OpenTourian, OpenTower, SMLogic, SwordLocation};

#[group]
#[commands(smz3)]
struct Randomizer;


#[command]
#[description = "Generates a SMZ3 seed"]                
#[min_args(0)]
pub async fn smz3(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let mut request = RandomizerRequest::default();
    match parse_args(&mut args, &mut request) {
        Ok(_) => Ok(create_game(ctx, msg, &request).await?),
        Err(error) => {
            msg.channel_id.say(&ctx, format!("Error parsing arguments: {:?}", error)).await?;
            Ok(())
        }
    }    
}

fn parse_args(args: &mut Args, request: &mut RandomizerRequest) -> CommandResult {
    while !args.is_empty() {
        if let Some(arg) = args.current() {
            /* If arg contains a colon, split at it and save the first part as the option, and the second part as the count */
            let split = arg.split_once(':');
            if let Some(split) = split {
                let option = split.0;
                let value = split.1;
                match option {
                    "ganon" => {
                        if let Ok(value_int) = value.parse::<i64>() {
                            request.ganonvulnerable = GanonVulnerable::from_int(value_int);
                        } else {
                            request.ganonvulnerable = GanonVulnerable::from_str(value)?
                        }
                    }                    
                    "goal" => request.goal = Goal::from_str(value)?,
                    "keysanity" => request.keyshuffle = if value.parse::<bool>()? { KeyShuffle::Keysanity } else { KeyShuffle::None },
                    "morph" => request.morphlocation = MorphLocation::from_str(value)?,
                    "tourian" => {
                        if let Ok(value_int) = value.parse::<i64>() {
                            request.opentourian = OpenTourian::from_int(value_int);
                        } else {
                            request.opentourian = OpenTourian::from_str(value)?
                        }
                    },
                    "tower" => {
                        if let Ok(value_int) = value.parse::<i64>() {
                            request.opentower = OpenTower::from_int(value_int);
                        } else {
                            request.opentower = OpenTower::from_str(value)?
                        }
                    }
                    "logic" => request.smlogic = SMLogic::from_str(value)?,
                    "sword" => request.swordlocation = SwordLocation::from_str(value)?,
                    "race" => request.race = value.parse::<bool>()?,
                    "beta" => request.beta = value.parse::<bool>()?,
                    "start" => request.initialitems = Some(value.to_string()),
                    "mode" => request.gamemode = GameMode::from_str(value)?,
                    "names" => request.names = Some(value.split(',').map(|s| s.to_string()).collect()),
                    _ => {}
                }
            }
        }

        args.advance();
    }
    
    Ok(())

}

async fn create_game(ctx: &Context, msg: &Message, request: &RandomizerRequest) -> CommandResult
{
    let mut e = CreateEmbed::default();
    e.title("SMZ3 Randomizer");
    e.description("Generating seed, please wait");

    let mut sent_msg = msg.channel_id.send_message(&ctx, |m| {
        m.set_embed(e.clone())
    }).await?;

    let response = request.send().await;
    if let Err(error) = response {
        e.description(format!("Error generating game: {:?}", error));
        sent_msg.edit(&ctx, |m| m.set_embed(e)).await?;
        return Ok(());
    } else {
        e.description("Game generated successfully!");
        e.field("Permalink", response.unwrap().permalink(), false);
        sent_msg.edit(&ctx, |m| m.set_embed(e)).await?;
    }

    Ok(())
}