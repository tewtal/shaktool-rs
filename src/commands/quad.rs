use crate::api::quad::{self, RandomizerRequest, DEFAULT_BASE_URL};
use crate::{Context, Error};
use poise::serenity_prelude::{CreateEmbed, CreateEmbedFooter};
use serde_json::{json, Map, Value};

const SCOPE: &str = "quad";
const SITES_KEY: &str = "sites";
const LIVE_SITE: &str = "live";
const QUAD_COLOR_PENDING: u32 = 0x4F86C6;
const QUAD_COLOR_SUCCESS: u32 = 0x48A868;
const QUAD_COLOR_ERROR: u32 = 0xD64545;
const QUAD_COLOR_HELP: u32 = 0xD9A441;
const GAME_KEYS: &[&str] = &["Alttp", "Zelda1", "SuperMetroid", "Metroid", "Combo"];
const PLAYABLE_GAME_KEYS: &[&str] = &["Alttp", "Zelda1", "SuperMetroid", "Metroid"];
const OPTIONS_PAGE_SIZE: usize = 8;
const OPTIONS_FIELD_LIMIT: usize = 950;

#[derive(Clone, Debug, PartialEq, Eq)]
struct QuadSite {
    name: String,
    url: String,
}

#[derive(Clone, Copy)]
struct QuadCommandOptions<'a> {
    seed: Option<u64>,
    spoiler: Option<bool>,
    zelda1: Option<bool>,
    super_metroid: Option<bool>,
    metroid1: Option<bool>,
    profile: Option<&'a str>,
    revision: Option<&'a str>,
    options: Option<&'a str>,
}

/// Generates a Quad randomizer seed
#[poise::command(prefix_command, slash_command, aliases("csrando", "combo"))]
pub async fn quad(
    ctx: Context<'_>,
    #[description = "Specific seed number, or omit/0 for random"] seed: Option<u64>,
    #[description = "Include spoiler data in the generated seed page"] spoiler: Option<bool>,
    #[description = "Include Zelda 1"] zelda1: Option<bool>,
    #[description = "Include Super Metroid"] super_metroid: Option<bool>,
    #[description = "Include Metroid 1"] metroid1: Option<bool>,
    #[description = "Configured site to roll on, defaults to live"]
    #[autocomplete = "autocomplete_site"]
    site: Option<String>,
    #[description = "Saved profile's internal ID (use /quad-profiles to find one)"]
    #[lazy]
    profile: Option<String>,
    #[description = "Optional profile revision ID; defaults to the current revision"]
    #[lazy]
    revision: Option<String>,
    #[description = "Extra settings as key:value pairs, e.g. sm.logic:medium z1.dungeonshuffle:true"]
    #[rest]
    options: Option<String>,
) -> Result<(), Error> {
    // For prefix commands the leading `Option` positionals never consume a
    // `key:value` token (it fails to parse as their type), but `site` is a bare
    // String that would otherwise swallow the first option. Treat a `site` value
    // that looks like an option pair as part of `options` instead.
    let (site, options) = split_site_and_options(site, options);

    let command_options = QuadCommandOptions {
        seed,
        spoiler,
        zelda1,
        super_metroid,
        metroid1,
        profile: profile.as_deref(),
        revision: revision.as_deref(),
        options: options.as_deref(),
    };

    let site = match resolve_site(ctx, site.as_deref()).await {
        Ok(site) => site,
        Err(error) => {
            ctx.say(error).await?;
            return Ok(());
        }
    };

    let mut request = RandomizerRequest::quad();
    request.set_base_url(&site.url);
    request.set_api_key(quad_api_key().as_deref());

    if let Err(error) = apply_command_options(&mut request, command_options) {
        ctx.say(format!("Error parsing Quad options: {}", error))
            .await?;
        return Ok(());
    }

    create_seed(ctx, &request, &site, command_options).await
}

