use mediawiki::api::Api;
use mediawiki::title::Title;
use std::error::Error;
use std::collections::VecDeque;
use kuchiki::traits::*;
use cached::proc_macro::cached;

const URL: &str = "https://wiki.supermetroid.run/api.php";


pub type WikiResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug, PartialEq)]
pub struct WikiRecord {
    pub place: i32,
    pub runner: String,
    pub real_time: String,
    pub date: String,
    pub link: String,
    pub source: String,
    pub comment: String,
    pub category: String
}

/* Use the cached crate here to cache this for 300 seconds (5 minutes) to prevent hammering of the Wiki */
#[cached(size=1, time=300, result=true)]
pub async fn get_leaderboard_result() -> WikiResult<serde_json::Value> {
    let api = Api::new(URL).await?;
    let params = api.params_into(&[
        ("action", "parse"),
        ("page", "Combined_Leaderboards"),
        ("prop", "text")
    ]);

    let result = api.get_query_api_json(&params).await?;
    Ok(result)
}

pub async fn get_wiki_leaderboard() -> WikiResult<Vec<WikiRecord>> {

    let result = get_leaderboard_result().await?;
    let text = result.get("parse").ok_or("Could not parse wiki result")?
        .get("text").ok_or("Could not parse wiki result")?
        .get("*").ok_or("Could not parse wiki result")?
        .as_str().ok_or("Could not parse wiki result")?;

    let document = kuchiki::parse_html().one(text);

    let mut categories = VecDeque::new();
    let mut records = Vec::new();

    /* Get all categories */
    for category in document.select("h1 > span.mw-headline > a").map_err(|_| "Could not parse categories")? {
        if let Some(element) = category.as_node().as_element() {
            let attributes = element.attributes.borrow();
            if let Some(category_name) = attributes.get("title") {
                categories.push_back(category_name.to_string());
            }
        }
    }

    /* Parse the tables */
    for table in document.select("table").map_err(|_| "Could not parse tables")? {
        let category = categories.pop_front().unwrap_or("Unknown".to_string());
        let table_node = table.as_node();
        for tr in table_node.select("tr").map_err(|_| "Could not parse rows")? {
            let tr_node = tr.as_node();
            let cols = tr_node.select("td").map_err(|_| "Could not parse columns")?.map(|e| e.text_contents().trim().to_string()).collect::<Vec<_>>();
            if cols.len() > 0 {
                let place = i32::from_str_radix(&cols[0], 10).unwrap_or(999);
                
                let link = match tr_node.select("a") {
                    Ok(mut l) => {
                        if let Some(node) = l.next() {
                            let attributes = node.attributes.borrow();
                            let link_href = attributes.get("href").unwrap_or("");
                            link_href.to_string()
                        } else {
                            String::default()
                        }
                    }
                    _ => String::default()
                };

                records.push(WikiRecord {
                    place,
                    runner: cols[1].clone(),
                    real_time: cols[2].clone(),
                    date: cols[3].clone(),
                    link,
                    source: cols[5].clone(),
                    comment: cols[6].clone(),
                    category: category.clone()
                });
            }
        }
    }

    Ok(records)
}

pub async fn search_wiki_titles(title: &str) -> WikiResult<Vec<Title>>  {
    let api = Api::new(URL).await?;
    
    let params = api.params_into(&[
        ("action", "query"),
        ("list", "search"),
        ("redirects", "1"),
        ("utf8", "1"),
        ("formatversion", "2"),
        ("srsearch", title),
        ("srwhat", "title"),
        ("srprop", "redirecttitle"),
        ("srlimit", "10")
    ]);

    let result = api.get_query_api_json(&params).await?;
    let titles = Api::result_array_to_titles(&result);
    Ok(titles)
}



