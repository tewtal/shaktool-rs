use inline_python::{Context, python};
use crate::Data;

fn strip_bot_mentions(content: &str, bot_id: u64, bot_name: &str) -> String {
    let mentions = [
        format!("<@{}>", bot_id),
        format!("<@!{}>", bot_id),
        format!("@{}", bot_name),
    ];

    mentions
        .iter()
        .fold(content.to_string(), |content, mention| content.replace(mention, ""))
        .trim()
        .to_string()
}

pub struct Cobe {
    context: Context
}

impl Cobe {
    pub fn new() -> Cobe {
        let context: Context = python! {
            import sys
            sys.path.insert(1, '.')
            print("Python :: Initializing COBE Brain")
            from cobe.brain import Brain
            b = Brain("bot.brain")
            print("Python :: Done initializing")
        };

        Cobe { context }
    }

    pub fn reply(&self, msg: &str) -> String {
        self.context.run(python! {
            reply = b.reply('msg)
        });
        self.context.get::<String>("reply")
    }

    pub fn learn(&self, msg: &str) {
        self.context.run(python! {
            b.learn('msg)
        });
    }
}

pub async fn message_hook(
    ctx: &poise::serenity_prelude::Context,
    msg: &poise::serenity_prelude::Message,
    data: &Data,
) -> Result<(), crate::Error> {
    if msg.author.id == ctx.cache.current_user().id {
        return Ok(());
    }

    let current_user = ctx.cache.current_user().clone();
    let bot_id = current_user.id.get();
    let content = strip_bot_mentions(&msg.content, bot_id, &current_user.name);

    if msg.mentions_me(&ctx).await? {
        let reply = {
            let cobe = data.cobe.lock().await;
            cobe.learn(&content);
            cobe.reply(&content)
        };
        let reply = strip_bot_mentions(&reply, bot_id, &current_user.name);

        if !reply.is_empty() {
            let _ = msg.channel_id.say(&ctx, reply).await;
        }
    } else {
        let cobe = data.cobe.lock().await;
        cobe.learn(&content);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::strip_bot_mentions;

    #[test]
    fn strips_discord_bot_mentions() {
        assert_eq!(
            strip_bot_mentions("<@123> hello <@!123>", 123, "Shaktool"),
            "hello"
        );
    }

    #[test]
    fn strips_legacy_name_mentions() {
        assert_eq!(strip_bot_mentions("@Shaktool hello", 123, "Shaktool"), "hello");
    }
}
