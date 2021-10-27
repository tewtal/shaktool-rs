use serde_json::Map;
use serenity::prelude::*;
use serenity::model::{channel::{ReactionType, Message, Reaction}, id::{MessageId, UserId}, prelude::User};
use serenity::{builder::{CreateEmbed}, framework::standard::{CommandResult, macros::{command, group}}};
use std::{collections::{HashMap}, error::Error};
use maplit::hashmap;
use tracing::{error, info};
use crate::util::slugid;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct MultiworldSession {
    pub author_name: String,
    pub author_id: UserId,
    pub logic: String,
    pub status: i64,
    pub players: HashMap<UserId, User>,
    pub link: Option<String>,
    pub msg: Option<Message>,
    pub error: Option<String>
}

pub struct MultiworldSessionKey;
impl TypeMapKey for MultiworldSessionKey {
    type Value = Arc<RwLock<HashMap<MessageId, MultiworldSession>>>;
}

#[group]
#[commands(mw)]
struct Multiworld;

#[command]
#[bucket = "multiworld"]
#[description = "Creates a new Multiworld game proposal and enables people to join before creating a new server session.\n\
                ***logic*** can be either **normal** or **hard** and defaults to **normal** if omitted.\n\
                ***game*** can be either **smz3** or **sm** and defaults to **smz3** if omitted.\n"]
#[min_args(0)]
#[max_args(2)]
#[usage = "<logic> <game>"]
#[example = "normal smz3"]
pub async fn mw(ctx: &Context, msg: &Message) -> CommandResult {
    let mut session = MultiworldSession {
        author_name: format!("{}#{}", &msg.author.name, &msg.author.discriminator),
        author_id: msg.author.id,
        logic: "normal".to_owned(),
        status: 0,
        players: HashMap::new(),
        link: None,
        msg: None,
        error: None
    };
    
    let base_msg = msg.channel_id.say(&ctx, "A new multiworld session thread has been started").await?;

    let mut map = Map::new();
    map.insert("name".to_string(), "Multiworld Game #12345".to_string().into());
    let thread_id = ctx.http.create_public_thread(*msg.channel_id.as_u64(), *base_msg.id.as_u64(), &map).await?;

    let new_msg = thread_id.send_message(&ctx, |m| {
        m.embed(|e| create_embed(&session, e));
        m.reactions(vec!['👍', '✅', '❌']);
        m
    }).await.unwrap();

    let new_msg_id = new_msg.id;
    info!("New message id: {}", new_msg_id);

    /* We cache this message here to reduce API calls later down the line */
    session.msg = Some(new_msg);

    {
        let data = ctx.data.read().await;
        let sessions_lock = data.get::<MultiworldSessionKey>().unwrap().clone();
        let mut sessions = sessions_lock.write().await;
        sessions.insert(new_msg_id, session);
    }

    Ok(())
}

pub async fn mw_reaction_add(ctx: &Context, reaction: &Reaction) {
    if let Err(why) = mw_reaction_update(true, ctx, reaction).await {
        error!("{:?}", why);
    }
}

pub async fn mw_reaction_remove(ctx: &Context, reaction: &Reaction) {
    if let Err(why) = mw_reaction_update(false, ctx, reaction).await {
        error!("{:?}", why);
    }
}

