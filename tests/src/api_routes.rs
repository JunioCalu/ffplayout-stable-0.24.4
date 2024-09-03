use actix_web::{get, web, App, Error, HttpResponse, Responder};
// use actix_web_httpauth::extractors::bearer::BearerAuth;

use serde_json::json;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};

use ffplayout::api::routes::login;
use ffplayout::db::{
    handles,
    models::{init_globales, User},
};
use ffplayout::player::controller::ChannelManager;
use ffplayout::utils::config::PlayoutConfig;
// use ffplayout::validator;

async fn prepare_config() -> (PlayoutConfig, ChannelManager, Pool<Sqlite>) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET public_root = "assets/hls", logging_path = "assets/log", playlist_root = "assets/playlists", storage_root = "assets/storage";
        UPDATE channels SET hls_path = "assets/hls", playlist_path = "assets/playlists", storage_path = "assets/storage";
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let user = User {
        id: 0,
        mail: Some("admin@mail.com".to_string()),
        username: "admin".to_string(),
        password: "admin".to_string(),
        role_id: Some(1),
        channel_ids: Some(vec![1]),
        token: None,
    };

    handles::insert_user(&pool, user.clone()).await.unwrap();

    let config = PlayoutConfig::new(&pool, 1).await;
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(Some(pool.clone()), channel, config.clone());

    (config, manager, pool)
}

#[get("/")]
async fn get_handler() -> Result<impl Responder, Error> {
    Ok(HttpResponse::Ok())
}

#[actix_rt::test]
async fn test_get() {
    let srv = actix_test::start(|| App::new().service(get_handler));

    let req = srv.get("/");
    let res = req.send().await.unwrap();

    assert!(res.status().is_success());
}

#[actix_rt::test]
async fn test_login() {
    let (_, _, pool) = prepare_config().await;

    init_globales(&pool).await;

    let srv = actix_test::start(move || {
        let db_pool = web::Data::new(pool.clone());
        App::new().app_data(db_pool).service(login)
    });

    let payload = json!({"username": "admin", "password": "admin"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert!(res.status().is_success());

    let payload = json!({"username": "admin", "password": "1234"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert_eq!(res.status().as_u16(), 403);

    let payload = json!({"username": "aaa", "password": "1234"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert_eq!(res.status().as_u16(), 400);
}