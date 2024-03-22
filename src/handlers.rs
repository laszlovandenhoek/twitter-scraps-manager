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

    let mut having_clauses = Vec::new();
    having_clauses.push("1 = 1");

    if parameters.hide_archived.unwrap_or(true) {
        where_clauses.push("archived = false");
    };

    if parameters.hide_categorized.unwrap_or(true) {
        having_clauses.push("array_length(array_remove(array_agg(c.name), NULL), 1) IS NULL");
    }

    let search_terms: Vec<String> = parameters.search
        .map(|s| s.split_whitespace().map(|word| format!("%{}%", word).to_string()).collect())
        .unwrap_or_else(|| vec![]);

    // LIMIT will be $1 and OFFSET will be $2, so start at $3
    let mut argument_index = 3;
    let mut terms_clauses: Vec<String> = Vec::new();
    for _ in 0..search_terms.len() {
        let string = format!("(rest_id ILIKE ${} OR screen_name ILIKE ${} OR full_text ILIKE ${} OR quoted_text ILIKE ${} OR BOOL_OR(c.name ILIKE ${}))", argument_index, argument_index, argument_index, argument_index, argument_index);
        argument_index += 1;
        terms_clauses.push(string);
    }

    //append all clauses pertaining to terms to the main vector of having clauses
    terms_clauses.iter().for_each(|term| having_clauses.push(term));

    let mut query_parameters: Vec<&(dyn ToSql + Sync)> = vec!(&size, &offset);
    search_terms.iter().for_each(|term| query_parameters.push(term));

    let query = format!("SELECT \
        tweets.rest_id, \
        tweets.sort_index, \
        tweets.screen_name, \
        tweets.created_at, \
        tweets.fetched_at, \
        tweets.full_text, \
        tweets.bookmarked, \
        tweets.liked, \
        tweets.important, \
        tweets.archived, \
        COALESCE(tweets.quoted_text, '') as quoted_text, \
        array_remove(array_agg(c.name), NULL) as categories FROM tweets \
        LEFT JOIN tweet_categories ON tweets.rest_id = tweet_categories.tweet_id \
        LEFT JOIN categories c ON tweet_categories.category_id = c.id \
        WHERE {} \
        GROUP BY tweets.rest_id, fetched_at, sort_index \
        HAVING {} \
        ORDER BY created_at DESC \
        LIMIT $1 OFFSET $2", where_clauses.join(" AND "), having_clauses.join(" AND "));

    let stmt = client.prepare(&query).await.unwrap();
    let rows = client.query(&stmt, &query_parameters).await.unwrap();

    // Convert rows to your Tweet struct and then to JSON
    let tweets: Vec<Tweet> = rows.iter().map(|row| {
        Tweet {
            rest_id: row.get("rest_id"),
            sort_index: row.get("sort_index"),
            screen_name: row.get("screen_name"),
            created_at: row.get("created_at"),
            fetched_at: row.get("fetched_at"),
            full_text: row.get("full_text"),
            quoted_text: row.get("quoted_text"),
            bookmarked: row.get("bookmarked"),
            liked: row.get("liked"),
            categories: row.get("categories"),
            important: row.get("important"),
            archived: row.get("archived"),
        }
    }).collect();

    Ok(warp::reply::json(&tweets))
}

pub async fn patch_tweet(rest_id: String, update: UpdateTweet, pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
    let client = pool.get().await.unwrap();

    let mut changed = false;

    if let Some(ref category) = update.add_category {
        let mut values: Vec<&(dyn ToSql + Sync)> = Vec::new();

        //ensure category exists
        let ensure_category = "INSERT INTO categories (name) VALUES ($1) ON CONFLICT (name) DO NOTHING;";

        values.push(category);

        let stmt = client.prepare(&ensure_category).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();

        //register category for tweet
        let register_tweet_category = "INSERT INTO tweet_categories (tweet_id, category_id) SELECT $2, id FROM categories WHERE name = $1;";

        values.push(&rest_id);

        let stmt = client.prepare(&register_tweet_category).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();

        changed = true;
    }

    if let Some(ref category) = update.remove_category {
        let mut values: Vec<&(dyn ToSql + Sync)> = Vec::new();

        let remove_category_sql = "DELETE FROM tweet_categories WHERE tweet_id = $1 AND category_id = (SELECT id FROM categories WHERE name = $2);";

        values.push(&rest_id);
        values.push(category);

        let stmt = client.prepare(&remove_category_sql).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();

        let remove_orphans_sql = "DELETE FROM categories WHERE id NOT IN (SELECT category_id FROM tweet_categories);";
        client.simple_query(remove_orphans_sql).await.unwrap();

        changed = true;
    }

    if let Some(ref important) = update.important {
        let mut values: Vec<&(dyn ToSql + Sync)> = Vec::new();

        let sql = "UPDATE tweets SET important = $2 WHERE rest_id = $1";
        values.push(&rest_id);
        values.push(important);
        let stmt = client.prepare(&sql).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();
        changed = true;
    }

    if let Some(ref archived) = update.archived {
        let mut values: Vec<&(dyn ToSql + Sync)> = Vec::new();

        let sql = "UPDATE tweets SET archived = $2 WHERE rest_id = $1";
        values.push(&rest_id);
        values.push(archived);
        let stmt = client.prepare(&sql).await.unwrap();
        client.execute(&stmt, &values).await.unwrap();
        changed = true;
    }

    if !changed {
        return Err(warp::reject::custom(CustomRejection(String::from("No valid fields provided for update"))));
    }

    Ok(warp::reply::with_status("Updated", warp::http::StatusCode::NO_CONTENT))
}

pub async fn get_categories(pool: Arc<Pool>) -> Result<impl warp::Reply, warp::Rejection> {
    let client = pool.get().await.unwrap();

    let stmt = client.prepare("SELECT name FROM tweet_categories join categories on tweet_categories.category_id = categories.id GROUP BY name ORDER BY count(*) DESC").await.unwrap();
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
     (SELECT COUNT(*) num FROM tweets WHERE rest_id NOT IN (SELECT tweet_id FROM tweet_categories)) as uncategorized,
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
