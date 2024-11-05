use poise::serenity_prelude as serenity;
use poise::command;
use crate::api::crocomire::Strategy;
use crate::commands::time::*;

#[command(slash_command)]
pub async fn version(ctx: poise::Context<'_>) -> Result<(), serenity::Error> {
    let response = format!("**Shaktool** {} *by total*", VERSION.unwrap_or("<unknown version>"));
    ctx.say(response).await?;
    Ok(())
}

#[command(slash_command)]
#[description = "Searches the crocomi.re database for a Super Metroid strategy"]
pub async fn strat(ctx: poise::Context<'_>, args: String) -> Result<(), serenity::Error> {
    let strategies = Strategy::find(&args).await?;
    if !strategies.is_empty() {
        let mut output = String::new();

        for s in strategies {
            let strat_str = format!("**{}** *({}/{})* :: <https://crocomi.re/{}>\n", &s.name, &s.area_name, &s.room_name, &s.id);

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

#[command(slash_command)]
#[description = "Searches the Super Metroid Wiki for pages matching the search string"]
pub async fn wiki(ctx: poise::Context<'_>, args: String) -> Result<(), serenity::Error> {
    let titles = crate::api::wiki::search_wiki_titles(&args).await?;
    if !titles.is_empty() {
        let mut output = String::new();
        for t in titles {
            let title_str = format!("**{}** :: <https://wiki.supermetroid.run/{}>\n", &t.pretty(), urlencoding::encode(&t.with_underscores()));

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

#[command(slash_command)]
#[description = "Searches for an MTG card"]
pub async fn card(ctx: poise::Context<'_>, args: String) -> Result<(), serenity::Error> {
    let result = crate::api::mtg::get_card(&args).await?;
    if let Some(card) = result.cards.first() {
        ctx.send(|m| {
            m.embed(|e| {
                e.title(&card.name);
                e.field("Mana cost", &card.mana_cost.as_ref().unwrap_or(&"None".into()).replace(&['{', '}'], ""), true);
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
        ctx.say("Didn't find any cards matching that search.").await?;
    }
    Ok(())
}
