use serenity::builder::CreateApplicationCommand;
use serenity::{model::{
    interactions::{
        application_command::{
            ApplicationCommandOptionType,
        },
        Interaction,
        InteractionResponseType,
    },
}, model::{interactions::message_component::ButtonStyle, prelude::*}, prelude::*};


use serenity::model::{channel::{Message}, id::{MessageId, UserId}, prelude::User};
use serenity::{builder::{CreateEmbed}};
use std::{collections::{HashMap}, error::Error};
use maplit::hashmap;
use tracing::{error, debug};
use crate::util::slugid;
use std::sync::Arc;
use tokio::sync::RwLock;


#[derive(Clone)]
pub struct MultiworldSession {
    pub author_name: String,
    pub author_id: UserId,
    pub logic: String,
    pub game: String,
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

pub struct MultiworldSettingsSessionKey;
impl TypeMapKey for MultiworldSettingsSessionKey {
    type Value = Arc<RwLock<HashMap<MessageId, MessageId>>>;
}

pub fn create_multiworld_command(command: &mut CreateApplicationCommand) -> &mut CreateApplicationCommand {
    command
        .name("multiworld")
        .description("Create multiworld session")
        .create_option(|option|
            option
                .name("logic")
                .kind(ApplicationCommandOptionType::String)
                .add_string_choice("Normal", "normal")
                .add_string_choice("Hard", "hard")
                .required(true)
        )
        .create_option(|option|
            option
                .name("game")
                .kind(ApplicationCommandOptionType::String)
                .add_string_choice("SMZ3", "smz3")
                .add_string_choice("SM", "sm")
                .required(true)                
        )
}

pub async fn interaction_create_multiworld(ctx: &Context, interaction: &Interaction) -> Result<bool, Box<dyn std::error::Error>> {
    let m = interaction.clone();
    let mc = m.message_component();
    debug!("Message component: {:?}", mc);

    debug!("Interaction Id: {:?} :: Token: {:?}", &interaction.id(), &interaction.token());
    match interaction {
        Interaction::ApplicationCommand(command) => {
            match command.data.name.as_str() {
                "multiworld" => {
                    let mut session = MultiworldSession {
                        author_name: format!("{}#{}", &command.user.name, &command.user.discriminator),
                        author_id: command.user.id,
                        logic: "normal".to_owned(),
                        game: "smz3".to_owned(),
                        status: 0,
                        players: HashMap::new(),
                        link: None,
                        msg: None,
                        error: None
                    };

                    let _response = command
                        .create_interaction_response(&ctx.http, |response| {
                            response
                                .kind(InteractionResponseType::ChannelMessageWithSource)
                                .interaction_response_data(|message| {
                                    message
                                        .content("Multiworld Session Creator Settings")
                                        .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                                        .components(|components|
                                            components
                                                .create_action_row(|row|
                                                    row
                                                        .create_select_menu(|menu|
                                                            menu
                                                                .placeholder("Logic (Default 'Normal')")
                                                                .custom_id("multiworld_logic")
                                                                .options(|options|
                                                                    options
                                                                        .create_option(|option|
                                                                            option
                                                                                .label("Normal")
                                                                                .value("normal")
                                                                                .description("Normal logic")
                                                                        )
                                                                        .create_option(|option|
                                                                            option
                                                                                .label("Hard")
                                                                                .value("hard")
                                                                                .description("Hard logic")
                                                                        )
                                                                )
                                                        )                                                     
                                                )
                                                .create_action_row(|row|
                                                    row
                                                    .create_select_menu(|menu|
                                                        menu
                                                            .placeholder("Game (Default 'SMZ3')")
                                                            .custom_id("multiworld_game")
                                                            .options(|options|
                                                                options
                                                                    .create_option(|option|
                                                                        option
                                                                            .label("SMZ3")
                                                                            .value("smz3")
                                                                            .description("Super Metroid + ALTTP")
                                                                    )
                                                                    .create_option(|option|
                                                                        option
                                                                            .label("SM")
                                                                            .value("sm")
                                                                            .description("Super Metroid")
                                                                    )
                                                            )
                                                    )   
                                                )                                            
                                                .create_action_row(|row|
                                                    row
                                                        .create_button(|button|
                                                            button
                                                                .label("Start game")
                                                                .style(ButtonStyle::Success)
                                                                .custom_id("multiworld_start")
                                                            )
                                                        .create_button(|button|
                                                            button
                                                                .label("Cancel game")
                                                                .style(ButtonStyle::Danger)
                                                                .custom_id("multiworld_cancel")
                                                            )
            
                                                    )
                                            )
                                    })
                    }).await;

                    let settings_response = command.get_interaction_response(&ctx.http).await?;
                    let new_msg = command
                        .create_followup_message(&ctx.http, |message| {
                            message
                                .create_embed(|e| create_embed(&session, e))
                                .components(|components|
                                    components                                 
                                        .create_action_row(|row|
                                            row
                                                .create_button(|button|
                                                    button
                                                        .label("Join game")
                                                        .style(ButtonStyle::Primary)
                                                        .custom_id("multiworld_join")
                                                    )
                                                .create_button(|button|
                                                    button
                                                        .label("Leave game")
                                                        .style(ButtonStyle::Secondary)
                                                        .custom_id("multiworld_leave")
                                                    )            
                                            )
                                    )
                    }).await?;

                    let new_msg_id = new_msg.id;
                    session.msg = Some(new_msg);
                    
                    {
                        let data = ctx.data.read().await;
                        let sessions_lock = data.get::<MultiworldSessionKey>().unwrap().clone();
                        let mut sessions = sessions_lock.write().await;
                        sessions.insert(new_msg_id, session);
                    }

                    {
                        let data = ctx.data.read().await;
                        let sessions_lock = data.get::<MultiworldSettingsSessionKey>().unwrap().clone();
                        let mut sessions = sessions_lock.write().await;
                        sessions.insert(settings_response.id, new_msg_id);
                    }

                    Ok(true)
                },
                _ => Ok(false)
            }
        },
        Interaction::MessageComponent(component) => {
            if !component.data.custom_id.as_str().starts_with("multiworld_") {
                return Ok(false);
            }

            // Send a deferred edit message
            let _ = component.create_interaction_response(&ctx.http, |response| response.kind(InteractionResponseType::DeferredUpdateMessage)).await;
             
            let settings_session = {
                let data = ctx.data.read().await;
                let sessions_lock = data.get::<MultiworldSettingsSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                let sessions = sessions_lock.read().await;
                match sessions.contains_key(&component.message.id) {
                    true => Some(*sessions.get(&component.message.id).ok_or("Couldn't retrieve session from storage")?),
                    false => None
                }
            };

            let session_msg_id = settings_session.unwrap_or(component.message.id);

            // Get the session
            let session = {
                let data = ctx.data.read().await;
                let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                let sessions = sessions_lock.read().await;
                match sessions.contains_key(&session_msg_id) {
                    true => Some(sessions.get(&session_msg_id).ok_or("Couldn't retrieve session from storage")?.clone()),
                    false => None
                }
            };
    
            if let Some(mut session) = session {                
                match component.data.custom_id.as_str() {
                    "multiworld_join" => {                            
                        if session.status == 0 && !session.players.contains_key(&component.user.id) {                                     
                            session.players.insert(component.user.id, component.user.clone());
                        } else {
                            return Ok(true);
                        }                        
                    },
                    "multiworld_leave" => {
                        if session.status == 0 && session.players.contains_key(&component.user.id) {
                            session.players.remove(&component.user.id);
                        } else {
                            return Ok(true);
                        }                          
                    },
                    "multiworld_start" => {
                        if session.status == 0 && component.user.id == session.author_id && !session.players.is_empty() {
                            session.status = 1;
                        } else {
                            session.error = Some("At least two players are needed to start a session.".to_owned());
                            return Ok(true);
                        }                        
                    },
                    "multiworld_cancel" => {
                        if session.status == 0 && component.user.id == session.author_id {
                            session.status = 3;
                        } else {
                            return Ok(true);
                        }
                    },
                    "multiworld_logic" => {
                        if session.status == 0 {
                            session.logic = component.data.values.first().unwrap().to_owned();
                        }
                    },
                    "multiworld_game" => {
                        if session.status == 0 {
                            session.game = component.data.values.first().unwrap().to_owned();
                        }
                    },
                    _ => return Ok(false)
                };

                component.edit_followup_message(&ctx.http, session_msg_id,|message| 
                    message.create_embed(|e| create_embed(&session, e))
                ).await?;

                /* If the session isn't cancelled, update it, otherwise remove it */
                if session.status < 3
                {
                    let data = ctx.data.read().await;
                    let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                    let mut sessions = sessions_lock.write().await;
                    sessions.insert(session_msg_id, session.clone());
                } else {
                    let data = ctx.data.read().await;
                    let sessions_lock = data.get::<MultiworldSessionKey>().ok_or("Can't find multiworld session data")?.clone();
                    let mut sessions = sessions_lock.write().await;
                    sessions.remove(&session_msg_id);
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
                                let _msg = session_msg.clone();
                                // let _ = msg.edit(&ctx, |m| {
                                //     m.embed(|e| create_embed(&session, e));
                                //     m
                                // }).await;

                                component.edit_followup_message(&ctx.http, session_msg_id, |message| 
                                    message.create_embed(|e| create_embed(&session, e))
                                ).await?;

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
                                sessions.remove(&session_msg_id);
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
                            sessions.insert(session_msg_id, session.clone());
                        }
                        
                        if let Some(session_msg) = session.msg.as_ref() {
                            let mut msg = session_msg.clone();
                            let _ = msg.edit(&ctx, |m| {
                                m.embed(|e| create_embed(&session, e));
                                m
                            }).await;
                        }

                    }
                }

                Ok(true)
            } else {
                Ok(false)
            }
        },
        _ => Ok(false)
    }
}

fn create_embed<'a>(session: &MultiworldSession, e: &'a mut CreateEmbed) -> &'a mut CreateEmbed {
    e.title("Multiworld Game");    
    e.description("A new multiworld game has been initiated, react with :thumbsup: to join.\nWhen everyone is ready, the game creator can react with :white_check_mark: to create a session.");
    
    e.field("Status", match &session.status {
        0 => ":orange_square: Waiting for players to join",
        1 => ":zzz: Generating game...",
        2 => ":white_check_mark: Game created",
        3 => ":x: Game cancelled",
        _ => "Unknown status"
    }, false);
    
    e.field("Logic", &session.logic, false);                    
    e.field("Game", &session.game, false);                    
    
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