/// Lists official and authenticated private Quad seed profiles
#[poise::command(
    prefix_command,
    slash_command,
    rename = "quad-profiles",
    aliases("quad_profiles", "quadprofiles")
)]
pub async fn quad_profiles(
    ctx: Context<'_>,
    #[description = "Configured site to read profiles from, defaults to live"]
    #[autocomplete = "autocomplete_site"]
    #[lazy]
    site: Option<String>,
    #[description = "Filter profiles by name, slug, ID, or game"]
    #[rest]
    search: Option<String>,
) -> Result<(), Error> {
    let site = match resolve_site(ctx, site.as_deref()).await {
        Ok(site) => site,
        Err(error) => {
            ctx.say(error).await?;
            return Ok(());
        }
    };
    let api_key = quad_api_key();
    let profiles = match quad::profiles(&site.url, api_key.as_deref()).await {
        Ok(profiles) => profiles,
        Err(error) => {
            ctx.say(format!(
                "Couldn't fetch Quad profiles from `{}`: {}",
                site.name, error
            ))
            .await?;
            return Ok(());
        }
    };

    let embed = profiles_embed(&profiles, &site, search.as_deref(), api_key.is_some());
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Shows Quad randomizer options from site metadata
#[poise::command(
    prefix_command,
    slash_command,
    rename = "quad-options",
    aliases("quad_options", "quadopts")
)]
pub async fn quad_options(
    ctx: Context<'_>,
    #[description = "Section to show: summary, global, alttp, z1, sm, m1, combo"]
    #[autocomplete = "autocomplete_game"]
    section: Option<String>,
    #[description = "Page number for long option lists"]
    page: Option<u8>,
    #[description = "Configured site to read metadata from, defaults to live"]
    #[autocomplete = "autocomplete_site"]
    site: Option<String>,
    #[description = "Filter settings by key, name, category, or choice value"]
    #[rest]
    search: Option<String>,
) -> Result<(), Error> {
    // For prefix commands, `search` is the trailing rest, so embedded `page:N`
    // and `site:name` tokens are parsed out of it. Slash callers use the
    // discrete `page`/`site` fields and pass a plain search string.
    let query = OptionsQuery::parse(search.as_deref(), page, site.as_deref());

    let site = match resolve_site(ctx, query.site.as_deref()).await {
        Ok(site) => site,
        Err(error) => {
            ctx.say(error).await?;
            return Ok(());
        }
    };

    let metadata = match quad::metadata(&site.url).await {
        Ok(metadata) => metadata,
        Err(error) => {
            ctx.say(format!("Couldn't fetch Quad metadata from `{}`: {}", site.name, error))
                .await?;
            return Ok(());
        }
    };

    let section = section.as_deref().unwrap_or("summary");
    let embed = options_embed(&metadata, section, query.search.as_deref(), query.page, &site);
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Resolved `quad-options` arguments. The discrete `page`/`site` slash fields
/// take precedence; for prefix callers the same values can be embedded in the
/// trailing search text as `page:N` / `site:name` tokens, and everything else
/// is treated as the search query.
struct OptionsQuery {
    search: Option<String>,
    page: Option<u8>,
    site: Option<String>,
}

impl OptionsQuery {
    fn parse(search: Option<&str>, page: Option<u8>, site: Option<&str>) -> Self {
        let mut page = page;
        let mut site = site.map(str::to_string);
        let mut terms = Vec::new();

        for token in search.unwrap_or_default().split_whitespace() {
            match token.split_once(':').or_else(|| token.split_once('=')) {
                Some(("page" | "p", value)) if page.is_none() => {
                    if let Ok(value) = value.parse::<u8>() {
                        page = Some(value);
                        continue;
                    }
                    terms.push(token);
                }
                Some(("site" | "s", value)) if site.is_none() && !value.is_empty() => {
                    site = Some(value.to_string());
                }
                _ => terms.push(token),
            }
        }

        let search = if terms.is_empty() {
            None
        } else {
            Some(terms.join(" "))
        };

        OptionsQuery { search, page, site }
    }
}

async fn autocomplete_site(ctx: Context<'_>, partial: &str) -> Vec<String> {
    let partial = partial.to_ascii_lowercase();
    configured_sites(ctx)
        .await
        .unwrap_or_else(|_| vec![live_site()])
        .into_iter()
        .filter(|site| site.name.to_ascii_lowercase().starts_with(&partial))
        .map(|site| site.name)
        .take(25)
        .collect()
}

async fn autocomplete_game(_ctx: Context<'_>, partial: &str) -> Vec<String> {
    let partial = normalize_key(partial);
    [
        "summary",
        "global",
        "alttp",
        "zelda3",
        "z1",
        "zelda1",
        "sm",
        "supermetroid",
        "m1",
        "metroid1",
        "combo",
    ]
        .into_iter()
        .filter(|game| normalize_key(game).contains(&partial))
        .map(str::to_string)
        .take(25)
        .collect()
}

/// A `site` token that contains `:` or `=` is really the first option pair that
/// the bare positional `site` parameter greedily captured. Push it back onto the
/// front of `options` so it gets parsed as a setting instead of a site name.
fn split_site_and_options(
    site: Option<String>,
    options: Option<String>,
) -> (Option<String>, Option<String>) {
    match site {
        Some(site) if looks_like_option_pair(&site) => {
            let combined = match options {
                Some(rest) if !rest.trim().is_empty() => format!("{} {}", site, rest),
                _ => site,
            };
            (None, Some(combined))
        }
        site => (site, options),
    }
}

fn looks_like_option_pair(token: &str) -> bool {
    let token = token.trim();
    token
        .split_once(':')
        .or_else(|| token.split_once('='))
        .is_some_and(|(key, _)| !key.is_empty())
}

async fn resolve_site(ctx: Context<'_>, site: Option<&str>) -> Result<QuadSite, String> {
    let requested = site.unwrap_or(LIVE_SITE).trim();
    if requested.is_empty() || requested.eq_ignore_ascii_case(LIVE_SITE) {
        return Ok(live_site());
    }

    configured_sites(ctx)
        .await
        .map_err(|error| format!("Couldn't read configured Quad sites: {}", error))?
        .into_iter()
        .find(|site| site.name.eq_ignore_ascii_case(requested))
        .ok_or_else(|| {
            format!(
                "Unknown Quad site `{}`. Use `{}` or one configured with `config set quad sites name=https://example.com`.",
                requested, LIVE_SITE
            )
        })
}

async fn configured_sites(ctx: Context<'_>) -> Result<Vec<QuadSite>, Error> {
    let mut sites = vec![live_site()];
    if let Some(value) = ctx.data().db.get_global_setting(SCOPE, SITES_KEY).await? {
        sites.extend(parse_sites(&value));
    }
    Ok(sites)
}

fn live_site() -> QuadSite {
    QuadSite {
        name: LIVE_SITE.to_string(),
        url: DEFAULT_BASE_URL.to_string(),
    }
}

fn parse_sites(value: &str) -> Vec<QuadSite> {
    value
        .split(',')
        .filter_map(|entry| {
            let (name, url) = entry.split_once('=')?;
            let name = name.trim();
            let url = url.trim().trim_end_matches('/');
            if name.is_empty() || url.is_empty() || name.eq_ignore_ascii_case(LIVE_SITE) {
                return None;
            }
            Some(QuadSite {
                name: name.to_string(),
                url: url.to_string(),
            })
        })
        .collect()
}

fn quad_api_key() -> Option<String> {
    std::env::var("QUAD_API_KEY")
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn profiles_embed(
    profiles: &quad::ProfilesResponse,
    site: &QuadSite,
    search: Option<&str>,
    authenticated: bool,
) -> CreateEmbed {
    let search = search.map(str::trim).filter(|value| !value.is_empty());
    let mut embed = CreateEmbed::new()
        .title("Quad Seed Profiles")
        .description(
            "Copy the internal ID into `/quad profile:`. Official profile slugs are for website links and cannot be used to roll through the API.",
        )
        .color(QUAD_COLOR_HELP);

    let officials = format_profile_list(&profiles.officials, search);
    let mine = format_profile_list(&profiles.mine, search);
    embed = embed.field("Official profiles", officials, false).field(
        if authenticated {
            "My private profiles"
        } else {
            "My private profiles (QUAD_API_KEY not configured)"
        },
        mine,
        false,
    );

    if !is_live_site(site) {
        embed = embed.field("Site", &site.name, true);
    }
    embed.footer(CreateEmbedFooter::new(
        "Profiles are read from configId=combo. Results are limited to 10 per section.",
    ))
}

fn format_profile_list(profiles: &[quad::ProfileSummary], search: Option<&str>) -> String {
    let search = search.map(str::to_ascii_lowercase);
    let matches = profiles.iter().filter(|profile| {
        let Some(search) = search.as_deref() else {
            return true;
        };
        profile.name.to_ascii_lowercase().contains(search)
            || profile.id.to_ascii_lowercase().contains(search)
            || profile
                .slug
                .as_deref()
                .is_some_and(|slug| slug.to_ascii_lowercase().contains(search))
            || profile
                .selected_games
                .iter()
                .any(|game| game.to_ascii_lowercase().contains(search))
    });

    let lines = matches
        .take(10)
        .map(|profile| {
            let games = if profile.selected_games.is_empty() {
                String::new()
            } else {
                format!(" — {}", profile.selected_games.join(", "))
            };
            let slug = profile
                .slug
                .as_deref()
                .map(|slug| format!(" (`{}`)", slug))
                .unwrap_or_default();
            format!("**{}**{}{}\n`{}`", profile.name, slug, games, profile.id)
        })
        .collect::<Vec<_>>();

    if lines.is_empty() {
        "None found".to_string()
    } else {
        short_text(&lines.join("\n"), OPTIONS_FIELD_LIMIT)
    }
}

fn options_embed(
    metadata: &Value,
    section: &str,
    search: Option<&str>,
    page: Option<u8>,
    site: &QuadSite,
) -> CreateEmbed {
    let requested = section.trim();
    match resolve_metadata_section(requested) {
        Ok(MetadataSection::Summary) => options_summary_embed(metadata, site),
        Ok(MetadataSection::Global) => {
            let settings = metadata.get("settings").and_then(Value::as_array);
            settings_options_embed(
                "Global",
                "",
                settings,
                search,
                page,
                site,
                "Global options use `key:value`, usually `game:Combo` or `language:en`.",
            )
        }
        Ok(MetadataSection::Game(key)) => {
            let settings = metadata
                .get("gameSettings")
                .and_then(|games| games.get(key))
                .and_then(|game| game.get("settings"))
                .and_then(Value::as_array);
            let title = metadata_game_name(metadata, key).unwrap_or_else(|| game_label(key).to_string());
            let alias = option_alias_for_game(key);
            settings_options_embed(
                &title,
                alias,
                settings,
                search,
                page,
                site,
                &format!(
                    "Use these in `/quad` as `{alias}.Setting:value`. Multiple pairs can be separated by spaces."
                ),
            )
        }
        Err(()) => unknown_section_embed(metadata, site, requested),
    }
}

enum MetadataSection<'a> {
    Summary,
    Global,
    Game(&'a str),
}

fn resolve_metadata_section(game: &str) -> Result<MetadataSection<'static>, ()> {
    match normalize_key(game).as_str() {
        "" | "summary" | "help" | "all" => Ok(MetadataSection::Summary),
        "global" => Ok(MetadataSection::Global),
        "alttp" | "z3" | "zelda3" | "a linktothepast" | "linktothepast" => {
            Ok(MetadataSection::Game("Alttp"))
        }
        "z1" | "zelda1" | "zelda" => Ok(MetadataSection::Game("Zelda1")),
        "sm" | "super" | "supermetroid" => Ok(MetadataSection::Game("SuperMetroid")),
        "m1" | "metroid" | "metroid1" => Ok(MetadataSection::Game("Metroid")),
        "combo" | "quad" => Ok(MetadataSection::Game("Combo")),
        _ => Err(()),
    }
}

fn display_game_key(game: &str) -> &str {
    match game {
        "Alttp" => "alttp",
        "Zelda1" => "z1",
        "SuperMetroid" => "sm",
        "Metroid" => "m1",
        "Combo" => "combo",
        other => other,
    }
}

fn option_alias_for_game(game: &str) -> &str {
    display_game_key(game)
}

#[derive(Clone, Debug)]
struct OptionHelp {
    key: String,
    name: String,
    setting_type: String,
    category: Option<String>,
    default: Option<String>,
    choices: Vec<String>,
    example: String,
}

fn options_summary_embed(metadata: &Value, site: &QuadSite) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title("Quad Option Help")
        .description("Use `/quad-options section:<section>` to inspect available settings, then put the shown `key:value` pairs into `/quad options:`.")
        .color(QUAD_COLOR_HELP)
        .field(
            "Common Rolls",
            "`/quad`\n`/quad zelda1:false`\n`/quad options:sm.Logic:Medium z1.DungeonShuffle:true`",
            false,
        )
        .field(
            "Sections",
            "`global`, `alttp`, `z1`, `sm`, `m1`, `combo`",
            false,
        )
        .field(
            "Search And Pages",
            "`/quad-options section:alttp search:crystal`\n`/quad-options section:sm page:2`",
            false,
        )
        .field("Games In Metadata", metadata_games_text(metadata), false);

    if !is_live_site(site) {
        embed = embed.field("Site", &site.name, true);
    }

    embed
}

