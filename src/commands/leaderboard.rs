use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::framework::standard::{Args, CommandResult, macros::{group, command}};
use crate::api::wiki;


#[group]
#[commands(top, records)]
struct Leaderboard;

#[command]
#[description = "Lists the top 10 players for a given category"]                
#[min_args(1)]
#[usage = "<category>"]
#[example = "any%"]
pub async fn top(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let records = wiki::get_wiki_leaderboard().await?;
    let category = args.message().to_lowercase();    
    
    if !records.is_empty() {
        let mut display_category = String::new();
        let mut output = String::new();

        for record in records.iter().filter(|r| r.category.to_lowercase().contains(&category)).take(10) {
            output.push_str(&format!("({}) **{}** by **{}** :: <{}>\n", record.place, record.real_time, record.runner, record.link));
            
            if display_category.is_empty() {
                display_category = record.category.to_string();
            }
        }

        msg.channel_id.say(&ctx, &format!("Top records for: **{}**\n{}", display_category, output)).await?;
    } else {
        msg.channel_id.say(&ctx, &format!("No records found for category: **{}**", category)).await?;
    }

    Ok(())
}

#[command]
#[description = "Lists all records for a given player"]                
#[min_args(1)]
#[usage = "<player>"]
#[example = "total"]
pub async fn records(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let records = wiki::get_wiki_leaderboard().await?;
    let runner = args.message().to_lowercase();    
    
    if !records.is_empty() {
        let mut display_name = String::new();
        let mut output = String::new();

        for record in records.iter().filter(|r| r.runner.to_lowercase().contains(&runner)) {
            output.push_str(&format!("**{}** ({}) **{}** :: <{}>\n", record.category, record.place, record.real_time, record.link));
            
            if display_name.is_empty() {
                display_name = record.runner.to_string();
            }
        }

        msg.channel_id.say(&ctx, &format!("Current records for player: **{}**\n{}", display_name, output)).await?;
    } else {
        msg.channel_id.say(&ctx, &format!("No records found for player: **{}**", runner)).await?;
    }

    Ok(())
}