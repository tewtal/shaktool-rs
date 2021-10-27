use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use phf::phf_map;
use std::collections::HashMap;
use serenity::prelude::*;
use serenity::model::channel::Message;
use serenity::framework::standard::{Args, CommandResult, macros::{command}};

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

#[command]
pub async fn time(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    let parser = dtparse::Parser::default();
    let mut from_datetime = Utc::now();

    if !args.is_empty() {
        /* Parse date if possible */
        let (naive_datetime, _, _) = match parser.parse(args.message(), None, None, true, false, None, true, &HashMap::new()) {
            Ok(r) => r,
            Err(e) => {
                msg.channel_id.say(&ctx, e).await?;
                return Ok(());
            }
        };

        let mut tz = chrono_tz::UTC;

        /* Parse timezone if possible */
        let am_pm = args.message().to_lowercase().contains("am") || args.message().to_lowercase().contains("pm");
        if (am_pm && args.len() > 2) || (!am_pm && args.len() > 1) {
            let tz_string = args.raw().last().unwrap();
            if TZMAP.contains_key(&tz_string.to_uppercase()) {
                tz = TZMAP[&tz_string.to_uppercase()];
            } else {
                tz = match tz_string.parse::<Tz>() {
                    Ok(t) => t,
                    Err(e) => {                    
                        msg.channel_id.say(&ctx, e).await?;
                        return Ok(());
                    }
                };
            }
        }
        
        /* Convert the input date back to UTC */
        let local_datetime = tz.from_local_datetime(&naive_datetime).unwrap();
        let utc_datetime = local_datetime.with_timezone(&Utc);
        from_datetime = utc_datetime;
    }

    let fmt_24 = "**%H:%M** %Z";
    let fmt_12 = "**%-I:%M** *%p* %Z";

    let utc_time = from_datetime.with_timezone(&chrono_tz::UTC).format(fmt_24).to_string();
    let est_time = from_datetime.with_timezone(&chrono_tz::US::Eastern).format(fmt_12).to_string();
    let cet_time = from_datetime.with_timezone(&chrono_tz::CET).format(fmt_24).to_string();
    let aest_time = from_datetime.with_timezone(&chrono_tz::Australia::Sydney).format(fmt_12).to_string();
    let unix_time = from_datetime.timestamp();

    msg.channel_id.say(&ctx, format!("{} -> {} :: {} :: {} :: {} :: **<t:{}:t>** Local", args.message(), utc_time, est_time, cet_time, aest_time, unix_time)).await?;
    
    Ok(())
}