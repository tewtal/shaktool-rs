use std::str::FromStr;
use poise::serenity_prelude::CreateEmbed;
use crate::{Context, Error};
use crate::api::smz3::{RandomizerRequest, GameMode, GanonVulnerable, Goal, KeyShuffle, MorphLocation, OpenTourian, OpenTower, SMLogic, SwordLocation};

/// Generates a SMZ3 randomizer seed
#[poise::command(prefix_command, slash_command)]
pub async fn smz3(
    ctx: Context<'_>,
    #[description = "Options as key:value pairs (e.g. logic:hard sword:early)"]
    #[rest]
    args: Option<String>,
) -> Result<(), Error> {
    let mut request = RandomizerRequest::default();
    if let Some(args) = args {
        if let Err(e) = parse_args(&args, &mut request) {
            ctx.say(format!("Error parsing arguments: {:?}", e)).await?;
            return Ok(());
        }
    }
    create_game(ctx, &request).await
}

fn parse_args(args: &str, request: &mut RandomizerRequest) -> Result<(), Error> {
    for arg in args.split_whitespace() {
        if let Some((option, value)) = arg.split_once(':') {
            match option {
                "ganon" => {
                    request.ganonvulnerable = if let Ok(i) = value.parse::<i64>() {
                        GanonVulnerable::from_int(i)
                    } else {
                        GanonVulnerable::from_str(value)?
                    };
                }
                "goal"     => request.goal = Goal::from_str(value)?,
                "keysanity" => request.keyshuffle = if value.parse::<bool>()? { KeyShuffle::Keysanity } else { KeyShuffle::None },
                "morph"    => request.morphlocation = MorphLocation::from_str(value)?,
                "tourian"  => {
                    request.opentourian = if let Ok(i) = value.parse::<i64>() {
                        OpenTourian::from_int(i)
                    } else {
                        OpenTourian::from_str(value)?
                    };
                }
                "tower"    => {
                    request.opentower = if let Ok(i) = value.parse::<i64>() {
                        OpenTower::from_int(i)
                    } else {
                        OpenTower::from_str(value)?
                    };
                }
                "logic"    => request.smlogic = SMLogic::from_str(value)?,
                "sword"    => request.swordlocation = SwordLocation::from_str(value)?,
                "race"     => request.race = value.parse::<bool>()?,
                "beta"     => request.beta = value.parse::<bool>()?,
                "start"    => request.initialitems = Some(value.to_string()),
                "gamemode" => request.gamemode = GameMode::from_str(value)?,
                "names"    => request.names = Some(value.split(',').map(|s| s.to_string()).collect()),
                _ => {}
            }
        }
    }
    Ok(())
}

async fn create_game(ctx: Context<'_>, request: &RandomizerRequest) -> Result<(), Error> {
    let embed = CreateEmbed::new()
        .title("SMZ3 Randomizer")
        .description("Generating seed, please wait");

    let reply = poise::CreateReply::default().embed(embed);
    let handle = ctx.send(reply).await?;

    match request.send().await {
        Err(error) => {
            let embed = CreateEmbed::new()
                .title("SMZ3 Randomizer")
                .description(format!("Error generating game: {:?}", error));
            handle.edit(ctx, poise::CreateReply::default().embed(embed)).await?;
        }
        Ok(response) => {
            let embed = CreateEmbed::new()
                .title("SMZ3 Randomizer")
                .description("Game generated successfully!")
                .field("Permalink", response.permalink(), false);
            handle.edit(ctx, poise::CreateReply::default().embed(embed)).await?;
        }
    }

    Ok(())
}
