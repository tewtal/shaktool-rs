use poise::command;
use poise::CreateReply;
use ::serenity::all::CreateEmbed;
use crate::api::crocomire::Strategy;
use crate::commands::time::*;
use crate::{Context, Data, Error};

pub fn general_commands() -> Vec<poise::Command<Data, Error>> {
    vec![version(), strat(), wiki(), card(), time()]
}

const VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");
#[command(slash_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    let response = format!("**Shaktool** {} *by total*", VERSION.unwrap_or("<unknown version>"));
    ctx.say(response).await?;
    Ok(())
}

/// Searches for strategies on the Crocomire website
#[command(slash_command)]
pub async fn strat(ctx: Context<'_>, args: String) -> Result<(), Error> {
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
pub async fn wiki(ctx: Context<'_>, args: String) -> Result<(), Error> {
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
pub async fn card(ctx: Context<'_>, args: String) -> Result<(), Error> {
    let result = crate::api::mtg::get_card(&args).await?;
    if let Some(card) = result.cards.first() {
        let mut e = CreateEmbed::new()
            .title(&card.name)
            .field("Mana cost", &card.mana_cost.as_ref().unwrap_or(&"None".into()).replace(&['{', '}'], ""), true)
            .field("Type", &card.card_type, true)
            .field("Card text", &card.text, false);

        if let Some(image_url) = &card.image_url {
            e = e.image(image_url.replace("http", "https"));
        }

        ctx.send(CreateReply::default().embed(e)).await?;
    } else {
        ctx.say("Didn't find any cards matching that search.").await?;
    }
    Ok(())
}
