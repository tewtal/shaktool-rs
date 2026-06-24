use crate::tasks::speedrun::Mode;
use crate::{Context, Error};

#[derive(Clone, Copy, PartialEq)]
enum Level {
    /// Stored per Discord server; each server has its own value.
    Server,
    /// Stored once for the whole bot; changing it affects all servers.
    Global,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Server => "server",
            Level::Global => "global",
        }
    }
}

/// The shape a setting's value must take. Stored values are always strings;
/// the kind is what `config set` validates input against and what the help
/// text describes, so the background task never has to defend against garbage.
#[derive(Clone, Copy)]
enum ValueKind {
    /// A Discord snowflake id (channel, role, …).
    Id,
    /// An integer within `[min, max]` inclusive.
    IntRange(i64, i64),
    /// A boolean (`true`/`false`, also accepts `1/0`, `yes/no`, `on/off`).
    Bool,
    /// A comma-separated list of bare tokens (e.g. game abbreviations).
    CsvList,
    /// A comma-separated list of Discord snowflake ids (e.g. role ids).
    IdList,
    /// A comma-separated list of named URLs (`name=https://example.com`).
    NamedUrlList,
    /// A comma-separated `game[/category]:value` override list, where each
    /// value is validated by the named inner kind.
    OverrideList(&'static str),
}

impl ValueKind {
    /// One-word type name shown in help.
    fn type_name(self) -> &'static str {
        match self {
            ValueKind::Id => "id",
            ValueKind::IntRange(..) => "integer",
            ValueKind::Bool => "boolean",
            ValueKind::CsvList => "list",
            ValueKind::IdList => "id list",
            ValueKind::NamedUrlList => "named URL list",
            ValueKind::OverrideList(_) => "overrides",
        }
    }

    /// Validates a value, returning a human-readable reason on failure.
    fn validate(self, value: &str) -> Result<(), String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("value is empty".to_string());
        }
        match self {
            ValueKind::Id => validate_id(value),
            ValueKind::IntRange(min, max) => validate_int_range(value, min, max),
            ValueKind::Bool => validate_bool(value),
            ValueKind::CsvList => {
                if non_empty_tokens(value).next().is_none() {
                    return Err("list has no entries".to_string());
                }
                Ok(())
            }
            ValueKind::IdList => {
                let mut seen = false;
                for token in non_empty_tokens(value) {
                    validate_id(token)?;
                    seen = true;
                }
                if !seen {
                    return Err("list has no entries".to_string());
                }
                Ok(())
            }
            ValueKind::NamedUrlList => validate_named_url_list(value),
            ValueKind::OverrideList(inner) => validate_override_list(value, inner),
        }
    }
}

fn validate_id(value: &str) -> Result<(), String> {
    value
        .parse::<u64>()
        .map(|_| ())
        .map_err(|_| format!("`{}` is not a valid id (expected a numeric Discord id)", value))
}

fn validate_int_range(value: &str, min: i64, max: i64) -> Result<(), String> {
    match value.parse::<i64>() {
        Ok(n) if (min..=max).contains(&n) => Ok(()),
        Ok(n) => Err(format!("{} is out of range (must be {}–{})", n, min, max)),
        Err(_) => Err(format!("`{}` is not a whole number", value)),
    }
}

fn validate_bool(value: &str) -> Result<(), String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "false" | "0" | "no" | "off" => Ok(()),
        _ => Err(format!("`{}` is not a boolean (use true or false)", value)),
    }
}

fn validate_named_url_list(value: &str) -> Result<(), String> {
    let mut seen = false;
    for entry in non_empty_tokens(value) {
        seen = true;
        let Some((name, url)) = entry.split_once('=') else {
            return Err(format!("`{}` is missing `=url` (expected name=https://example.com)", entry));
        };
        validate_site_name(name)?;
        validate_url(url)?;
    }
    if !seen {
        return Err("site list has no entries".to_string());
    }
    Ok(())
}

