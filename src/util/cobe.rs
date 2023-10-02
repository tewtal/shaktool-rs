use inline_python::{Context, python};
use serenity::framework::standard::CommandResult;
use serenity::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

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

        Cobe {
            context
        }
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

impl TypeMapKey for Cobe {
    type Value = Arc<Mutex<Cobe>>;
}

/* Message hook for responding to messages and learn new things */
pub async fn message_hook(ctx: &serenity::client::Context, msg: &serenity::model::channel::Message) -> CommandResult {
    if !msg.is_own(ctx) {
        if msg.mentions_me(&ctx).await? {    
            let current_user = &ctx.cache.current_user();
            let arg = &msg.content_safe(ctx).replace(&format!("@{}#{}", current_user.name, current_user.discriminator), "");

            let cobe_lock = {
                let data = ctx.data.read().await;
                data.get::<Cobe>().ok_or("Could not retrieve Cobe instance")?.clone()
            };

            let reply = {
                let cobe = cobe_lock.lock().await;
                cobe.learn(arg.trim());                
                cobe.reply(arg.trim())
            };

            let _ = msg.channel_id.say(&ctx, reply).await;
            
        } else {
            let cobe_lock = {
                let data = ctx.data.read().await;
                data.get::<Cobe>().ok_or("Could not retrieve Cobe instance")?.clone()
            };
        
            {
                let cobe = cobe_lock.lock().await;
                cobe.learn(&msg.content);
            }
        }
    }

    Ok(())
}