fn unknown_section_embed(metadata: &Value, site: &QuadSite, section: &str) -> CreateEmbed {
    options_summary_embed(metadata, site)
        .title("Unknown Quad Option Section")
        .description(format!(
            "`{}` is not a known section. Use one of `summary`, `global`, `alttp`, `z1`, `sm`, `m1`, or `combo`.",
            section
        ))
        .color(QUAD_COLOR_ERROR)
}

fn settings_options_embed(
    title: &str,
    alias: &str,
    settings: Option<&Vec<Value>>,
    search: Option<&str>,
    page: Option<u8>,
    site: &QuadSite,
    intro: &str,
) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title(format!("Quad Options: {}", title))
        .description(intro)
        .color(QUAD_COLOR_HELP);

    if !is_live_site(site) {
        embed = embed.field("Site", &site.name, true);
    }

    let Some(settings) = settings else {
        return embed.field("Settings", "No metadata options found for that section.", false);
    };

    let query = search.map(str::trim).filter(|query| !query.is_empty());
    let all_entries = settings
        .iter()
        .filter_map(|setting| option_help(setting, alias))
        .collect::<Vec<_>>();
    let filtered_entries = all_entries
        .iter()
        .filter(|entry| query.map(|query| option_matches(entry, query)).unwrap_or(true))
        .cloned()
        .collect::<Vec<_>>();

    if filtered_entries.is_empty() {
        let message = match query {
            Some(query) => format!(
                "No settings matched `{}`. Try a broader search, or omit `search` to browse the section.",
                query
            ),
            None => "No settings matched this section.".to_string(),
        };
        return embed.field("Settings", message, false);
    }

    let requested_page = page.map(usize::from).unwrap_or(1).max(1);
    let page_count = filtered_entries.len().div_ceil(OPTIONS_PAGE_SIZE).max(1);
    let page = requested_page.min(page_count);
    let start = (page - 1) * OPTIONS_PAGE_SIZE;
    let end = (start + OPTIONS_PAGE_SIZE).min(filtered_entries.len());
    let entries = &filtered_entries[start..end];

    let mut summary = format!(
        "Showing {}-{} of {} settings",
        start + 1,
        end,
        filtered_entries.len()
    );
    if let Some(query) = query {
        summary.push_str(&format!(" matching `{}`", query));
    }
    summary.push_str(&format!(". Page {}/{}.", page, page_count));
    embed = embed.field("Result", summary, false);

    embed = add_option_entry_fields(embed, entries);
    if page_count > 1 {
        embed = embed.footer(CreateEmbedFooter::new(format!(
            "Use page:{} to see the next page.",
            (page + 1).min(page_count)
        )));
    }

    embed
}

fn add_option_entry_fields(mut embed: CreateEmbed, entries: &[OptionHelp]) -> CreateEmbed {
    let mut chunk = String::new();
    let mut chunk_start = 1usize;
    let mut chunk_count = 0usize;

    for (index, entry) in entries.iter().enumerate() {
        let line = option_help_line(entry);
        let next_len = if chunk.is_empty() {
            line.len()
        } else {
            chunk.len() + 1 + line.len()
        };
        if !chunk.is_empty() && next_len > OPTIONS_FIELD_LIMIT {
            let title = options_field_title(chunk_start, chunk_start + chunk_count - 1);
            embed = embed.field(title, chunk, false);
            chunk = String::new();
            chunk_start = index + 1;
            chunk_count = 0;
        }
        if !chunk.is_empty() {
            chunk.push('\n');
        }
        chunk.push_str(&line);
        chunk_count += 1;
    }

    if !chunk.is_empty() {
        let title = options_field_title(chunk_start, chunk_start + chunk_count - 1);
        embed = embed.field(title, chunk, false);
    }

    embed
}

