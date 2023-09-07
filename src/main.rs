use std::sync::Arc;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use serde::Deserialize;
use serde::Serialize;
use tokio_postgres::Config;
use warp::Filter;
use warp::http::Method;

#[derive(Serialize)]
pub struct Tweet {
    rest_id: String,
    sort_index: String,
    screen_name: String,
    created_at: chrono::NaiveDateTime,
    full_text: String,
    bookmarked: bool,
    liked: bool,
    category: Option<String>,
    important: bool,
    archived: bool,
}

#[derive(Deserialize)]
pub struct Pagination {
    page_size: Option<i64>,
    page_number: Option<i64>,
    hide_archived: Option<bool>,
    hide_categorized: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateTweet {
    category: Option<String>,
    important: Option<bool>,
    archived: Option<bool>,
}

mod handlers {
    use std::sync::Arc;

    use deadpool_postgres::Pool;

    use crate::{Pagination, Tweet, UpdateTweet};

    #[derive(Debug)]
    struct CustomRejection(String);

    impl warp::reject::Reject for CustomRejection {}

    pub async fn get_tweets(pagination: Pagination, pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
        let client = pool.get().await.unwrap();

        let size = pagination.page_size.unwrap_or(10);
        let number = pagination.page_number.unwrap_or(1);
        let offset = (number - 1) * size;

        let mut where_clauses = Vec::new();
        where_clauses.push("1 = 1");

        if pagination.hide_archived.unwrap_or(true) {
            where_clauses.push("archived = false");
        };

        if pagination.hide_categorized.unwrap_or(true) {
            where_clauses.push("(category is null OR category = '')")
        }

        let query = format!("SELECT * FROM tweets WHERE {} ORDER BY sort_index DESC LIMIT $1 OFFSET $2", where_clauses.join(" AND "));

        let stmt = client.prepare(&query).await.unwrap();
        let rows = client.query(&stmt, &[&size, &offset]).await.unwrap();

        // Convert rows to your Tweet struct and then to JSON
        let tweets: Vec<Tweet> = rows.iter().map(|row| {
            Tweet {
                rest_id: row.get(0),
                sort_index: row.get(1),
                screen_name: row.get(2),
                created_at: row.get(3),
                full_text: row.get(4),
                bookmarked: row.get(5),
                liked: row.get(6),
                category: row.get(7),
                important: row.get(8),
                archived: row.get(9),
            }
        }).collect();

        Ok(warp::reply::json(&tweets))
    }

    pub async fn patch_tweet(rest_id: String, update: UpdateTweet, pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
        let client = pool.get().await.unwrap();

        let mut set_clauses = Vec::new();
        let mut values: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();

        let mut counter = 1;

        if let Some(ref category) = update.category {
            //FIXME: handle setting null
            set_clauses.push(format!("category = ${}", counter));
            values.push(category);
            counter += 1;
        }

        if let Some(ref important) = update.important {
            set_clauses.push(format!("important = ${}", counter));
            values.push(important);
            counter += 1;
        }

        if let Some(ref archived) = update.archived {
            set_clauses.push(format!("archived = ${}", counter));
            values.push(archived);
            counter += 1;
        }

        if set_clauses.is_empty() {
            return Err(warp::reject::custom(CustomRejection(String::from("No valid fields provided for update"))));
        }

        let set_clause = set_clauses.join(", ");
        let sql = format!("UPDATE tweets SET {} WHERE rest_id = ${}", set_clause, counter);
        values.push(&rest_id);

        let stmt = client.prepare(&sql).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();

        Ok(warp::reply::with_status("Updated", warp::http::StatusCode::NO_CONTENT))
    }

    pub async fn get_categories(pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
        let client = pool.get().await.unwrap();

        let stmt = client.prepare("SELECT category FROM tweets WHERE category IS NOT NULL GROUP BY category ORDER BY count(*) DESC").await.unwrap();
        let rows = client.query(&stmt, &[]).await.unwrap();

        let categories: Vec<String> = rows.iter().map(|row| {
            row.get::<_, Option<String>>(0).unwrap_or_default() // Since category can be NULL, we handle it with Option<String>
        }).collect();

        Ok(warp::reply::json(&categories))
    }
}

#[tokio::main]
async fn main() {
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

    let mgr = Manager::from_config(cfg, tokio_postgres::NoTls, mgrcfg );
    let pool = Arc::new(Pool::builder(mgr).max_size(16).build().unwrap());
    let with_pool = warp::any().map(move || Arc::clone(&pool));

    let tweets = warp::path("tweets")
        .and(warp::get())
        .and(warp::query::<Pagination>())
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

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec![Method::GET, Method::POST, Method::PATCH, Method::OPTIONS]) // Add any other methods you want to allow here
        .allow_headers(vec!["Content-Type", "User-Agent", "Authorization"]) // Add any other headers you expect
        .build();

    let routes = tweets.or(update).or(categories).with(cors);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

