use inline_python::{Context, python};
use crate::Data;

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

    if msg.mentions_me(&ctx).await? {
        let current_user = ctx.cache.current_user().clone();
        let arg = msg.content.replace(&format!("@{}", current_user.name), "");

        let reply = {
            let cobe = data.cobe.lock().await;
            cobe.learn(arg.trim());
            cobe.reply(arg.trim())
        };

        let _ = msg.channel_id.say(&ctx, reply).await;
    } else {
        let cobe = data.cobe.lock().await;
        cobe.learn(&msg.content);
    }

    Ok(())
}