fn options_field_title(start: usize, end: usize) -> String {
    if start == end {
        format!("Setting {}", start)
    } else {
        format!("Settings {}-{}", start, end)
    }
}

fn option_help(setting: &Value, alias: &str) -> Option<OptionHelp> {
    let key = setting.get("key").and_then(Value::as_str)?;
    let name = setting
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();
    let setting_type = setting
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("Option")
        .to_string();
    let category = setting_category(setting);
    let default = setting
        .get("default")
        .map(display_value)
        .filter(|value| !value.is_empty());
    let choices = setting_choice_values(setting);
    let example = example_for_setting(alias, key, setting);

    Some(OptionHelp {
        key: key.to_string(),
        name,
        setting_type,
        category,
        default,
        choices,
        example,
    })
}

fn option_help_line(entry: &OptionHelp) -> String {
    let mut details = vec![entry.setting_type.clone()];
    if let Some(category) = &entry.category {
        details.push(category.clone());
    }
    if let Some(default) = &entry.default {
        details.push(format!("default `{}`", short_text(default, 32)));
    }
    if !entry.choices.is_empty() {
        let choices = entry
            .choices
            .iter()
            .take(5)
            .map(|choice| format!("`{}`", short_text(choice, 24)))
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if entry.choices.len() > 5 { ", ..." } else { "" };
        details.push(format!("choices {}{}", choices, suffix));
    }

    format!(
        "`{}` - {} ({})",
        entry.example,
        entry.name,
        details.join("; ")
    )
}

fn option_matches(entry: &OptionHelp, query: &str) -> bool {
    let haystack = normalize_key(&format!(
        "{} {} {} {} {} {}",
        entry.key,
        entry.name,
        entry.setting_type,
        entry.category.as_deref().unwrap_or_default(),
        entry.default.as_deref().unwrap_or_default(),
        entry.choices.join(" ")
    ));

    query
        .split_whitespace()
        .map(normalize_key)
        .all(|token| haystack.contains(&token))
}

