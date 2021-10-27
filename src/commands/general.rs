// Commands that doesn't need their own module
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::framework::standard::{Args, CommandResult, macros::{group, command}};
use crate::api::crocomire::Strategy;
use crate::commands::time::*;

#[group]
#[commands(strat, version, wiki, card, time)]
struct General;

const VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");
#[command]
pub async fn version(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id.say(&ctx, format!("**Shaktool** {} *by total*", VERSION.unwrap_or("<unknown version>"))).await?;
    Ok(())
}

#[command]
#[description = "Searches the crocomi.re database for a Super Metroid strategy"]                
#[min_args(1)]
#[usage = "<search string>"]
#[example = "bomb jump"]
pub async fn strat(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let strategies = Strategy::find(args.message()).await?;
    if !strategies.is_empty() {
        let mut output = String::new();
        
        for s in strategies {
            let strat_str = format!("**{}** *({}/{})* :: <https://crocomi.re/{}>\n", &s.name, &s.area_name, &s.room_name, &s.id);
            
            /* Discord max message length is 2000, so make sure we output before hitting that limit */
            if output.len() + strat_str.len() >= 2000 {
                msg.channel_id.say(&ctx, &output).await?;
                output = String::new();
            }
            
            output.push_str(&strat_str);
        }
        
        msg.channel_id.say(&ctx, &output).await?;
    } else {
        msg.channel_id.say(&ctx, "No strategies found for that search string").await?;
    }

    Ok(())
}

#[command]
#[description = "Searches the Super Metroid Wiki for pages matching the search string"]                
#[min_args(1)]
#[usage = "<search string>"]
#[example = "mockball"]
pub async fn wiki(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let titles = crate::api::wiki::search_wiki_titles(args.message()).await?;
    if !titles.is_empty() {
        let mut output = String::new();
        for t in titles {
            let title_str = format!("**{}** :: <https://wiki.supermetroid.run/{}>\n", &t.pretty(), urlencoding::encode(&t.with_underscores()));

            /* Discord max message length is 2000, so make sure we output before hitting that limit */
            if output.len() + title_str.len() >= 2000 {
                msg.channel_id.say(&ctx, &output).await?;
                output = String::new();
            }
            
            output.push_str(&title_str);
        }

        msg.channel_id.say(&ctx, output).await?;
    } else {
        msg.channel_id.say(&ctx, "No wiki pages found for that search string").await?;
    }

    Ok(())
}

#[command]
#[description = "Searches for an MTG card"]                
#[min_args(1)]
#[usage = "<search string>"]
#[example = "splinter twin"]
pub async fn card(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let result = crate::api::mtg::get_card(args.message()).await?;
    if let Some(card) = result.cards.first() {
        msg.channel_id.send_message(&ctx, |m| {
            m.embed(|e| {
                e.title(&card.name);
                e.field("Mana cost", &card.mana_cost.as_ref().unwrap_or(&"None".into()).replace("{", "").replace("}",""), true);
                e.field("Type", &card.card_type, true);
                e.field("Card text", &card.text, false);
                if let Some(image_url) = &card.image_url {
                    e.image(image_url.replace("http", "https"));
                }
                e
            });
            m
        }).await?;
    } else {
        msg.channel_id.say(&ctx, "Didn't find any cards matching that search.").await?;
    }
    Ok(())
}


