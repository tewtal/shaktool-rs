use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::framework::standard::{Args, CommandResult, macros::{group, command}};
use crate::api::wiki;


#[group]
#[commands(top)]
struct Leaderboard;

#[command]
#[description = "Lists the top 10 players for a given category"]                
#[min_args(1)]
#[usage = "<category>"]
#[example = "any%"]
pub async fn top(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let records = wiki::get_wiki_leaderboard().await;
    Ok(())
}