fn metadata_games_text(metadata: &Value) -> String {
    let Some(games) = metadata.get("gameSettings").and_then(Value::as_object) else {
        return "`alttp`, `z1`, `sm`, `m1`, `combo`".to_string();
    };

    let mut keys = GAME_KEYS
        .iter()
        .copied()
        .filter(|key| games.contains_key(*key))
        .collect::<Vec<_>>();
    for key in games.keys() {
        if !keys.iter().any(|known| known == key) {
            keys.push(key.as_str());
        }
    }

    keys.into_iter()
        .map(|key| {
            let count = games
                .get(key)
                .and_then(|game| game.get("settings"))
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let name = metadata_game_name(metadata, key).unwrap_or_else(|| game_label(key).to_string());
            format!("`{}` - {} ({} settings)", display_game_key(key), name, count)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn metadata_game_name(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get("gameSettings")
        .and_then(|games| games.get(key))
        .and_then(|game| game.get("game"))
        .and_then(|game| game.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn setting_category(setting: &Value) -> Option<String> {
    setting.get("category").and_then(|category| {
        category
            .as_str()
            .map(str::to_string)
            .or_else(|| {
                category
                    .as_object()
                    .and_then(|category| category.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    })
}

fn setting_choice_values(setting: &Value) -> Vec<String> {
    setting
        .get("values")
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| choice_value(key, value))
                .collect()
        })
        .unwrap_or_default()
}

fn choice_value(key: &str, value: &Value) -> Option<String> {
    match value {
        Value::Null if key == "Random" => Some("RandomPick".to_string()),
        Value::Null => Some(key.to_string()),
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn short_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut shortened = value.chars().take(max_chars.saturating_sub(3)).collect::<String>();
    shortened.push_str("...");
    shortened
}

fn example_for_setting(alias: &str, key: &str, setting: &Value) -> String {
    let value = setting
        .get("default")
        .map(display_value)
        .or_else(|| setting_choice_values(setting).into_iter().next())
        .unwrap_or_else(|| match setting.get("type").and_then(Value::as_str) {
            Some("Toggle") => "true".to_string(),
            Some("Slider") => "1".to_string(),
            Some("MultipleChoice") => "A,B".to_string(),
            _ => "value".to_string(),
        });

    if alias.is_empty() {
        format!("{}:{}", key, value)
    } else {
        format!("{}.{}:{}", alias, key, value)
    }
}

fn apply_command_options(
    request: &mut RandomizerRequest,
    options: QuadCommandOptions<'_>,
) -> Result<(), String> {
    let profile_in_options = options
        .options
        .is_some_and(|value| option_tokens_include(value, &["profile", "profileid"]));
    let profile_requested = options.profile.is_some() || profile_in_options;
    if options.profile.is_some() && profile_in_options {
        return Err("specify `profile` only once".to_string());
    }
    if profile_requested
        && (options.zelda1.is_some()
            || options.super_metroid.is_some()
            || options.metroid1.is_some()
            || options.options.is_some_and(options_include_custom_settings))
    {
        return Err(
            "a saved profile cannot be combined with game toggles or custom settings; only seed, spoiler, and revision may override it"
                .to_string(),
        );
    }

    if let Some(profile) = options.profile {
        let profile = profile.trim();
        if profile.is_empty() {
            return Err("`profile` cannot be empty".to_string());
        }
        request.set_profile(profile, options.revision.map(str::trim));
    } else if options.revision.is_some() && !profile_in_options {
        return Err("`revision` requires a saved profile".to_string());
    }

    if let Some(seed) = options.seed {
        request.seed = seed;
    }
    if let Some(spoiler) = options.spoiler {
        request.include_spoiler = spoiler;
    }
    if let Some(enabled) = options.zelda1 {
        request.set_game_enabled("Zelda1", enabled);
    }
    if let Some(enabled) = options.super_metroid {
        request.set_game_enabled("SuperMetroid", enabled);
    }
    if let Some(enabled) = options.metroid1 {
        request.set_game_enabled("Metroid", enabled);
    }
    if let Some(options) = options.options {
        parse_options(options, request)?;
    }
    if let Some(revision) = options.revision {
        let revision = revision.trim();
        if revision.is_empty() {
            return Err("`revision` cannot be empty".to_string());
        }
        request.revision_id = Some(revision.to_string());
    }
    if request.revision_id.is_some() && !request.is_profile() {
        return Err("`revision` requires a saved profile".to_string());
    }
    Ok(())
}

fn option_tokens_include(options: &str, names: &[&str]) -> bool {
    options.split_whitespace().any(|option| {
        option
            .split_once(':')
            .or_else(|| option.split_once('='))
            .map(|(key, _)| names.contains(&normalize_key(key).as_str()))
            .unwrap_or(false)
    })
}

fn options_include_custom_settings(options: &str) -> bool {
    options.split_whitespace().any(|option| {
        option
            .split_once(':')
            .or_else(|| option.split_once('='))
            .map(|(key, _)| {
                !matches!(
                    normalize_key(key).as_str(),
                    "seed"
                        | "spoiler"
                        | "includespoiler"
                        | "profile"
                        | "profileid"
                        | "revision"
                        | "revisionid"
                )
            })
            .unwrap_or(true)
    })
}

fn apply_metadata_defaults(request: &mut RandomizerRequest, metadata: &Value) {
    if let Some(settings) = metadata.get("settings").and_then(Value::as_array) {
        for setting in settings {
            let Some(key) = setting.get("key").and_then(Value::as_str) else {
                continue;
            };
            if let Some(value) = metadata_default_value(setting).and_then(sanitize_metadata_value) {
                request.set_world_option(key, value);
            }
        }
    }

    for game in included_game_keys(request, true) {
        let Some(settings) = metadata
            .get("gameSettings")
            .and_then(|games| games.get(game))
            .and_then(|game| game.get("settings"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        let mut defaults = Map::new();
        for setting in settings {
            let Some(key) = setting.get("key").and_then(Value::as_str) else {
                continue;
            };
            if let Some(value) = metadata_default_value(setting).and_then(sanitize_metadata_value) {
                defaults.insert(key.to_string(), value);
            }
        }
        apply_slider_options_for_defaults(settings, &mut defaults);

        for (key, value) in defaults {
            request.set_game_option(game, &key, value);
        }
    }
}

fn metadata_default_value(setting: &Value) -> Option<Value> {
    match setting.get("type").and_then(Value::as_str)? {
        "SingleChoice" => {
            let default = setting.get("default")?;
            if setting
                .get("values")
                .and_then(Value::as_object)
                .map(|values| values.contains_key(&value_as_key(default)))
                .unwrap_or(true)
            {
                Some(default.clone())
            } else {
                None
            }
        }
        "MultipleChoice" => Some(
            setting
                .get("default")
                .cloned()
                .unwrap_or_else(|| json!([])),
        ),
        "Slider" => Some(setting.get("default").cloned().unwrap_or_else(|| json!(0))),
        "Toggle" => Some(setting.get("default").cloned().unwrap_or_else(|| json!(false))),
        "Input" => Some(setting.get("default").cloned().unwrap_or_else(|| json!(""))),
        "Generic" => Some(setting.get("default").cloned().unwrap_or(Value::Null)),
        _ => None,
    }
}

fn sanitize_metadata_value(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::String(value) => {
            let value = value.trim();
            if value.is_empty() || is_random_selection(value) {
                None
            } else {
                Some(json!(value))
            }
        }
        Value::Array(values) => {
            let values = values
                .into_iter()
                .filter_map(|value| match value {
                    Value::Null => None,
                    Value::String(value) if is_random_selection(&value) => None,
                    value => Some(value),
                })
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(Value::Array(values))
            }
        }
        value => Some(value),
    }
}

fn apply_slider_options_for_defaults(settings: &[Value], defaults: &mut Map<String, Value>) {
    for setting in settings {
        if setting.get("type").and_then(Value::as_str) != Some("Slider")
            || setting.get("optionsFor").is_none()
        {
            continue;
        }

        let Some(key) = setting.get("key").and_then(Value::as_str) else {
            continue;
        };

        match defaults.get(key).cloned() {
            None => {
                if let Some(range) = slider_range(setting) {
                    defaults.insert(key.to_string(), Value::Array(range));
                }
            }
            Some(Value::Number(value)) => {
                defaults.insert(key.to_string(), Value::Array(vec![Value::Number(value)]));
            }
            _ => {}
        }
    }
}

fn slider_range(setting: &Value) -> Option<Vec<Value>> {
    let range = setting.get("range")?;
    let from = range.get("from").and_then(Value::as_i64).unwrap_or(0);
    let to = range.get("to").and_then(Value::as_i64)?;
    if to < from {
        return None;
    }
    Some((from..=to).map(|value| json!(value)).collect())
}

fn is_random_selection(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("randompick")
}

fn value_as_key(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        value => display_value(value),
    }
}

fn parse_options(options: &str, request: &mut RandomizerRequest) -> Result<(), String> {
    for option in options.split_whitespace() {
        let Some((key, value)) = option.split_once(':').or_else(|| option.split_once('=')) else {
            return Err(format!("`{}` is missing `:`", option));
        };

        let key = key.trim();
        if key.is_empty() {
            return Err(format!("`{}` has an empty key", option));
        }

        apply_option(request, key, parse_value(value.trim()))?;
    }
    Ok(())
}

fn apply_option(request: &mut RandomizerRequest, key: &str, value: Value) -> Result<(), String> {
    let normalized = normalize_key(key);
    match normalized.as_str() {
        "seed" => {
            request.seed = value
                .as_u64()
                .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or_else(|| "`seed` must be a non-negative integer".to_string())?;
        }
        "spoiler" | "includespoiler" => {
            request.include_spoiler = value
                .as_bool()
                .ok_or_else(|| "`spoiler` must be true or false".to_string())?;
        }
        "profile" | "profileid" => {
            let profile = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "`profile` must be a non-empty internal profile ID".to_string())?;
            let revision = request.revision_id.clone();
            request.set_profile(profile, revision.as_deref());
        }
        "revision" | "revisionid" => {
            let revision = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "`revision` must be a non-empty revision ID".to_string())?;
            request.revision_id = Some(revision.to_string());
        }
        "z1" | "zelda1" => {
            request.set_game_enabled("Zelda1", bool_value("z1", &value)?);
        }
        "sm" | "supermetroid" | "super_metroid" => {
            request.set_game_enabled("SuperMetroid", bool_value("sm", &value)?);
        }
        "m1" | "metroid" | "metroid1" => {
            request.set_game_enabled("Metroid", bool_value("m1", &value)?);
        }
        "language" => request.set_world_option("Language", value),
        "game" => request.set_world_option("Game", value),
        _ => {
            let Some((game, setting)) = key.split_once('.') else {
                return Err(format!(
                    "`{}` is not a known shortcut. Use `Game.Setting:value`, e.g. `sm.logic:medium`.",
                    key
                ));
            };
            let game = canonical_game_key(game)
                .ok_or_else(|| format!("Unknown game `{}` in `{}`", game, key))?;
            let setting = canonical_setting_key(setting);
            request.set_game_option(game, &setting, value);
        }
    }
    Ok(())
}

fn parse_value(value: &str) -> Value {
    if value.eq_ignore_ascii_case("true") {
        return json!(true);
    }
    if value.eq_ignore_ascii_case("false") {
        return json!(false);
    }
    if let Ok(number) = value.parse::<i64>() {
        return json!(number);
    }
    if value.starts_with('[') || value.starts_with('{') {
        if let Ok(json) = serde_json::from_str(value) {
            return json;
        }
    }
    if value.contains(',') {
        return json!(value.split(',').map(str::to_string).collect::<Vec<_>>());
    }
    json!(value)
}

fn bool_value(key: &str, value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("`{}` must be true or false", key))
}

fn normalize_key(key: &str) -> String {
    key.trim()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn canonical_game_key(game: &str) -> Option<&'static str> {
    match normalize_key(game).as_str() {
        "alttp" | "z3" | "zelda3" => Some("Alttp"),
        "z1" | "zelda1" => Some("Zelda1"),
        "sm" | "super" | "supermetroid" => Some("SuperMetroid"),
        "m1" | "metroid" | "metroid1" => Some("Metroid"),
        "combo" | "quad" => Some("Combo"),
        _ => None,
    }
}

fn canonical_setting_key(setting: &str) -> String {
    match normalize_key(setting).as_str() {
        "dungeonshuffle" => "DungeonShuffle".to_string(),
        "entranceshuffle" | "entrances" => "EntranceShuffle".to_string(),
        "mapshuffle" => "MapShuffle".to_string(),
        "maprandomizer" | "smmap" => "MapRandomizer".to_string(),
        "keycards" => "Keycards".to_string(),
        "fastg4" => "FastG4".to_string(),
        "spawneverything" | "spawnallitems" => "SpawnAllItems".to_string(),
        "bosses" => "Bosses".to_string(),
        "triforces" => "Triforces".to_string(),
        "logic" => "Logic".to_string(),
        "goal" => "Goal".to_string(),
        "state" => "State".to_string(),
        "weapon" => "Weapon".to_string(),
        "crystalsganon" | "ganoncrystals" => "CrystalsGanon".to_string(),
        "crystalstower" | "towercrystals" => "CrystalsTower".to_string(),
        _ => setting.to_string(),
    }
}

async fn create_seed(
    ctx: Context<'_>,
    preview_request: &RandomizerRequest,
    site: &QuadSite,
    options: QuadCommandOptions<'_>,
) -> Result<(), Error> {
    let mut preview_defaults = RandomizerRequest::quad();
    preview_defaults.set_base_url(&site.url);
    let preview_highlights = request_highlights(preview_request, &preview_defaults);

    let handle = ctx
        .send(poise::CreateReply::default().embed(generating_embed(
            preview_request,
            site,
            &preview_highlights,
        )))
        .await?;

    let mut default_request = RandomizerRequest::quad();
    default_request.set_base_url(&site.url);
    default_request.set_api_key(quad_api_key().as_deref());
    let metadata_loaded = if preview_request.is_profile() {
        true
    } else {
        match quad::metadata(&site.url).await {
            Ok(metadata) => {
                apply_metadata_defaults(&mut default_request, &metadata);
                true
            }
            Err(_) => false,
        }
    };

    let mut request = default_request.clone();
    if let Err(error) = apply_command_options(&mut request, options) {
        handle
            .edit(
                ctx,
                poise::CreateReply::default().embed(error_embed(
                    &request,
                    site,
                    &request_highlights(&request, &default_request),
                    &format!("Error parsing Quad options: {}", error),
                )),
            )
            .await?;
        return Ok(());
    }

    let highlights = request_highlights(&request, &default_request);
    match request.send().await {
        Err(error) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().embed(error_embed(
                        &request,
                        site,
                        &highlights,
                        &format!("Error generating seed: {}", error),
                    )),
                )
                .await?;
        }
        Ok(response) => {
            let mut embed = success_embed(&request, site, &highlights, &response);
            if !metadata_loaded {
                embed = embed.footer(CreateEmbedFooter::new(
                    "Generated without metadata defaults because option metadata could not be fetched.",
                ));
            }
            handle
                .edit(ctx, poise::CreateReply::default().embed(embed))
                .await?;
        }
    }

    Ok(())
}

