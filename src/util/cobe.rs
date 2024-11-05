use inline_python::{Context, python};
use serenity::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{Data, Error};

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
pub async fn message_hook(ctx: &serenity::client::Context, msg: &serenity::model::channel::Message, data: &Data) -> Result<(), Error> {
    if msg.author.id != ctx.cache.current_user().id {
        if msg.mentions_me(&ctx).await? {    
            
            let arg = {
                let current_user = ctx.cache.current_user();
                &msg.content_safe(ctx).replace(&format!("@{}#{}", current_user.name, current_user.discriminator.unwrap()), "")
            };

            // let cobe_lock = {
            //     let data = ctx.data.read().await;
            //     data.get::<Cobe>().ok_or("Could not retrieve Cobe instance")?.clone()
            // };

            let cobe_lock = &data.cobe;
            let reply = {
                let cobe = cobe_lock.lock().await;
                cobe.learn(arg.trim());                
                cobe.reply(arg.trim())
            };

            let _ = msg.channel_id.say(&ctx, reply).await;
            
        } else {
            let cobe_lock = &data.cobe;    
            {
                let cobe = cobe_lock.lock().await;
                cobe.learn(&msg.content);
            }
        }
    }

    Ok(())
}