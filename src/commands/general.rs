use crate::{Context, Error};
use crate::api::crocomire::Strategy;

/// Shows the current bot version
#[poise::command(prefix_command, slash_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    const VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");
    ctx.say(format!("**Shaktool** {} *by total*", VERSION.unwrap_or("<unknown version>"))).await?;
    Ok(())
}

/// Searches the crocomi.re database for a Super Metroid strategy
#[poise::command(prefix_command, slash_command)]
pub async fn strat(
    ctx: Context<'_>,
    #[description = "Search string"]
    #[rest]
    query: String,
) -> Result<(), Error> {
    let strategies = Strategy::find(&query).await?;
    if !strategies.is_empty() {
        let mut output = String::new();
        for s in strategies {
            let strat_str = format!("**{}** *({}/{})* :: <https://crocomi.re/{}>\n", s.name, s.area_name, s.room_name, s.id);
            if output.len() + strat_str.len() >= 2000 {
                ctx.say(&output).await?;
                output = String::new();
            }
            output.push_str(&strat_str);
        }
        ctx.say(&output).await?;
    } else {
        ctx.say("No strategies found for that search string").await?;
    }
    Ok(())
}

/// Searches the Super Metroid Wiki for pages matching the search string
#[poise::command(prefix_command, slash_command)]
pub async fn wiki(
    ctx: Context<'_>,
    #[description = "Search string"]
    #[rest]
    query: String,
) -> Result<(), Error> {
    let titles = crate::api::wiki::search_wiki_titles(&query).await?;
    if !titles.is_empty() {
        let mut output = String::new();
        for t in titles {
            let title_str = format!("**{}** :: <https://wiki.supermetroid.run/{}>\n", t.pretty(), urlencoding::encode(&t.with_underscores()));
            if output.len() + title_str.len() >= 2000 {
                ctx.say(&output).await?;
                output = String::new();
            }
            output.push_str(&title_str);
        }
        ctx.say(output).await?;
    } else {
        ctx.say("No wiki pages found for that search string").await?;
    }
    Ok(())
}

/// Searches for an MTG card
#[poise::command(prefix_command, slash_command)]
pub async fn card(
    ctx: Context<'_>,
    #[description = "Card name"]
    #[rest]
    query: String,
) -> Result<(), Error> {
    let result = crate::api::mtg::get_card(&query).await?;
    if let Some(card) = result.cards.first() {
        let mut embed = poise::serenity_prelude::CreateEmbed::new()
            .title(&card.name)
            .field("Mana cost", card.mana_cost.as_ref().unwrap_or(&"None".into()).replace(['{', '}'], ""), true)
            .field("Type", &card.card_type, true)
            .field("Card text", &card.text, false);
        if let Some(image_url) = &card.image_url {
            embed = embed.image(image_url.replace("http", "https"));
        }
        let reply = poise::CreateReply::default().embed(embed);
        ctx.send(reply).await?;
    } else {
        ctx.say("Didn't find any cards matching that search.").await?;
    }
    Ok(())
}