fn generating_embed(
    request: &RandomizerRequest,
    site: &QuadSite,
    highlights: &[String],
) -> CreateEmbed {
    seed_embed(
        "Quad Randomizer",
        "Generating seed. This can take a little while.",
        QUAD_COLOR_PENDING,
        request,
        site,
        highlights,
    )
    .field("Seed", requested_seed_text(request), true)
    .footer(CreateEmbedFooter::new("The message will update when the seed is ready."))
}

fn success_embed(
    request: &RandomizerRequest,
    site: &QuadSite,
    highlights: &[String],
    response: &quad::RandomizerResponse,
) -> CreateEmbed {
    seed_embed(
        "Quad Seed Ready",
        "Seed generated. The seed page has the patch, spoiler details, and full options.",
        QUAD_COLOR_SUCCESS,
        request,
        site,
        highlights,
    )
    .field("Seed", response.seed.to_string(), true)
    .field("Permalink", response.permalink(&site.url), false)
}

fn error_embed(
    request: &RandomizerRequest,
    site: &QuadSite,
    highlights: &[String],
    message: &str,
) -> CreateEmbed {
    seed_embed(
        "Quad Seed Failed",
        message,
        QUAD_COLOR_ERROR,
        request,
        site,
        highlights,
    )
}

fn seed_embed(
    title: &str,
    description: &str,
    color: u32,
    request: &RandomizerRequest,
    site: &QuadSite,
    highlights: &[String],
) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title(title)
        .description(description)
        .color(color)
        .field("Games", included_games_text(request), false)
        .field("Options", options_summary(highlights), false);

    if let Some(profile_id) = request.profile_id.as_deref() {
        let profile = match request.revision_id.as_deref() {
            Some(revision_id) => format!("`{}` (revision `{}`)", profile_id, revision_id),
            None => format!("`{}` (current revision)", profile_id),
        };
        embed = embed.field("Profile", profile, false);
    }

    if !is_live_site(site) {
        embed = embed.field("Site", &site.name, true);
    }

    embed
}

fn included_games_text(request: &RandomizerRequest) -> String {
    if request.is_profile() {
        return "Defined by saved profile".to_string();
    }
    let games = included_game_keys(request, false)
        .into_iter()
        .map(game_label)
        .collect::<Vec<_>>();

    if games.is_empty() {
        "No games selected".to_string()
    } else {
        games.join(", ")
    }
}

fn requested_seed_text(request: &RandomizerRequest) -> String {
    if request.seed == 0 {
        "Random".to_string()
    } else {
        request.seed.to_string()
    }
}

fn options_summary(highlights: &[String]) -> String {
    if highlights.is_empty() {
        return "Standard Quad defaults".to_string();
    }

    let shown = highlights.iter().take(8).cloned().collect::<Vec<_>>();
    let hidden = highlights.len().saturating_sub(shown.len());
    if hidden == 0 {
        shown.join("\n")
    } else {
        format!("{}\n+{} more", shown.join("\n"), hidden)
    }
}

fn request_highlights(
    request: &RandomizerRequest,
    default_request: &RandomizerRequest,
) -> Vec<String> {
    if request.is_profile() {
        let mut options = vec!["Saved profile settings".to_string()];
        if request.seed != 0 {
            options.push(format!("Seed: {}", requested_seed_text(request)));
        }
        if !request.include_spoiler {
            options.push("Spoiler log: off".to_string());
        }
        return options;
    }
    notable_options(request, default_request)
}

