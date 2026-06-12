use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use phf::phf_map;
use std::collections::HashMap;
use crate::{Context, Error};

const TZMAP: phf::Map<&'static str, chrono_tz::Tz> = phf_map! {
    "EST" => chrono_tz::US::Eastern,
    "CST" => chrono_tz::US::Central,
    "MST" => chrono_tz::US::Mountain,
    "PST" => chrono_tz::US::Pacific,
    "HAST" => chrono_tz::US::Hawaii,
    "AST" => chrono_tz::Canada::Atlantic,
    "UTC" => chrono_tz::UTC,
    "GMT" => chrono_tz::GMT,
    "CET" => chrono_tz::CET,
    "MEST" => chrono_tz::Africa::Cairo,
    "EEST" => chrono_tz::Africa::Cairo,
    "ARST" => chrono_tz::Asia::Baghdad,
    "MSK" => chrono_tz::Europe::Moscow,
    "EAT" => chrono_tz::Asia::Kuwait,
    "ARBST" => chrono_tz::Asia::Muscat,
    "WAST" => chrono_tz::Asia::Tashkent,
    "BTT" => chrono_tz::Asia::Dhaka,
    "NCAST" => chrono_tz::Asia::Almaty,
    "THA" => chrono_tz::Asia::Bangkok,
    "KRAT" => chrono_tz::Asia::Bangkok,
    "IRKT" => chrono_tz::Asia::Irkutsk,
    "AWST" => chrono_tz::Australia::Perth,
    "TST" => chrono_tz::Asia::Tokyo,
    "CAUST" => chrono_tz::Australia::Adelaide,
    "AEST" => chrono_tz::Australia::Sydney,
    "WPST" => chrono_tz::Pacific::Guam,
    "SBT" => chrono_tz::Pacific::Guadalcanal,
    "CAST" => chrono_tz::America::El_Salvador,
    "PSAST" => chrono_tz::America::Santiago,
    "ESAST" => chrono_tz::America::Sao_Paulo
};

/// Converts a date/time to multiple timezones
#[poise::command(prefix_command, slash_command)]
pub async fn time(
    ctx: Context<'_>,
    #[description = "Date/time string with optional timezone (e.g. '3pm EST')"]
    #[rest]
    input: Option<String>,
) -> Result<(), Error> {
    let parser = dtparse::Parser::default();
    let mut from_datetime = Utc::now();
    let input_str = input.as_deref().unwrap_or("");

    if !input_str.is_empty() {
        let (naive_datetime, _, _) = match parser.parse(input_str, None, None, true, false, None, true, &HashMap::new()) {
            Ok(r) => r,
            Err(e) => {
                ctx.say(e.to_string()).await?;
                return Ok(());
            }
        };

        let mut tz = chrono_tz::UTC;

        let words: Vec<&str> = input_str.split_whitespace().collect();
        let am_pm = input_str.to_lowercase().contains("am") || input_str.to_lowercase().contains("pm");
        if (am_pm && words.len() > 2) || (!am_pm && words.len() > 1) {
            let tz_string = words.last().unwrap();
            if TZMAP.contains_key(&tz_string.to_uppercase()) {
                tz = TZMAP[&tz_string.to_uppercase()];
            } else {
                tz = match tz_string.parse::<Tz>() {
                    Ok(t) => t,
                    Err(e) => {
                        ctx.say(e.to_string()).await?;
                        return Ok(());
                    }
                };
            }
        }

        let local_datetime = tz.from_local_datetime(&naive_datetime).unwrap();
        from_datetime = local_datetime.with_timezone(&Utc);
    }

    let fmt_24 = "**%H:%M** %Z";
    let fmt_12 = "**%-I:%M** *%p* %Z";

    let utc_time  = from_datetime.with_timezone(&chrono_tz::UTC).format(fmt_24).to_string();
    let est_time  = from_datetime.with_timezone(&chrono_tz::US::Eastern).format(fmt_12).to_string();
    let cet_time  = from_datetime.with_timezone(&chrono_tz::CET).format(fmt_24).to_string();
    let aest_time = from_datetime.with_timezone(&chrono_tz::Australia::Sydney).format(fmt_12).to_string();
    let unix_time = from_datetime.timestamp();

    ctx.say(format!("{} -> {} :: {} :: {} :: {} :: **<t:{}:t>** Local",
        input_str, utc_time, est_time, cet_time, aest_time, unix_time)).await?;

    Ok(())
}
