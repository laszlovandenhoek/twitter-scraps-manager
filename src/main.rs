use std::sync::Arc;

use deadpool_postgres::{BuildError, Manager, ManagerConfig, Pool, RecyclingMethod};
use serde::Deserialize;
use serde::Serialize;
use tokio_postgres::Config;
use warp::Filter;
use warp::http::Method;

mod handlers;

#[derive(Serialize)]
pub struct Tweet {
    rest_id: String,
    sort_index: String,
    screen_name: String,
    created_at: chrono::NaiveDateTime,
    fetched_at: chrono::NaiveDateTime,
    full_text: String,
    quoted_text: String,
    bookmarked: bool,
    liked: bool,
    categories: Vec<String>,
    important: bool,
    archived: bool,
}

#[derive(Serialize)]
pub struct Info {
    total: i64,
    categorized: i64,
    uncategorized: i64,
    archived: i64,
    important: i64,
}

#[derive(Deserialize)]
pub struct Parameters {
    page_size: Option<i64>,
    page_number: Option<i64>,
    hide_archived: Option<bool>,
    hide_categorized: Option<bool>,
    search: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTweet {
    add_category: Option<String>,
    remove_category: Option<String>,
    important: Option<bool>,
    archived: Option<bool>,
}

#[tokio::main]
async fn main() {
    let pool_reference = Arc::new(get_pool().unwrap());

    let with_pool = warp::any().map(move || Arc::clone(&pool_reference));

    let tweets = warp::path("tweets")
        .and(warp::get())
        .and(warp::query::<Parameters>())
        .and(with_pool.clone())
        .and_then(handlers::get_tweets);

    let update = warp::path!("tweets" / String)
        .and(warp::patch())
        .and(warp::body::json())
        .and(with_pool.clone())
        .and_then(handlers::patch_tweet);

    let categories = warp::path("categories")
        .and(warp::get())
        .and(with_pool.clone())
        .and_then(handlers::get_categories);

    let info = warp::path("info")
        .and(warp::get())
        .and(with_pool.clone())
        .and_then(handlers::get_info);

    let static_content = warp::path::param::<String>()
        .and(warp::get())
        .and_then(handlers::get_static);

    let root = warp::path!()
        .and(warp::get())
        .and_then(|| handlers::get_static("index.html".to_string()));

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec![Method::GET, Method::POST, Method::PATCH, Method::OPTIONS]) // Add any other methods you want to allow here
        .allow_headers(vec!["Content-Type", "User-Agent", "Authorization"]) // Add any other headers you expect
        .build();

    env_logger::init();
    let log = warp::log("root");

    let routes = tweets
        .or(update)
        .or(categories)
        .or(info)
        .or(static_content)
        .or(root)
        .with(cors)
        .with(log);

    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

fn get_pool() -> Result<Pool, BuildError> {
    let mut cfg = Config::new();

    dotenv::dotenv().ok();

    let database_host =
        std::env::var("DATABASE_HOST")
            .unwrap_or("localhost".to_string());
    let database_user =
        std::env::var("DATABASE_USER")
            .expect("DATABASE_USER must be set");
    let database_password =
        std::env::var("DATABASE_PASSWORD")
            .expect("DATABASE_PASSWORD must be set");
    let database_name =
        std::env::var("DATABASE_NAME")
            .unwrap_or("postgres".to_string());

    cfg.host(&database_host);
    cfg.user(&database_user);
    cfg.password(&database_password);
    cfg.dbname(&database_name);

    let mgrcfg = ManagerConfig {
        recycling_method: RecyclingMethod::Verified,
    };

    let mgr = Manager::from_config(cfg, tokio_postgres::NoTls, mgrcfg);
    return Pool::builder(mgr).max_size(16).build();
}