fn notable_options(
    request: &RandomizerRequest,
    default_request: &RandomizerRequest,
) -> Vec<String> {
    let mut options = Vec::new();

    if request.seed != default_request.seed {
        options.push(format!("Seed: {}", requested_seed_text(request)));
    }
    if request.include_spoiler != default_request.include_spoiler {
        options.push(format!(
            "Spoiler log: {}",
            if request.include_spoiler { "on" } else { "off" }
        ));
    }

    let Some(world) = world_config(request) else {
        return options;
    };
    let default_world = world_config(default_request);

    for game in PLAYABLE_GAME_KEYS {
        let enabled = world.contains_key(*game);
        let default_enabled = default_world
            .map(|world| world.contains_key(*game))
            .unwrap_or(false);
        if enabled != default_enabled {
            options.push(format!(
                "{}: {}",
                game_label(game),
                if enabled { "on" } else { "off" }
            ));
        }
    }

    for key in ["Language", "Game"] {
        let value = world.get(key);
        let default_value = default_world.and_then(|world| world.get(key));
        if let Some(value) = value {
            if Some(value) != default_value {
                options.push(format!("{}: {}", key, short_display_value(value)));
            }
        }
    }

    for game in GAME_KEYS {
        let Some(game_options) = world.get(*game).and_then(Value::as_object) else {
            continue;
        };
        let default_game_options = default_world
            .and_then(|world| world.get(*game))
            .and_then(Value::as_object);

        for (key, value) in game_options {
            if default_game_options.and_then(|options| options.get(key)) == Some(value) {
                continue;
            }
            options.push(format!(
                "{}.{}: {}",
                display_game_key(game),
                key,
                short_display_value(value)
            ));
        }
    }

    options
}

fn included_game_keys(request: &RandomizerRequest, include_combo: bool) -> Vec<&'static str> {
    let Some(world) = world_config(request) else {
        return Vec::new();
    };

    GAME_KEYS
        .iter()
        .copied()
        .filter(|game| include_combo || *game != "Combo")
        .filter(|game| world.get(*game).and_then(Value::as_object).is_some())
        .collect()
}

fn world_config(request: &RandomizerRequest) -> Option<&Map<String, Value>> {
    request.configs.first()
}

fn game_label(game: &str) -> &'static str {
    match game {
        "Alttp" => "Zelda 3",
        "Zelda1" => "Zelda 1",
        "SuperMetroid" => "Super Metroid",
        "Metroid" => "Metroid 1",
        "Combo" => "Combo",
        _ => "Unknown",
    }
}

fn short_display_value(value: &Value) -> String {
    let value = display_value(value);
    if value.chars().count() <= 64 {
        return value;
    }

    let mut shortened = value.chars().take(61).collect::<String>();
    shortened.push_str("...");
    shortened
}