async fn mw_reaction_update(added: bool, ctx: &Context, reaction: &Reaction) -> Result<(), Box<dyn Error>> {
    let session = {
        let data = ctx.data.read().await;
        let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
        let sessions = sessions_lock.read().await;
        match sessions.contains_key(&reaction.message_id) {
            true => Some(sessions.get(&reaction.message_id).ok_or("Couldn't retrieve session from storage")?.clone()),
            false => None
        }
    };

    if let Some(mut session) = session {
        session.error = None;
        let user = reaction.user(&ctx).await?;
        if !user.bot {
            if let ReactionType::Unicode(u) = &reaction.emoji {
                match u.as_str() {
                    "👍" => {
                        if added {                            
                            if session.status == 0 && !session.players.contains_key(&user.id) {                                     
                                session.players.insert(user.id, user);
                            } else {
                                let _ = reaction.delete(&ctx).await;
                                return Ok(());
                            }
                        } else if session.status == 0 && session.players.contains_key(&user.id) {                                     
                            session.players.remove(&user.id);
                        }
                    },
                    "✅" => {
                        if added {
                            if session.status == 0 && user.id == session.author_id && !session.players.is_empty() {
                                session.status = 1;
                            } else {
                                session.error = Some("At least two players are needed to start a session.".to_owned());
                                let _ = reaction.delete(&ctx).await;
                            }
                        } else {
                            return Ok(());
                        }
                    },
                    "❌" => {
                        if added {
                            if session.status == 0 && user.id == session.author_id {
                                session.status = 3;
                            } else {
                                let _ = reaction.delete(&ctx).await;
                                return Ok(());
                            }
                        } else {
                            return Ok(());
                        }
                    }
                    _ => {}                                     
                }
            }

            let mut msg = session.msg.as_ref().ok_or("Could not get message from session cache.")?.clone();
            let _ = msg.edit(&ctx, |m| {
                m.embed(|e| create_embed(&session, e));
                m
            }).await;
        } else {
            return Ok(());
        }
        
        /* If the session isn't cancelled, update it, otherwise remove it */
        if session.status < 3
        {
            let data = ctx.data.read().await;
            let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
            let mut sessions = sessions_lock.write().await;
            sessions.insert(reaction.message_id, session.clone());
        } else {
            let data = ctx.data.read().await;
            let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
            let mut sessions = sessions_lock.write().await;
            sessions.remove(&reaction.message_id);
        }

        /* If the status got changed to "generating", spawn a thread that generates a new game and updates the message when done */
        if session.status == 1 {
            let mut rollback = false;
            let mut options: HashMap<String, String> = hashmap!{
                "smlogic".to_string() => session.logic.to_string(),
                "goal".to_string() => "defeatboth".to_string(),
                "swordlocation".to_string() => "randomized".to_string(),
                "morphlocation".to_string() => "randomized".to_string(),
                "race".to_string() => "false".to_string(),
                "gamemode".to_string() => "multiworld".to_string(),
                "players".to_string() => session.players.len().to_string(),
                "seed".to_string() => "".to_string()
            };
            
            options.extend(session.players.iter()
                .map(|(uid, user)| 
                    (format!("player-{}", uid), user.name.to_string())
                )
            );
            
            let url = "https://beta.samus.link/api/randomizers/smz3/generate";
            let client = reqwest::Client::new();
            let res = client.post(url)
                .json(&options)
                .send()
                .await;

            if let Ok(response) = res {
                if let Ok(seed) = response.json::<serde_json::Value>().await {
                    /* Get seed guid and convert to slugid for URL */
                    let guid = seed["guid"].as_str().unwrap();
                    let slug = slugid::create(guid).unwrap();

                    /* Update the session with the final status for display */
                    session.status = 2;
                    session.link = Some(format!("https://beta.samus.link/multiworld/{}", &slug));

                    if let Some(session_msg) = session.msg.as_ref() {
                        let mut msg = session_msg.clone();
                        let _ = msg.edit(&ctx, |m| {
                            m.embed(|e| create_embed(&session, e));
                            m
                        }).await;

                        /* send DM's */
                        for user in session.players.values() {
                            let _ = user.dm(&ctx, |m| {
                                m.embed(|e| {
                                    e.title("SMZ3 Multiworld Game");
                                    e.description("A new multiworld session has now been created.");
                                    e.field("Session", session.link.as_ref().unwrap(), false);
                                    e
                                });
                                m
                            }).await;
                        }

                        /* Remove session from storage since it's not active anymore */
                        let data = ctx.data.read().await;
                        let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                        let mut sessions = sessions_lock.write().await;
                        sessions.remove(&reaction.message_id);
                    } else {
                        error!("Could not retrieve message from session cache");
                        rollback = true;
                    }
                } else {
                    error!("Could not parse the randomizer API repsonse");
                    rollback = true;
                }
            } else {                    
                error!("Could not call the randomizer API");
                rollback = true;
            }

            /* If something went wrong, we roll back the operation back to waiting for players */
            if rollback {
                session.status = 0;
                session.link = None;
                session.error = Some("An error occured while trying to generate the game, please try again later.".to_owned());

                {
                    let data = ctx.data.read().await;
                    let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                    let mut sessions = sessions_lock.write().await;
                    sessions.insert(reaction.message_id, session.clone());
                }
                
                if let Some(session_msg) = session.msg.as_ref() {
                    let mut msg = session_msg.clone();
                    let _ = msg.edit(&ctx, |m| {
                        m.embed(|e| create_embed(&session, e));
                        m
                    }).await;
                }

                let _ = reaction.delete(&ctx).await;
            }
        }
    }

    Ok(())
}

fn create_embed<'a>(session: &MultiworldSession, e: &'a mut CreateEmbed) -> &'a mut CreateEmbed {
    e.title("SMZ3 Multiworld Game");    
    e.description("A new multiworld game has been initiated, react with :thumbsup: to join.\nWhen everyone is ready, the game creator can react with :white_check_mark: to create a session.");
    
    e.field("Status", match &session.status {
        0 => ":orange_square: Waiting for players to join",
        1 => ":zzz: Generating game...",
        2 => ":white_check_mark: Game created",
        3 => ":x: Game cancelled",
        _ => "Unknown status"
    }, false);
    
    e.field("Logic", "Normal", false);                    
    
    if !session.players.is_empty() {
        e.field("Players", session.players.iter().map(|v| v.1.name.to_owned()).collect::<Vec<String>>().join("\n"), false);
    } else {
        e.field("Players", ":sob: No players registered yet", false);
    }

    if session.status == 2 {
        e.field("Session", "A session link has been generated and set as a DM to the participants.", false);
    }

    e.colour(match &session.status {
        0 => serenity::utils::Colour::DARKER_GREY,
        1 => serenity::utils::Colour::ORANGE,
        2 => serenity::utils::Colour::from_rgb(32, 200, 32),
        _ => serenity::utils::Colour::RED
    });

    if let Some(error) = &session.error {
        e.footer(|f| {
            f.text(format!("🚫 {}", error));
            f
        });
    }
    e
}