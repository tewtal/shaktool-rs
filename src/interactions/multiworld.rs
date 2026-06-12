use poise::serenity_prelude::{
    self as serenity,
    CreateActionRow, CreateButton, CreateCommand, CreateCommandOption, CreateEmbed,
    CreateEmbedFooter, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateMessage, CreateSelectMenu, CreateSelectMenuKind,
    CreateSelectMenuOption, EditMessage,
};
use serenity::model::application::{
    ButtonStyle, CommandOptionType, ComponentInteractionDataKind, Interaction,
};
use serenity::model::prelude::*;

use std::collections::HashMap;
use maplit::hashmap;
use tracing::{error, debug};
use crate::{util::slugid, Data};


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
    pub error: Option<String>,
}

#[allow(dead_code)]
pub fn create_multiworld_command() -> CreateCommand {
    CreateCommand::new("multiworld")
        .description("Create multiworld session")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "logic", "Logic difficulty")
                .add_string_choice("Normal", "normal")
                .add_string_choice("Hard", "hard")
                .required(true)
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "game", "Game to randomize")
                .add_string_choice("SMZ3", "smz3")
                .add_string_choice("SM", "sm")
                .required(true)
        )
}

pub async fn interaction_create_multiworld(
    ctx: &serenity::Context,
    interaction: &Interaction,
    data: &Data,
) -> Result<bool, crate::Error> {
    debug!("Interaction: {:?}", interaction.id());
    match interaction {
        Interaction::Command(command) => {
            match command.data.name.as_str() {
                "multiworld" => {
                    let mut session = MultiworldSession {
                        author_name: command.user.name.clone(),
                        author_id: command.user.id,
                        logic: "normal".to_owned(),
                        game: "smz3".to_owned(),
                        status: 0,
                        players: HashMap::new(),
                        link: None,
                        msg: None,
                        error: None,
                    };

                    let settings_message = CreateInteractionResponseMessage::new()
                        .content("Multiworld Session Creator Settings")
                        .ephemeral(true)
                        .components(vec![
                            CreateActionRow::SelectMenu(
                                CreateSelectMenu::new(
                                    "multiworld_logic",
                                    CreateSelectMenuKind::String {
                                        options: vec![
                                            CreateSelectMenuOption::new("Normal", "normal")
                                                .description("Normal logic"),
                                            CreateSelectMenuOption::new("Hard", "hard")
                                                .description("Hard logic"),
                                        ]
                                    }
                                ).placeholder("Logic (Default 'Normal')")
                            ),
                            CreateActionRow::SelectMenu(
                                CreateSelectMenu::new(
                                    "multiworld_game",
                                    CreateSelectMenuKind::String {
                                        options: vec![
                                            CreateSelectMenuOption::new("SMZ3", "smz3")
                                                .description("Super Metroid + ALTTP"),
                                            CreateSelectMenuOption::new("SM", "sm")
                                                .description("Super Metroid"),
                                        ]
                                    }
                                ).placeholder("Game (Default 'SMZ3')")
                            ),
                            CreateActionRow::Buttons(vec![
                                CreateButton::new("multiworld_start")
                                    .label("Start game")
                                    .style(ButtonStyle::Success),
                                CreateButton::new("multiworld_cancel")
                                    .label("Cancel game")
                                    .style(ButtonStyle::Danger),
                            ]),
                        ]);

                    let _ = command
                        .create_response(&ctx.http, CreateInteractionResponse::Message(settings_message))
                        .await;

                    let settings_response = command.get_response(&ctx.http).await?;

                    let followup_message = CreateInteractionResponseFollowup::new()
                        .embed(create_embed(&session))
                        .components(vec![
                            CreateActionRow::Buttons(vec![
                                CreateButton::new("multiworld_join")
                                    .label("Join game")
                                    .style(ButtonStyle::Primary),
                                CreateButton::new("multiworld_leave")
                                    .label("Leave game")
                                    .style(ButtonStyle::Secondary),
                            ]),
                        ]);

                    let new_msg = command.create_followup(&ctx.http, followup_message).await?;
                    let new_msg_id = new_msg.id;
                    session.msg = Some(new_msg);

                    {
                        let mut sessions = data.multiworld_sessions.write().await;
                        sessions.insert(new_msg_id, session);
                    }
                    {
                        let mut settings = data.multiworld_settings.write().await;
                        settings.insert(settings_response.id, new_msg_id);
                    }

                    Ok(true)
                }
                _ => Ok(false),
            }
        }

        Interaction::Component(component) => {
            if !component.data.custom_id.starts_with("multiworld_") {
                return Ok(false);
            }

            let _ = component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await;

            let settings_session = {
                let settings = data.multiworld_settings.read().await;
                settings.get(&component.message.id).copied()
            };

            let session_msg_id = settings_session.unwrap_or(component.message.id);

            let session = {
                let sessions = data.multiworld_sessions.read().await;
                sessions.get(&session_msg_id).cloned()
            };

            if let Some(mut session) = session {
                match component.data.custom_id.as_str() {
                    "multiworld_join" => {
                        if session.status == 0 && !session.players.contains_key(&component.user.id) {
                            session.players.insert(component.user.id, component.user.clone());
                        } else {
                            return Ok(true);
                        }
                    }
                    "multiworld_leave" => {
                        if session.status == 0 && session.players.contains_key(&component.user.id) {
                            session.players.remove(&component.user.id);
                        } else {
                            return Ok(true);
                        }
                    }
                    "multiworld_start" => {
                        if session.status == 0 && component.user.id == session.author_id && !session.players.is_empty() {
                            session.status = 1;
                        } else {
                            session.error = Some("At least two players are needed to start a session.".to_owned());
                            return Ok(true);
                        }
                    }
                    "multiworld_cancel" => {
                        if session.status == 0 && component.user.id == session.author_id {
                            session.status = 3;
                        } else {
                            return Ok(true);
                        }
                    }
                    "multiworld_logic" => {
                        if session.status == 0 {
                            if let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind {
                                session.logic = values.first().unwrap().to_owned();
                            }
                        }
                    }
                    "multiworld_game" => {
                        if session.status == 0 {
                            if let ComponentInteractionDataKind::StringSelect { values } = &component.data.kind {
                                session.game = values.first().unwrap().to_owned();
                            }
                        }
                    }
                    _ => return Ok(false),
                }

                component.edit_followup(&ctx.http, session_msg_id,
                    CreateInteractionResponseFollowup::new().embed(create_embed(&session))
                ).await?;

                if session.status < 3 {
                    data.multiworld_sessions.write().await.insert(session_msg_id, session.clone());
                } else {
                    data.multiworld_sessions.write().await.remove(&session_msg_id);
                }

                if session.status == 1 {
                    let mut rollback = false;
                    let mut options: HashMap<String, String> = hashmap! {
                        "smlogic".to_string()      => session.logic.clone(),
                        "goal".to_string()          => "defeatboth".to_string(),
                        "swordlocation".to_string() => "randomized".to_string(),
                        "morphlocation".to_string() => "randomized".to_string(),
                        "race".to_string()          => "false".to_string(),
                        "gamemode".to_string()      => "multiworld".to_string(),
                        "players".to_string()       => session.players.len().to_string(),
                        "seed".to_string()          => "".to_string(),
                    };
                    options.extend(session.players.iter()
                        .map(|(uid, user)| (format!("player-{}", uid), user.name.clone()))
                    );

                    let url = "https://beta.samus.link/api/randomizers/smz3/generate";
                    let res = reqwest::Client::new().post(url).json(&options).send().await;

                    if let Ok(response) = res {
                        if let Ok(seed) = response.json::<serde_json::Value>().await {
                            let guid = seed["guid"].as_str().unwrap();
                            let slug = slugid::create(guid).unwrap();

                            session.status = 2;
                            session.link = Some(format!("https://beta.samus.link/multiworld/{}", slug));

                            if session.msg.is_some() {
                                component.edit_followup(&ctx.http, session_msg_id,
                                    CreateInteractionResponseFollowup::new().embed(create_embed(&session))
                                ).await?;

                                for user in session.players.values() {
                                    if let Ok(channel) = user.create_dm_channel(&ctx.http).await {
                                        let embed = CreateEmbed::new()
                                            .title("SMZ3 Multiworld Game")
                                            .description("A new multiworld session has now been created.")
                                            .field("Session", session.link.as_ref().unwrap(), false);
                                        let _ = channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await;
                                    }
                                }

                                data.multiworld_sessions.write().await.remove(&session_msg_id);
                            } else {
                                error!("Could not retrieve message from session cache");
                                rollback = true;
                            }
                        } else {
                            error!("Could not parse the randomizer API response");
                            rollback = true;
                        }
                    } else {
                        error!("Could not call the randomizer API");
                        rollback = true;
                    }

                    if rollback {
                        session.status = 0;
                        session.link = None;
                        session.error = Some("An error occurred while trying to generate the game, please try again later.".to_owned());

                        data.multiworld_sessions.write().await.insert(session_msg_id, session.clone());

                        let rollback_embed = create_embed(&session);
                        if let Some(ref mut session_msg) = session.msg {
                            let _ = session_msg.edit(&ctx.http, EditMessage::new().embed(rollback_embed)).await;
                        }
                    }
                }

                Ok(true)
            } else {
                Ok(false)
            }
        }

        _ => Ok(false),
    }
}

