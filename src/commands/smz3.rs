use std::str::FromStr;
use poise::serenity_prelude as serenity;
use poise::command;
use crate::api::smz3::{RandomizerRequest, GameMode, GanonVulnerable, Goal, KeyShuffle, MorphLocation, OpenTourian, OpenTower, SMLogic, SwordLocation};

#[command(slash_command)]
#[description = "Generates a SMZ3 seed"]
pub async fn smz3(ctx: poise::Context<'_>, args: String) -> Result<(), serenity::Error> {
    let mut request = RandomizerRequest::default();
    match parse_args(&args, &mut request) {
        Ok(_) => Ok(create_game(ctx, &request).await?),
        Err(error) => {
            ctx.say(format!("Error parsing arguments: {:?}", error)).await?;
            Ok(())
        }
    }
}

fn parse_args(args: &str, request: &mut RandomizerRequest) -> Result<(), Box<dyn std::error::Error>> {
    for arg in args.split_whitespace() {
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
                "gamemode" => request.gamemode = GameMode::from_str(value)?,
                "names" => request.names = Some(value.split(',').map(|s| s.to_string()).collect()),
                _ => {}
            }
        }
    }
    Ok(())
}

async fn create_game(ctx: poise::Context<'_>, request: &RandomizerRequest) -> Result<(), serenity::Error> {
    let mut e = serenity::CreateEmbed::default();
    e.title("SMZ3 Randomizer");
    e.description("Generating seed, please wait");

    let mut sent_msg = ctx.send(|m| {
        m.set_embed(e.clone())
    }).await?;

    let response = request.send().await;
    if let Err(error) = response {
        e.description(format!("Error generating game: {:?}", error));
        sent_msg.edit(ctx, |m| m.set_embed(e)).await?;
        return Ok(());
    } else {
        e.description("Game generated successfully!");
        e.field("Permalink", response.unwrap().permalink(), false);
        sent_msg.edit(ctx, |m| m.set_embed(e)).await?;
    }

    Ok(())
}