fn is_live_site(site: &QuadSite) -> bool {
    site.name.eq_ignore_ascii_case(LIVE_SITE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_options<'a>(options: Option<&'a str>) -> QuadCommandOptions<'a> {
        QuadCommandOptions {
            seed: None,
            spoiler: None,
            zelda1: None,
            super_metroid: None,
            metroid1: None,
            profile: None,
            revision: None,
            options,
        }
    }

    #[test]
    fn parses_shortcuts_and_game_options() {
        let mut request = RandomizerRequest::quad();
        parse_options(
            "seed:123 spoiler:false z1:false sm.logic:medium",
            &mut request,
        )
        .unwrap();

        assert_eq!(request.seed, 123);
        assert!(!request.include_spoiler);
        let world = &request.configs[0];
        assert!(!world.contains_key("Zelda1"));
        assert_eq!(world["SuperMetroid"]["Logic"], json!("medium"));

        parse_options("z1.dungeonshuffle:true", &mut request).unwrap();
        let world = &request.configs[0];
        assert_eq!(world["Zelda1"]["DungeonShuffle"], json!(true));
    }

    #[test]
    fn profile_options_build_a_profile_request() {
        let mut request = RandomizerRequest::quad();
        apply_command_options(
            &mut request,
            command_options(Some(
                "revision:revision-id profile:profile-id seed:123 spoiler:false",
            )),
        )
        .unwrap();

        assert_eq!(request.profile_id.as_deref(), Some("profile-id"));
        assert_eq!(request.revision_id.as_deref(), Some("revision-id"));
        assert!(request.configs.is_empty());
        assert_eq!(request.seed, 123);
        assert!(!request.include_spoiler);
    }

    #[test]
    fn profile_rejects_custom_settings_and_revision_requires_profile() {
        let mut request = RandomizerRequest::quad();
        let error = apply_command_options(
            &mut request,
            command_options(Some("profile:profile-id sm.logic:medium")),
        )
        .unwrap_err();
        assert!(error.contains("cannot be combined"));

        let mut request = RandomizerRequest::quad();
        let error =
            apply_command_options(&mut request, command_options(Some("revision:revision-id")))
                .unwrap_err();
        assert!(error.contains("requires a saved profile"));
    }

    #[test]
    fn site_positional_does_not_swallow_options() {
        // `%quad sm.Logic:Medium z1.DungeonShuffle:true` parses as
        // site="sm.Logic:Medium", options="z1.DungeonShuffle:true" before fixup.
        let (site, options) = split_site_and_options(
            Some("sm.Logic:Medium".to_string()),
            Some("z1.DungeonShuffle:true".to_string()),
        );
        assert_eq!(site, None);
        assert_eq!(
            options.as_deref(),
            Some("sm.Logic:Medium z1.DungeonShuffle:true")
        );

        // A real site name is left untouched.
        let (site, options) =
            split_site_and_options(Some("beta".to_string()), Some("sm.Logic:Medium".to_string()));
        assert_eq!(site.as_deref(), Some("beta"));
        assert_eq!(options.as_deref(), Some("sm.Logic:Medium"));

        // A single option pair with no trailing options still gets moved.
        let (site, options) = split_site_and_options(Some("language:en".to_string()), None);
        assert_eq!(site, None);
        assert_eq!(options.as_deref(), Some("language:en"));
    }

    #[test]
    fn options_query_parses_embedded_page_and_site() {
        // Prefix: `%quad-options alttp dungeon shuffle page:2 site:beta`
        let query = OptionsQuery::parse(Some("dungeon shuffle page:2 site:beta"), None, None);
        assert_eq!(query.search.as_deref(), Some("dungeon shuffle"));
        assert_eq!(query.page, Some(2));
        assert_eq!(query.site.as_deref(), Some("beta"));

        // Plain multi-word search with no embedded tokens.
        let query = OptionsQuery::parse(Some("map shuffle"), None, None);
        assert_eq!(query.search.as_deref(), Some("map shuffle"));
        assert_eq!(query.page, None);
        assert_eq!(query.site, None);

        // Discrete slash fields win over embedded tokens.
        let query = OptionsQuery::parse(Some("crystal page:5"), Some(2), Some("live"));
        assert_eq!(query.search.as_deref(), Some("crystal page:5"));
        assert_eq!(query.page, Some(2));
        assert_eq!(query.site.as_deref(), Some("live"));

        // A bare page number is NOT treated as a page (avoids eating search terms).
        let query = OptionsQuery::parse(Some("2"), None, None);
        assert_eq!(query.search.as_deref(), Some("2"));
        assert_eq!(query.page, None);
    }

    #[test]
    fn parses_configured_sites() {
        assert_eq!(
            parse_sites("beta=https://beta.example.com,local=http://localhost:5173"),
            vec![
                QuadSite {
                    name: "beta".to_string(),
                    url: "https://beta.example.com".to_string(),
                },
                QuadSite {
                    name: "local".to_string(),
                    url: "http://localhost:5173".to_string(),
                },
            ]
        );
        assert!(parse_sites("live=https://not-used.example.com").is_empty());
    }

    #[test]
    fn formats_option_examples() {
        let setting = json!({
            "key": "Logic",
            "name": "Logic",
            "type": "SingleChoice",
            "values": {"Basic": "Basic", "Medium": "Medium"},
            "default": "Basic"
        });
        assert_eq!(example_for_setting("sm", "Logic", &setting), "sm.Logic:Basic");

        let setting = json!({
            "key": "Goal",
            "name": "Goal",
            "type": "SingleChoice",
            "values": {"Fast Ganon": "FastGanon", "Pedestal": "Pedestal"}
        });
        assert_eq!(example_for_setting("alttp", "Goal", &setting), "alttp.Goal:FastGanon");
    }

    #[test]
    fn options_summary_documents_slash_name_and_hides_live_site() {
        let metadata = json!({
            "gameSettings": {
                "Alttp": {
                    "game": {"name": "Zelda 3"},
                    "settings": []
                }
            }
        });

        let embed = options_embed(&metadata, "summary", None, None, &live_site());
        let json = serde_json::to_value(embed).unwrap();
        let text = json.to_string();

        assert!(text.contains("/quad-options"));
        assert!(!text.contains("\"name\":\"Site\""));
    }

    #[test]
    fn options_search_filters_settings() {
        let metadata = json!({
            "gameSettings": {
                "Alttp": {
                    "game": {"name": "Zelda 3"},
                    "settings": [
                        {"key": "Logic", "name": "Logic", "type": "SingleChoice", "values": {"Normal": "Normal"}},
                        {"key": "CrystalsGanon", "name": "Crystals Ganon", "category": {"name": "Goal"}, "type": "SingleChoice", "values": {"7": "7"}, "default": "7"}
                    ]
                }
            }
        });

        let embed = options_embed(&metadata, "alttp", Some("crystal"), None, &live_site());
        let text = serde_json::to_string(&embed).unwrap();

        assert!(text.contains("CrystalsGanon"));
        assert!(!text.contains("alttp.Logic"));
        assert!(text.contains("matching `crystal`"));
    }

    #[test]
    fn options_embed_chunks_large_sections_under_discord_limits() {
        let settings = (0..24)
            .map(|index| {
                json!({
                    "key": format!("VeryLongSettingName{}", index),
                    "name": format!("Very Long Setting Name {}", index),
                    "category": {"name": "Very Long Category Name"},
                    "type": "SingleChoice",
                    "values": {
                        "Very Long Display Choice A": "VeryLongChoiceA",
                        "Very Long Display Choice B": "VeryLongChoiceB",
                        "Very Long Display Choice C": "VeryLongChoiceC",
                        "Very Long Display Choice D": "VeryLongChoiceD",
                        "Very Long Display Choice E": "VeryLongChoiceE",
                        "Very Long Display Choice F": "VeryLongChoiceF"
                    },
                    "default": "VeryLongChoiceA"
                })
            })
            .collect::<Vec<_>>();
        let metadata = json!({
            "gameSettings": {
                "Alttp": {
                    "game": {"name": "Zelda 3"},
                    "settings": settings
                }
            }
        });

        let embed = options_embed(&metadata, "alttp", None, Some(1), &live_site());
        let json = serde_json::to_value(embed).unwrap();

        assert!(json["description"].as_str().unwrap().len() <= 4096);
        for field in json["fields"].as_array().unwrap() {
            assert!(field["name"].as_str().unwrap().len() <= 256);
            assert!(field["value"].as_str().unwrap().len() <= 1024);
        }
    }

    #[test]
    fn applies_metadata_defaults_to_request() {
        let metadata = json!({
            "settings": [
                {"key": "Language", "name": "Language", "type": "SingleChoice", "values": {"en": "en"}, "default": "en"},
                {"key": "Game", "name": "Game", "type": "SingleChoice", "values": {"Combo": "Combo"}, "default": "Combo"}
            ],
            "gameSettings": {
                "SuperMetroid": {
                    "settings": [
                        {"key": "Logic", "name": "Logic", "type": "SingleChoice", "values": {"Basic": "Basic", "Medium": "Medium"}, "default": "Basic"},
                        {"key": "MapRandomizer", "name": "Map Randomizer", "type": "Toggle", "default": false}
                    ]
                },
                "Alttp": {
                    "settings": [
                        {"key": "MoldormEyeCountChoices", "name": "Moldorm Eye Count Choices", "type": "Slider", "range": {"from": 0, "to": 8}, "optionsFor": "MoldormEyeCount"}
                    ]
                }
            }
        });

        let mut request = RandomizerRequest::quad();
        apply_metadata_defaults(&mut request, &metadata);
        let world = &request.configs[0];

        assert_eq!(world["Language"], json!("en"));
        assert_eq!(world["Game"], json!("Combo"));
        assert_eq!(world["SuperMetroid"]["Logic"], json!("Basic"));
        assert_eq!(world["SuperMetroid"]["MapRandomizer"], json!(false));
        assert_eq!(world["Alttp"]["MoldormEyeCountChoices"], json!([0]));
    }

    #[test]
    fn notable_options_only_reports_changes_from_defaults() {
        let metadata = json!({
            "gameSettings": {
                "SuperMetroid": {
                    "settings": [
                        {"key": "Logic", "name": "Logic", "type": "SingleChoice", "values": {"Basic": "Basic", "Medium": "Medium"}, "default": "Basic"}
                    ]
                }
            }
        });

        let mut defaults = RandomizerRequest::quad();
        apply_metadata_defaults(&mut defaults, &metadata);

        let mut request = defaults.clone();
        request.set_game_enabled("Zelda1", false);
        request.set_game_option("SuperMetroid", "Logic", json!("Medium"));

        let options = notable_options(&request, &defaults);
        assert_eq!(
            options,
            vec![
                "Zelda 1: off".to_string(),
                "sm.Logic: Medium".to_string()
            ]
        );
    }
}
