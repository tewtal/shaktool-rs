use poise::command;
use poise::CreateReply;
use ::serenity::all::CreateEmbed;
use crate::api::wiki;
use crate::{Context, Data, Error};

pub fn leaderboard_commands () -> Vec<poise::Command<Data, Error>> {
    vec![top(), records()]
}

/// Lists the top 10 players for a given category
#[command(slash_command)]
pub async fn top(ctx: Context<'_>, args: String) -> Result<(), Error> {
    let records = wiki::get_wiki_leaderboard().await?;
    let category = args.to_lowercase();

    if !records.is_empty() {
        let mut display_category = String::new();
        let mut output = String::new();

        for record in records.iter().filter(|r| r.category.to_lowercase().contains(&category)).take(10) {
            output.push_str(&format!("({}) **{}** by **{}** :: <{}>\n", record.place, record.real_time, record.runner, record.link));

            if display_category.is_empty() {
                display_category = record.category.to_string();
            }
        }

        ctx.say(&format!("Top records for: **{}**\n{}", display_category, output)).await?;
    } else {
        ctx.say(&format!("No records found for category: **{}**", category)).await?;
    }

    Ok(())
}

/// Lists all records for a given player
#[command(slash_command)]
pub async fn records(ctx: Context<'_>, args: String) -> Result<(), Error> {
    let records = wiki::get_wiki_leaderboard().await?;
    let runner = args.to_lowercase();

    if !records.is_empty() {
        let mut display_name = String::new();
        let mut output = String::new();

        for record in records.iter().filter(|r| r.runner.to_lowercase().contains(&runner)) {
            output.push_str(&format!("**{}** ({}) **{}** :: <{}>\n", record.category, record.place, record.real_time, record.link));

            if display_name.is_empty() {
                display_name = record.runner.to_string();
            }
        }

        ctx.say(&format!("Current records for player: **{}**\n{}", display_name, output)).await?;
    } else {
        ctx.say(&format!("No records found for player: **{}**", runner)).await?;
    }

    Ok(())
}
