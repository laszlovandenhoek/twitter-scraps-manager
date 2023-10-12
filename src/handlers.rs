use std::sync::Arc;

use deadpool_postgres::Pool;
use include_dir::{Dir, include_dir};
use tokio_postgres::types::ToSql;

use crate::{Info, Parameters, Tweet, UpdateTweet};

#[derive(Debug)]
struct CustomRejection(String);

impl warp::reject::Reject for CustomRejection {}

const STATIC_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/static");

pub async fn get_tweets(parameters: Parameters, pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
    let client = pool.get().await.unwrap();

    let size = parameters.page_size.unwrap_or(10);
    let number = parameters.page_number.unwrap_or(1);
    let offset = (number - 1) * size;

    let mut where_clauses = Vec::new();
    where_clauses.push("1 = 1");

    if parameters.hide_archived.unwrap_or(true) {
        where_clauses.push("archived = false");
    };

    if parameters.hide_categorized.unwrap_or(true) {
        where_clauses.push("(category is null OR category = '')");
    }

    let search_terms: Vec<String> = parameters.search
        .map(|s| s.split_whitespace().map(|word| format!("%{}%", word).to_string()).collect())
        .unwrap_or_else(|| vec![]);

    // LIMIT will be $1 and OFFSET will be $2, so start at $3
    let mut argument_index = 3;
    let mut terms_clauses: Vec<String> = Vec::new();
    for _ in 0..search_terms.len() {
        let string = format!("(screen_name ILIKE ${} OR full_text ILIKE ${} OR category ILIKE ${})", argument_index, argument_index, argument_index);
        argument_index += 1;
        terms_clauses.push(string);
    }

    //append all clauses pertaining to terms to the main vector of where clauses
    terms_clauses.iter().for_each(|term| where_clauses.push(term));

    let mut query_parameters: Vec<&(dyn ToSql + Sync)> = vec!(&size, &offset);
    search_terms.iter().for_each(|term| query_parameters.push(term));

    let query = format!("SELECT * FROM tweets WHERE {} ORDER BY sort_index DESC LIMIT $1 OFFSET $2", where_clauses.join(" AND "));
    let stmt = client.prepare(&query).await.unwrap();
    let rows = client.query(&stmt, &query_parameters).await.unwrap();

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

pub async fn get_info(pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
    let client = pool.get().await.unwrap();

    let stmt = client.prepare("
select
    total.num                     as total,
    uncategorized.num             as uncategorized,
    total.num - uncategorized.num as categorized,
    archived.num                  as archived,
    important.num                 as important
from (select count(*) num from tweets) as total,
     (select count(*) num from tweets where category is null or category = '') as uncategorized,
     (select count(*) num from tweets where archived = true) as archived,
     (select count(*) num from tweets where important = true) as important"
    ).await.unwrap();
    let rows = client.query(&stmt, &[]).await.unwrap();

    let info = rows.get(0).map(|row| {
        Info {
            total: row.get(0),
            uncategorized: row.get(1),
            categorized: row.get(2),
            archived: row.get(3),
            important: row.get(4),
        }
    });

    Ok(warp::reply::json(&info))
}

pub async fn get_static(file_path: String) -> Result<impl warp::Reply, warp::Rejection> {
    if let Some(file) = STATIC_DIR.get_file(&file_path) {
        let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
        Ok(warp::reply::with_header(
            file.contents(),
            "content-type",
            mime.as_ref(),
        ))
    } else {
        Err(warp::reject::not_found())
    }
}