fn validate_site_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("site name is empty".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(format!(
            "`{}` is not a valid site name (use letters, numbers, hyphen, or underscore)",
            name
        ));
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("`{}` is not an http(s) URL", url));
    }
    if url.contains(',') || url.split("://").nth(1).unwrap_or_default().is_empty() {
        return Err(format!("`{}` is not a valid URL for this setting", url));
    }
    Ok(())
}

/// Validates a `game[/category]:value` override list. The named inner kind
/// must match how the task parses each value (see `parse_overrides`).
fn validate_override_list(value: &str, inner: &str) -> Result<(), String> {
    let mut seen = false;
    for entry in non_empty_tokens(value) {
        seen = true;
        let Some((key, val)) = entry.rsplit_once(':') else {
            return Err(format!("`{}` is missing a `:value` (expected game[/category]:value)", entry));
        };
        if key.trim().is_empty() {
            return Err(format!("`{}` has an empty game/category", entry));
        }
        let val = val.trim();
        let ok = match inner {
            "mode" => Mode::parse(&val.to_ascii_lowercase()).is_some(),
            "threshold" => val.parse::<u32>().map(|n| n <= 100).unwrap_or(false),
            _ => true,
        };
        if !ok {
            return Err(format!("`{}` has an invalid value `{}`", entry, val));
        }
    }
    if !seen {
        return Err("override list has no entries".to_string());
    }
    Ok(())
}

fn non_empty_tokens(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty())
}

struct SettingDef {
    scope: &'static str,
    key: &'static str,
    level: Level,
    kind: ValueKind,
    /// A valid example value, shown in help and error messages.
    example: &'static str,
    description: &'static str,
}

/// Registry of every setting the bot understands. `config set` refuses keys
/// that aren't listed here, so adding a setting means adding it here too.
const KNOWN_SETTINGS: &[SettingDef] = &[
    SettingDef {
        scope: "speedrun",
        key: "mod_channel",
        level: Level::Server,
        kind: ValueKind::Id,
        example: "123456789012345678",
        description: "Moderation log channel: queue submissions are posted here with review buttons",
    },
    SettingDef {
        scope: "speedrun",
        key: "announce_channel",
        level: Level::Server,
        kind: ValueKind::Id,
        example: "123456789012345678",
        description: "Public channel where approved runs are announced",
    },
    SettingDef {
        scope: "speedrun",
        key: "games",
        level: Level::Server,
        kind: ValueKind::CsvList,
        example: "supermetroid,smz3",
        description: "Games this server watches (comma-separated speedrun.com abbreviations)",
    },
    SettingDef {
        scope: "speedrun",
        key: "mod_role",
        level: Level::Server,
        kind: ValueKind::IdList,
        example: "123456789012345678,987654321098765432",
        description: "Role(s) allowed to use the run review buttons in this server (comma-separated)",
    },
    SettingDef {
        scope: "speedrun",
        key: "modes",
        level: Level::Global,
        kind: ValueKind::OverrideList("mode"),
        example: "supermetroid:auto,supermetroid/100%:manual",
        description: "Moderation mode per game or game/category (modes: manual, auto; default manual)",
    },
    SettingDef {
        scope: "speedrun",
        key: "threshold",
        level: Level::Global,
        kind: ValueKind::IntRange(0, 100),
        example: "50",
        description: "Default suspicion score (0-100) at which a run counts as suspicious (default 50)",
    },
    SettingDef {
        scope: "speedrun",
        key: "thresholds",
        level: Level::Global,
        kind: ValueKind::OverrideList("threshold"),
        example: "supermetroid:60,supermetroid/Any%:40",
        description: "Threshold (0-100) overrides per game or game/category",
    },
    SettingDef {
        scope: "speedrun",
        key: "dry_run",
        level: Level::Global,
        kind: ValueKind::Bool,
        example: "true",
        description: "When true, report what auto-moderation would do without doing it",
    },
    SettingDef {
        scope: "quad",
        key: "sites",
        level: Level::Global,
        kind: ValueKind::NamedUrlList,
        example: "beta=https://beta-quad.example.com",
        description: "Extra Quad randomizer sites selectable by the quad command (live is always built in)",
    },
];

fn find_setting(scope: &str, key: &str) -> Option<&'static SettingDef> {
    KNOWN_SETTINGS.iter().find(|s| s.scope == scope && s.key == key)
}