fn create_embed(session: &MultiworldSession) -> CreateEmbed {
    let mut e = CreateEmbed::new()
        .title("Multiworld Game")
        .description("A new multiworld game has been initiated, react with :thumbsup: to join.\nWhen everyone is ready, the game creator can react with :white_check_mark: to create a session.")
        .field("Status", match session.status {
            0 => ":orange_square: Waiting for players to join",
            1 => ":zzz: Generating game...",
            2 => ":white_check_mark: Game created",
            3 => ":x: Game cancelled",
            _ => "Unknown status",
        }, false)
        .field("Logic", &session.logic, false)
        .field("Game", &session.game, false)
        .color(match session.status {
            0 => serenity::model::Colour::DARKER_GREY,
            1 => serenity::model::Colour::ORANGE,
            2 => serenity::model::Colour::from_rgb(32, 200, 32),
            _ => serenity::model::Colour::RED,
        });

    if !session.players.is_empty() {
        e = e.field("Players", session.players.values().map(|u| u.name.clone()).collect::<Vec<_>>().join("\n"), false);
    } else {
        e = e.field("Players", ":sob: No players registered yet", false);
    }

    if session.status == 2 {
        e = e.field("Session", "A session link has been generated and sent as a DM to the participants.", false);
    }

    if let Some(error) = &session.error {
        e = e.footer(CreateEmbedFooter::new(format!("🚫 {}", error)));
    }

    e
}
