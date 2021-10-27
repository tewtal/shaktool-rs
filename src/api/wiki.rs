use mediawiki::api::Api;
use mediawiki::title::Title;
use std::error::Error;
use kuchiki::traits::*;

const URL: &str = "https://wiki.supermetroid.run/api.php";


pub type WikiResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct WikiRecord {
    place: i32,
    runner: String,
    real_time: String,
    date: String,
    link: String,
    source: String,
    comment: String,
    category: String
}

pub async fn get_wiki_leaderboard() -> WikiResult<Vec<WikiRecord>> {
    let api = Api::new(URL).await?;
    let params = api.params_into(&[
        ("action", "parse"),
        ("page", "Combined_Leaderboards"),
        ("prop", "text")
    ]);

    println!("Starting query...");
    let result = api.get_query_api_json(&params).await?;
    let text = result.get("parse").unwrap().get("text").unwrap().get("*").unwrap().as_str().unwrap();
    let document = kuchiki::parse_html().one(text);
    println!("Got response");

    for row in document.select("tr").unwrap() {
        let node = row.as_node();
        
    }

    // for row in html.select(&row_selector) {
    //     //let mut cols = row.select(&col_selector);
    //     println!("{:?}", cols);
    //     break;
    // }

    //let mut rows = html.select(&row_selector);
    // rows.next();
    // rows.next();

    //let row = rows.next().unwrap();
    //let cols = row.select(&col_selector).next().unwrap();


    Ok(Vec::new())
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