fn known_settings_text() -> String {
    KNOWN_SETTINGS
        .iter()
        .map(|s| {
            format!(
                "`{} {}` ({}, {}) — {}\n   e.g. `{} {}`",
                s.scope,
                s.key,
                s.level.label(),
                s.kind.type_name(),
                s.description,
                s.key,
                s.example
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Manage bot configuration settings (per-server or global)
#[poise::command(
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    subcommands("get", "set", "unset", "list")
)]
pub async fn config(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say(format!(
        "Available subcommands: `get`, `set`, `unset`, `list`\n\nKnown settings:\n{}",
        known_settings_text()
    ))
    .await?;
    Ok(())
}

/// Gets a configuration value
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn get(
    ctx: Context<'_>,
    #[description = "Setting scope (e.g. speedrun)"] scope: String,
    #[description = "Setting key"] key: String,
) -> Result<(), Error> {
    let Some(def) = find_setting(&scope, &key) else {
        ctx.say(unknown_setting_text(&scope, &key)).await?;
        return Ok(());
    };
    let value = match def.level {
        Level::Server => {
            let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
            ctx.data().db.get_guild_setting(guild_id, &scope, &key).await?
        }
        Level::Global => ctx.data().db.get_global_setting(&scope, &key).await?,
    };
    match value {
        Some(value) => {
            ctx.say(format!("`{}.{}` ({} setting) = `{}`", scope, key, def.level.label(), value)).await?
        }
        None => ctx.say(format!("`{}.{}` ({} setting) is not set", scope, key, def.level.label())).await?,
    };
    Ok(())
}

/// Sets a configuration value
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn set(
    ctx: Context<'_>,
    #[description = "Setting scope (e.g. speedrun)"] scope: String,
    #[description = "Setting key"] key: String,
    #[description = "Setting value"]
    #[rest]
    value: String,
) -> Result<(), Error> {
    let Some(def) = find_setting(&scope, &key) else {
        ctx.say(unknown_setting_text(&scope, &key)).await?;
        return Ok(());
    };
    let value = value.trim();
    if let Err(reason) = def.kind.validate(value) {
        ctx.say(format!(
            "Can't set `{}.{}`: {}.\nExpected a {} — e.g. `set {} {} {}`",
            scope,
            key,
            reason,
            def.kind.type_name(),
            scope,
            key,
            def.example
        ))
        .await?;
        return Ok(());
    }
    match def.level {
        Level::Server => {
            let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
            ctx.data().db.set_guild_setting(guild_id, &scope, &key, value).await?;
            ctx.say(format!("Set **server** setting `{}.{}` = `{}` (only affects this server)", scope, key, value)).await?;
        }
        Level::Global => {
            ctx.data().db.set_global_setting(&scope, &key, value).await?;
            ctx.say(format!("Set **global** setting `{}.{}` = `{}` (affects all servers)", scope, key, value)).await?;
        }
    }
    Ok(())
}

/// Removes a configuration value
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn unset(
    ctx: Context<'_>,
    #[description = "Setting scope (e.g. speedrun)"] scope: String,
    #[description = "Setting key"] key: String,
) -> Result<(), Error> {
    let Some(def) = find_setting(&scope, &key) else {
        ctx.say(unknown_setting_text(&scope, &key)).await?;
        return Ok(());
    };
    let removed = match def.level {
        Level::Server => {
            let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
            ctx.data().db.delete_guild_setting(guild_id, &scope, &key).await?
        }
        Level::Global => ctx.data().db.delete_global_setting(&scope, &key).await?,
    };
    if removed {
        ctx.say(format!("Removed {} setting `{}.{}`", def.level.label(), scope, key)).await?;
    } else {
        ctx.say(format!("`{}.{}` ({} setting) is not set", scope, key, def.level.label())).await?;
    }
    Ok(())
}

/// Lists configured values, or all known settings when no scope is given
#[poise::command(prefix_command, slash_command, guild_only, required_permissions = "ADMINISTRATOR")]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Setting scope (e.g. speedrun)"] scope: Option<String>,
) -> Result<(), Error> {
    let Some(scope) = scope else {
        ctx.say(format!("Known settings:\n{}", known_settings_text())).await?;
        return Ok(());
    };

    let guild_id = ctx.guild_id().map(|g| g.get()).unwrap_or_default();
    let server_settings = ctx.data().db.list_guild_settings(guild_id, &scope).await?;
    let global_settings = ctx.data().db.list_global_settings(&scope).await?;

    if server_settings.is_empty() && global_settings.is_empty() {
        ctx.say(format!("No settings configured in scope `{}`", scope)).await?;
        return Ok(());
    }

    let format_settings = |settings: &[(String, String)]| {
        settings
            .iter()
            .map(|(key, value)| format!("`{}` = `{}`", key, value))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut output = format!("Settings in scope `{}`:", scope);
    if !server_settings.is_empty() {
        output.push_str(&format!("\n**This server:**\n{}", format_settings(&server_settings)));
    }
    if !global_settings.is_empty() {
        output.push_str(&format!("\n**Global (all servers):**\n{}", format_settings(&global_settings)));
    }
    ctx.say(output).await?;
    Ok(())
}

fn unknown_setting_text(scope: &str, key: &str) -> String {
    format!("Unknown setting `{} {}`. Known settings:\n{}", scope, key, known_settings_text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_settings_example_validates() {
        for setting in KNOWN_SETTINGS {
            assert!(
                setting.kind.validate(setting.example).is_ok(),
                "example `{}` for `{} {}` failed its own validation",
                setting.example,
                setting.scope,
                setting.key
            );
        }
    }

    #[test]
    fn id_rejects_non_numeric() {
        assert!(ValueKind::Id.validate("123456789012345678").is_ok());
        assert!(ValueKind::Id.validate("banana").is_err());
        assert!(ValueKind::Id.validate("").is_err());
    }

    #[test]
    fn id_list_accepts_one_or_many_ids() {
        let kind = ValueKind::IdList;
        assert!(kind.validate("123456789012345678").is_ok());
        assert!(kind.validate("123456789012345678,987654321098765432").is_ok());
        // Whitespace around entries is tolerated.
        assert!(kind.validate(" 123 , 456 ").is_ok());
        assert!(kind.validate("123,banana").is_err());
        assert!(kind.validate("").is_err());
        assert!(kind.validate(",").is_err());
    }

    #[test]
    fn named_url_list_accepts_named_sites() {
        let kind = ValueKind::NamedUrlList;
        assert!(kind.validate("beta=https://beta.example.com").is_ok());
        assert!(kind.validate("beta=https://beta.example.com,local=http://localhost:5173").is_ok());
        assert!(kind.validate("bad=https://").is_err());
        assert!(kind.validate("bad name=https://example.com").is_err());
        assert!(kind.validate("https://example.com").is_err());
        assert!(kind.validate(",").is_err());
    }

    #[test]
    fn int_range_enforces_bounds() {
        let kind = ValueKind::IntRange(0, 100);
        assert!(kind.validate("0").is_ok());
        assert!(kind.validate("100").is_ok());
        assert!(kind.validate("101").is_err());
        assert!(kind.validate("-1").is_err());
        assert!(kind.validate("fifty").is_err());
    }

    #[test]
    fn bool_accepts_common_spellings() {
        for ok in ["true", "FALSE", "1", "0", "yes", "no", "on", "off"] {
            assert!(ValueKind::Bool.validate(ok).is_ok(), "{} should be valid", ok);
        }
        assert!(ValueKind::Bool.validate("maybe").is_err());
    }

    #[test]
    fn override_list_validates_inner_values() {
        let modes = ValueKind::OverrideList("mode");
        assert!(modes.validate("supermetroid:auto,supermetroid/100%:manual").is_ok());
        assert!(modes.validate("supermetroid:bogus").is_err());
        assert!(modes.validate("supermetroid").is_err());
        assert!(modes.validate(":auto").is_err());

        let thresholds = ValueKind::OverrideList("threshold");
        assert!(thresholds.validate("supermetroid:60").is_ok());
        assert!(thresholds.validate("supermetroid:200").is_err());
    }
}
