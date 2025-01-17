/// ### Possible endpoints
///
/// Run the API thru the systemd service, or like:
///
/// ```BASH
/// ffplayout -l 127.0.0.1:8787
/// ```
///
/// For all endpoints an (Bearer) authentication is required.\
/// `{id}` represent the channel id, and at default is 1.
use std::{
    env,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc, Mutex},
    collections::HashMap,
};

use actix_files;
use actix_multipart::Multipart;
use actix_web::{
    delete, get,
    http::{
        header::{ContentDisposition, DispositionType},
        StatusCode,
    },
    patch, post, put, web, HttpRequest, HttpResponse, Responder,
};
use actix_web_grants::{authorities::AuthDetails, proc_macro::protect};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeDelta, TimeZone, Utc};
use log::*;
use path_clean::PathClean;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::fs;

use crate::db::models::Role;
use crate::utils::{
    channels::{create_channel, delete_channel},
    config::{get_config, PlayoutConfig, Template},
    control::{control_state, send_message, ControlParams, Process, ProcessCtl},
    errors::ServiceError,
    files::{
        browser, create_directory, norm_abs_path, remove_file_or_folder, rename_file, upload,
        MoveObject, PathObject,
    },
    naive_date_time_from_str,
    playlist::{delete_playlist, generate_playlist, read_playlist, write_playlist},
    public_path, read_log_file, system, TextFilter,
};
use crate::{
    api::auth::{create_jwt, Claims},
    utils::advanced_config::AdvancedConfig,
    vec_strings,
};
use crate::{
    db::{
        handles,
        models::{Channel, TextPreset, User, UserMeta},
    },
    player::controller::ChannelController,
};
use crate::{
    player::utils::{
        get_data_map, get_date_range, import::import_file, sec_to_time, time_to_sec, JsonPlaylist,
    },
    utils::logging::MailQueue,
};

use dirs::home_dir;
use once_cell::sync::Lazy;
use std::process::Stdio;
use tokio::fs::metadata;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::Duration;
use url::Url;
//use actix_web::route;
//use reqwest::Method;
//use shell_escape::escape;
//use std::borrow::Cow;
//use serde_json::json;
use actix_web::Scope;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::timeout;

#[derive(Serialize)]
struct UserObj<T> {
    message: String,
    user: Option<T>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DateObj {
    #[serde(default)]
    date: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct FileObj {
    #[serde(default)]
    path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PathsObj {
    #[serde(default)]
    paths: Option<Vec<String>>,
    template: Option<Template>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImportObj {
    #[serde(default)]
    file: PathBuf,
    #[serde(default)]
    date: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProgramObj {
    #[serde(default = "time_after", deserialize_with = "naive_date_time_from_str")]
    start_after: NaiveDateTime,
    #[serde(default = "time_before", deserialize_with = "naive_date_time_from_str")]
    start_before: NaiveDateTime,
}

fn time_after() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 0, 0, 0)
        .unwrap()
        .naive_local()
}

fn time_before() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 23, 59, 59)
        .unwrap()
        .naive_local()
}

#[derive(Debug, Serialize)]
struct ProgramItem {
    source: String,
    start: String,
    title: Option<String>,
    r#in: f64,
    out: f64,
    duration: f64,
    category: String,
    description: Option<String>,
    enable_description: Option<bool>,
}

/// #### User Handling
///
/// **Login**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/auth/login/ -H "Content-Type: application/json" \
/// -d '{ "username": "<USER>", "password": "<PASS>" }'
/// ```
/// **Response:**
///
/// ```JSON
/// {
///     "id": 1,
///     "mail": "user@example.org",
///     "username": "<USER>",
///     "token": "<TOKEN>"
/// }
/// ```
#[post("/auth/login/")]
pub async fn login(
    pool: web::Data<Pool<Sqlite>>,
    credentials: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    let username = credentials.username.clone();
    let password = credentials.password.clone();

    match handles::select_login(&pool, &username).await {
        Ok(mut user) => {
            let role = handles::select_role(&pool, &user.role_id.unwrap_or_default()).await?;

            let pass_hash = user.password.clone();
            let cred_password = password.clone();

            user.password = String::new();

            let verified_password = web::block(move || {
                let hash = PasswordHash::new(&pass_hash)?;
                Argon2::default().verify_password(cred_password.as_bytes(), &hash)
            })
            .await?;

            if verified_password.is_ok() {
                let claims = Claims::new(
                    user.id,
                    user.channel_ids.clone().unwrap_or_default(),
                    username.clone(),
                    role.clone(),
                );

                if let Ok(token) = create_jwt(claims).await {
                    user.token = Some(token);
                };

                info!("user {} login, with role: {role}", username);

                Ok(web::Json(UserObj {
                    message: "login correct!".into(),
                    user: Some(user),
                })
                .customize()
                .with_status(StatusCode::OK))
            } else {
                error!("Wrong password for {username}!");

                Ok(web::Json(UserObj {
                    message: "Wrong password!".into(),
                    user: None,
                })
                .customize()
                .with_status(StatusCode::FORBIDDEN))
            }
        }
        Err(e) => {
            error!("Login {username} failed! {e}");
            Ok(web::Json(UserObj {
                message: format!("Login {username} failed!"),
                user: None,
            })
            .customize()
            .with_status(StatusCode::BAD_REQUEST))
        }
    }
}

/// From here on all request **must** contain the authorization header:\
/// `"Authorization: Bearer <TOKEN>"`

/// **Get current User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/user")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role"
)]
async fn get_user(
    pool: web::Data<Pool<Sqlite>>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    match handles::select_user(&pool, user.id).await {
        Ok(user) => Ok(web::Json(user)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// **Get User by ID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/user/{id}")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn get_by_name(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match handles::select_user(&pool, *id).await {
        Ok(user) => Ok(web::Json(user)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

// **Get all User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/users' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/users")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn get_users(pool: web::Data<Pool<Sqlite>>) -> Result<impl Responder, ServiceError> {
    match handles::select_users(&pool).await {
        Ok(users) => Ok(web::Json(users)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// **Update current User**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/user/1 -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "password": "<PASS>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/user/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "*id == user.id || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_user(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<User>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let channel_ids = data.channel_ids.clone().unwrap_or_default();
    let mut fields = String::new();

    if let Some(mail) = data.mail.clone() {
        if !fields.is_empty() {
            fields.push_str(", ");
        }

        fields.push_str(&format!("mail = '{mail}'"));
    }

    if !data.password.is_empty() {
        if !fields.is_empty() {
            fields.push_str(", ");
        }

        let password_hash = web::block(move || {
            let salt = SaltString::generate(&mut OsRng);

            let argon = Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .map(|p| p.to_string());

            argon
        })
        .await?
        .unwrap();

        fields.push_str(&format!("password = '{password_hash}'"));
    }

    handles::update_user(&pool, *id, fields).await?;

    let related_channels = handles::select_related_channels(&pool, Some(*id)).await?;

    for channel in related_channels {
        if !channel_ids.contains(&channel.id) {
            handles::delete_user_channel(&pool, *id, channel.id).await?;
        }
    }

    handles::insert_user_channel(&pool, *id, channel_ids).await?;

    Ok("Update Success")
}

/// **Add User**
///
/// ```BASH
/// curl -X POST 'http://127.0.0.1:8787/api/user/' -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/user/")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn add_user(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    match handles::insert_user(&pool, data.into_inner()).await {
        Ok(..) => Ok("Add User Success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

// **Delete User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/user/{id}")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn remove_user(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match handles::delete_user(&pool, *id).await {
        Ok(_) => return Ok("Delete user success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// #### Settings
///
/// **Get Settings from Channel**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/channel/1 -H "Authorization: Bearer <TOKEN>"
/// ```
///
/// **Response:**
///
/// ```JSON
/// {
///     "id": 1,
///     "name": "Channel 1",
///     "preview_url": "http://localhost/live/preview.m3u8",
///     "extra_extensions": "jpg,jpeg,png",
///     "utc_offset": "+120"
/// }
/// ```
#[get("/channel/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_channel(&pool, &id).await {
        return Ok(web::Json(channel));
    }

    Err(ServiceError::InternalServerError)
}

/// **Get settings from all Channels**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/channels -H "Authorization: Bearer <TOKEN>"
/// ```
#[get("/channels")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role"
)]
async fn get_all_channels(
    pool: web::Data<Pool<Sqlite>>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_related_channels(&pool, Some(user.id)).await {
        return Ok(web::Json(channel));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Channel**
///
/// ```BASH
/// curl -X PATCH http://127.0.0.1:8787/api/channel/1 -H "Content-Type: application/json" \
/// -d '{ "id": 1, "name": "Channel 1", "preview_url": "http://localhost/live/stream.m3u8", "extra_extensions": "jpg,jpeg,png"}' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[patch("/channel/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn patch_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<Channel>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .unwrap()
        .get(*id)
        .ok_or_else(|| format!("Channel {id} not found!"))?;
    let mut data = data.into_inner();

    if !role.has_authority(&Role::GlobalAdmin) {
        let channel = handles::select_channel(&pool, &id).await?;

        data.public = channel.public;
        data.playlists = channel.playlists;
        data.storage = channel.storage;
    }

    handles::update_channel(&pool, *id, data).await?;
    let new_config = get_config(&pool, *id).await?;
    manager.update_config(new_config);

    Ok("Update Success")
}

/// **Create new Channel**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/channel/ -H "Content-Type: application/json" \
/// -d '{ "name": "Channel 2", "preview_url": "http://localhost/live/channel2.m3u8", "extra_extensions": "jpg,jpeg,png" }' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[post("/channel/")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn add_channel(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<Channel>,
    controllers: web::Data<Mutex<ChannelController>>,
    queue: web::Data<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<impl Responder, ServiceError> {
    match create_channel(
        &pool,
        controllers.into_inner(),
        queue.into_inner(),
        data.into_inner(),
    )
    .await
    {
        Ok(c) => Ok(web::Json(c)),
        Err(e) => Err(e),
    }
}

/// **Delete Channel**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/channel/2 -H "Authorization: Bearer <TOKEN>"
/// ```
#[delete("/channel/{id}")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn remove_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    queue: web::Data<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<impl Responder, ServiceError> {
    if delete_channel(&pool, *id, controllers.into_inner(), queue.into_inner())
        .await
        .is_ok()
    {
        return Ok("Delete Channel Success");
    }

    Err(ServiceError::InternalServerError)
}

/// #### ffplayout Config
///
/// **Get Advanced Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/advanced/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
#[get("/playout/advanced/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_advanced_config(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .unwrap()
        .get(*id)
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel ({id}) not exists!")))?;
    let config = manager.config.lock().unwrap().advanced.clone();

    Ok(web::Json(config))
}

/// **Update Advanced Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/advanced/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/playout/advanced/{id}")]
#[protect(
    "Role::GlobalAdmin",
    "Role::ChannelAdmin",
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_advanced_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<AdvancedConfig>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();

    handles::update_advanced_configuration(&pool, *id, data.clone()).await?;
    let new_config = get_config(&pool, *id).await?;

    manager.update_config(new_config);

    Ok(web::Json("Update success"))
}

/// **Get Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/config/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
#[get("/playout/config/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_playout_config(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .unwrap()
        .get(*id)
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel ({id}) not exists!")))?;
    let config = manager.config.lock().unwrap().clone();

    Ok(web::Json(config))
}

/// **Update Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/config/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/playout/config/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_playout_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    mut data: web::Json<PlayoutConfig>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let p = manager.channel.lock().unwrap().storage.clone();
    let storage = Path::new(&p);
    let config_id = manager.config.lock().unwrap().general.id;

    let (_, _, logo) = norm_abs_path(storage, &data.processing.logo)?;
    let (_, _, filler) = norm_abs_path(storage, &data.storage.filler)?;
    let (_, _, font) = norm_abs_path(storage, &data.text.font)?;

    data.processing.logo = logo;
    data.storage.filler = filler;
    data.text.font = font;

    handles::update_configuration(&pool, config_id, data.clone()).await?;
    let new_config = get_config(&pool, *id).await?;

    manager.update_config(new_config);

    Ok(web::Json("Update success"))
}

/// #### Text Presets
///
/// Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.
///
/// **Get all Presets**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/presets/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_presets(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(presets) = handles::select_presets(&pool, *id).await {
        return Ok(web::Json(presets));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Preset**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "<TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/presets/{channel}/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&path.0) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_preset(
    pool: web::Data<Pool<Sqlite>>,
    path: web::Path<(i32, i32)>,
    data: web::Json<TextPreset>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (_, id) = path.into_inner();

    if handles::update_preset(&pool, &id, data.into_inner())
        .await
        .is_ok()
    {
        return Ok("Update Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Add new Preset**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/presets/1/ -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/presets/{id}/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn add_preset(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<TextPreset>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if handles::insert_preset(&pool, data.into_inner())
        .await
        .is_ok()
    {
        return Ok("Add preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Delete Preset**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/presets/1/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/presets/{channel}/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&path.0) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn delete_preset(
    pool: web::Data<Pool<Sqlite>>,
    path: web::Path<(i32, i32)>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (_, id) = path.into_inner();

    if handles::delete_preset(&pool, &id).await.is_ok() {
        return Ok("Delete preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// ### ffplayout controlling
///
/// here we communicate with the engine for:
/// - jump to last or next clip
/// - reset playlist state
/// - get infos about current, next, last clip
/// - send text to the engine, for overlaying it (as lower third etc.)
///
/// **Send Text to ffplayout**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/text/ \
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>' \
/// -d '{"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", fontsize": "24", "line_spacing": "4", "fontcolor": "#ffffff", "box": "1", "boxcolor": "#000000", "boxborderw": "4", "alpha": "1.0"}'
/// ```
#[post("/control/{id}/text/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn send_text_message(
    id: web::Path<i32>,
    data: web::Json<TextFilter>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();

    match send_message(manager, data.into_inner()).await {
        Ok(res) => Ok(web::Json(res)),
        Err(e) => Err(e),
    }
}

/// **Control Playout**
///
/// - next
/// - back
/// - reset
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/playout/ -H 'Content-Type: application/json'
/// -d '{ "command": "reset" }' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/control/{id}/playout/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn control_playout(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    control: web::Json<ControlParams>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();

    if manager.is_processing.load(Ordering::SeqCst) {
        return Err(ServiceError::Conflict(
            "A command is already being processed, please wait".to_string(),
        ));
    }

    manager.is_processing.store(true, Ordering::SeqCst);

    let resp = match control_state(&pool, &manager, &control.control).await {
        Ok(res) => Ok(web::Json(res)),
        Err(e) => Err(e),
    };

    manager.is_processing.store(false, Ordering::SeqCst);

    resp
}

/// **Get current Clip**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/control/1/media/current
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// **Response:**
///
/// ```JSON
///     {
///       "media": {
///         "category": "",
///         "duration": 154.2,
///         "out": 154.2,
///         "in": 0.0,
///         "source": "/opt/tv-media/clip.mp4"
///       },
///       "index": 39,
///       "ingest": false,
///       "mode": "playlist",
///       "played": 67.808
///     }
/// ```
#[get("/control/{id}/media/current")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn media_current(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let media_map = get_data_map(&manager);

    Ok(web::Json(media_map))
}

/// #### ffplayout Process Control
///
/// Control ffplayout process, like:
/// - start
/// - stop
/// - restart
/// - status
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/process/
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// -d '{"command": "start"}'
/// ```
#[post("/control/{id}/process/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn process_control(
    id: web::Path<i32>,
    proc: web::Json<Process>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    manager.list_init.store(true, Ordering::SeqCst);

    if manager.is_processing.load(Ordering::SeqCst) {
        return Err(ServiceError::Conflict(
            "A command is already being processed, please wait".to_string(),
        ));
    }

    manager.is_processing.store(true, Ordering::SeqCst);

    match proc.into_inner().command {
        ProcessCtl::Status => {
            manager.is_processing.store(false, Ordering::SeqCst);

            if manager.is_alive.load(Ordering::SeqCst) {
                return Ok(web::Json("active"));
            }
            return Ok(web::Json("not running"));
        }
        ProcessCtl::Start => {
            if !manager.is_alive.load(Ordering::SeqCst) {
                manager.channel.lock().unwrap().active = true;
                manager.async_start().await;
            }
        }
        ProcessCtl::Stop => {
            manager.channel.lock().unwrap().active = false;
            manager.async_stop().await?;
        }
        ProcessCtl::Restart => {
            manager.async_stop().await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

            if !manager.is_alive.load(Ordering::SeqCst) {
                manager.async_start().await;
            }
        }
    }

    manager.is_processing.store(false, Ordering::SeqCst);

    Ok(web::Json("Success"))
}

/// #### ffplayout Playlist Operations
///
/// **Get playlist**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playlist/1?date=2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/playlist/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_playlist(
    id: web::Path<i32>,
    obj: web::Query<DateObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    match read_playlist(&config, obj.date.clone()).await {
        Ok(playlist) => Ok(web::Json(playlist)),
        Err(e) => Err(e),
    }
}

/// **Save playlist**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// --data "{<JSON playlist data>}"
/// ```
#[post("/playlist/{id}/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn save_playlist(
    id: web::Path<i32>,
    data: web::Json<JsonPlaylist>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    match write_playlist(&config, data.into_inner()).await {
        Ok(res) => Ok(web::Json(res)),
        Err(e) => Err(e),
    }
}

/// **Generate Playlist**
///
/// A new playlist will be generated and response.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// /// --data '{ "paths": [<list of paths>] }' # <- data is optional
/// ```
///
/// Or with template:
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2023-00-05
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// --data '{"template": {"sources": [\
///            {"start": "00:00:00", "duration": "10:00:00", "shuffle": true, "paths": ["path/1", "path/2"]}, \
///            {"start": "10:00:00", "duration": "14:00:00", "shuffle": false, "paths": ["path/3", "path/4"]}]}}'
/// ```
#[post("/playlist/{id}/generate/{date}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&params.0) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn gen_playlist(
    params: web::Path<(i32, String)>,
    data: Option<web::Json<PathsObj>>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(params.0).unwrap();
    manager.config.lock().unwrap().general.generate = Some(vec![params.1.clone()]);
    let storage = manager.config.lock().unwrap().channel.storage.clone();

    if let Some(obj) = data {
        if let Some(paths) = &obj.paths {
            let mut path_list = vec![];

            for path in paths {
                let (p, _, _) = norm_abs_path(&storage, path)?;

                path_list.push(p);
            }

            manager.config.lock().unwrap().storage.paths = path_list;
        }

        manager
            .config
            .lock()
            .unwrap()
            .general
            .template
            .clone_from(&obj.template);
    }

    match generate_playlist(manager) {
        Ok(playlist) => Ok(web::Json(playlist)),
        Err(e) => Err(e),
    }
}

/// **Delete Playlist**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/playlist/1/2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/playlist/{id}/{date}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&params.0) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn del_playlist(
    params: web::Path<(i32, String)>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(params.0).unwrap();
    let config = manager.config.lock().unwrap().clone();

    match delete_playlist(&config, &params.1).await {
        Ok(m) => Ok(web::Json(m)),
        Err(e) => Err(e),
    }
}

/// ### Log file
///
/// **Read Log File**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/log/1?date=2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/log/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_log(
    id: web::Path<i32>,
    log: web::Query<DateObj>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    read_log_file(&id, &log.date).await
}

/// ### File Operations
///
/// **Get File/Folder List**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/browse/ -H 'Content-Type: application/json'
/// -d '{ "source": "/" }' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/browse/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn file_browser(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let channel = manager.channel.lock().unwrap().clone();
    let config = manager.config.lock().unwrap().clone();

    match browser(&config, &channel, &data.into_inner()).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Create Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/create-folder/ -H 'Content-Type: application/json'
/// -d '{"source": "<FOLDER PATH>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/create-folder/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn add_dir(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    create_directory(&config, &data.into_inner()).await
}

/// **Rename File**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/rename/ -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/rename/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn move_rename(
    id: web::Path<i32>,
    data: web::Json<MoveObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    match rename_file(&config, &data.into_inner()).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Remove File/Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/remove/ -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/remove/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn remove(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();
    let recursive = data.recursive;

    match remove_file_or_folder(&config, &data.into_inner().source, recursive).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Upload File**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/file/1/upload/ -H 'Authorization: Bearer <TOKEN>'
/// -F "file=@file.mp4"
/// ```
#[allow(clippy::too_many_arguments)]
#[put("/file/{id}/upload/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn save_file(
    id: web::Path<i32>,
    req: HttpRequest,
    payload: Multipart,
    obj: web::Query<FileObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    let size: u64 = req
        .headers()
        .get("content-length")
        .and_then(|cl| cl.to_str().ok())
        .and_then(|cls| cls.parse().ok())
        .unwrap_or(0);

    upload(&config, size, payload, &obj.path, false).await
}

/// **Get File**
///
/// Can be used for preview video files
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/file/1/path/to/file.mp4
/// ```
#[get("/file/{id}/{filename:.*}")]
async fn get_file(
    req: HttpRequest,
    controllers: web::Data<Mutex<ChannelController>>,
) -> Result<actix_files::NamedFile, ServiceError> {
    let id: i32 = req.match_info().query("id").parse()?;
    let manager = controllers.lock().unwrap().get(id).unwrap();
    let config = manager.config.lock().unwrap();
    let storage = config.channel.storage.clone();
    let file_path = req.match_info().query("filename");
    let (path, _, _) = norm_abs_path(&storage, file_path)?;
    let file = actix_files::NamedFile::open(path)?;

    Ok(file
        .use_last_modified(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Attachment,
            parameters: vec![],
        }))
}

/// **Get Public**
///
/// Can be used for HLS Playlist and other static files in public folder
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/1/live/stream.m3u8
/// ```
#[get("/{id}/{public:live|preview|public}/{file_stem:.*}")]
async fn get_public(
    path: web::Path<(i32, String, String)>,
    controllers: web::Data<Mutex<ChannelController>>,
) -> Result<actix_files::NamedFile, ServiceError> {
    let (id, public, file_stem) = path.into_inner();

    let absolute_path = if file_stem.ends_with(".ts")
        || file_stem.ends_with(".m3u8")
        || file_stem.ends_with(".vtt")
    {
        let manager = controllers.lock().unwrap().get(id).unwrap();
        let config = manager.config.lock().unwrap();
        config.channel.public.join(public)
    } else {
        public_path()
    }
    .clean();

    let path = absolute_path.join(file_stem.as_str());
    let file = actix_files::NamedFile::open(path)?;

    Ok(file
        .use_last_modified(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Attachment,
            parameters: vec![],
        }))
}

/// **Import playlist**
///
/// Import text/m3u file and convert it to a playlist
/// lines with leading "#" will be ignore
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/file/1/import/ -H 'Authorization: Bearer <TOKEN>'
/// -F "file=@list.m3u"
/// ```
#[allow(clippy::too_many_arguments)]
#[put("/file/{id}/import/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn import_playlist(
    id: web::Path<i32>,
    req: HttpRequest,
    payload: Multipart,
    obj: web::Query<ImportObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let channel_name = manager.channel.lock().unwrap().name.clone();
    let config = manager.config.lock().unwrap().clone();
    let file = obj.file.file_name().unwrap_or_default();
    let path = env::temp_dir().join(file);
    let path_clone = path.clone();
    let size: u64 = req
        .headers()
        .get("content-length")
        .and_then(|cl| cl.to_str().ok())
        .and_then(|cls| cls.parse().ok())
        .unwrap_or(0);

    upload(&config, size, payload, &path, true).await?;

    let response =
        web::block(move || import_file(&config, &obj.date, Some(channel_name), &path_clone))
            .await??;

    fs::remove_file(path).await?;

    Ok(HttpResponse::Ok().body(response))
}

/// **Program info**
///
/// Get program infos about given date, or current day
///
/// Examples:
///
/// * get program from current day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/ -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get a program range between two dates
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T12:00:00&start_before=2022-11-20T11:59:59 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get program from give day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T10:00:00 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/program/{id}/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_program(
    id: web::Path<i32>,
    obj: web::Query<ProgramObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();
    let id = config.general.channel_id;
    let start_sec = config.playlist.start_sec.unwrap();
    let mut days = 0;
    let mut program = vec![];
    let after = obj.start_after;
    let mut before = obj.start_before;

    if after > before {
        before = chrono::Local
            .with_ymd_and_hms(after.year(), after.month(), after.day(), 23, 59, 59)
            .unwrap()
            .naive_local();
    }

    if start_sec > time_to_sec(&after.format("%H:%M:%S").to_string()) {
        days = 1;
    }

    let date_range = get_date_range(
        id,
        &vec_strings![
            (after - TimeDelta::try_days(days).unwrap_or_default()).format("%Y-%m-%d"),
            "-",
            before.format("%Y-%m-%d")
        ],
    );

    for date in date_range {
        let mut naive = NaiveDateTime::parse_from_str(
            &format!("{date} {}", sec_to_time(start_sec)),
            "%Y-%m-%d %H:%M:%S%.3f",
        )
        .unwrap();

        let playlist = match read_playlist(&config, date.clone()).await {
            Ok(p) => p,
            Err(e) => {
                error!("Error in Playlist from {date}: {e}");
                continue;
            }
        };

        for item in playlist.program {
            let start: DateTime<Local> = Local.from_local_datetime(&naive).unwrap();

            let source = match Regex::new(&config.text.regex)
                .ok()
                .and_then(|r| r.captures(&item.source))
            {
                Some(t) => t[1].to_string(),
                None => item.source,
            };

            let p_item = ProgramItem {
                source,
                start: start.format("%Y-%m-%d %H:%M:%S%.3f%:z").to_string(),
                title: item.title,
                r#in: item.seek,
                out: item.out,
                duration: item.duration,
                category: item.category,
                description: item.description,
                enable_description: item.enable_description,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive += TimeDelta::try_milliseconds(((item.out - item.seek) * 1000.0) as i64)
                .unwrap_or_default();
        }
    }

    Ok(web::Json(program))
}

/// ### System Statistics
///
/// Get statistics about CPU, Ram, Disk, etc. usage.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/system/1
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/system/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_system_stat(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers.lock().unwrap().get(*id).unwrap();
    let config = manager.config.lock().unwrap().clone();

    let stat = web::block(move || system::stat(&config)).await?;

    Ok(web::Json(stat))
}

pub mod ytbot {
    use super::*;
    use super::livestream::extract_rtmp_stream_details; // IMPORTANTE: para usar a função que extrai o rtmp_details

    static YTBOT_PROCESSES: Lazy<AsyncMutex<HashMap<i32, Arc<AsyncMutex<Child>>>>> = Lazy::new(|| AsyncMutex::new(HashMap::new()));

    #[derive(Error, Debug)]
    enum YtbotError {
        #[error("Erro ao verificar o status do ytbot: {0}")]
        StatusError(String),
    }

    async fn get_ytbot_path() -> Option<String> {
        if let Ok(path) = env::var("YTBOT_PATH") {
            if metadata(&path).await.is_ok() {
                return Some(path);
            }
        }

        let paths = ["/usr/local/bin/ytbot.sh", "/usr/local/bin/ytbot.py"];

        for path in &paths {
            if metadata(path).await.is_ok() {
                return Some(path.to_string());
            }
        }
        None
    }

    /// Verifica se o processo `ytbot` está ativo para um determinado canal.
    async fn is_ytbot_active(channel_id: i32) -> Result<bool, YtbotError> {
        let mut processes = YTBOT_PROCESSES.lock().await;

        // Removemos do mapa primeiro
        if let Some(ytbot_process) = processes.remove(&channel_id) {
            let mut ytbot_child = ytbot_process.lock().await;

            match ytbot_child.try_wait() {
                Ok(Some(_status)) => {
                    // O processo terminou, não reinserimos no mapa
                    Ok(false)
                }
                Ok(None) => {
                    // O processo ainda está ativo
                    // Precisamos reinserir o processo no mapa
                    drop(ytbot_child); // Solta o guard antes de reinserir

                    // Reinserir o mesmo processo no mapa
                    processes.insert(channel_id, ytbot_process);

                    Ok(true)
                }
                Err(e) => Err(YtbotError::StatusError(e.to_string())),
            }
        } else {
            Ok(false) // Nenhum processo registrado para esse canal
        }
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ServiceStatus {
        Active,
        Inactive,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ServiceStatusResponse {
        pub status: ServiceStatus,
    }

    #[get("/status/{id}")]
    #[protect(
        any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
        ty = "Role"
    )]
    pub async fn ytbot_service_status(
        id: web::Path<i32>,
        _role: AuthDetails<Role>,
        _user: web::ReqData<UserMeta>,
        controllers: web::Data<Mutex<ChannelController>>, // Adicionado como parâmetro
    ) -> impl Responder {
        let channel_id = *id;
        let channel_name = match get_channel_name(channel_id, controllers.clone()).await {
            Ok(name) => name,
            Err(_) => return HttpResponse::InternalServerError().json("Erro ao acessar o canal"),
        };

        match is_ytbot_active(channel_id).await {
            Ok(active) => {
                let status = if active {
                    ServiceStatus::Active
                } else {
                    ServiceStatus::Inactive
                };
                let response = ServiceStatusResponse { status };
                HttpResponse::Ok().json(response)
            }
            Err(e) => {
                error!(
                    "Erro ao verificar o status do ytbot para o canal {}: {}",
                    channel_name, e
                );
                HttpResponse::InternalServerError().json(format!(
                    "Erro ao verificar o status do ytbot para o canal {}",
                    channel_name
                ))
            }
        }
    }

    #[derive(Debug, Deserialize, Serialize, Clone)]
    #[serde(rename_all = "snake_case")]
    pub enum ServiceAction {
        Start,
        Stop,
    }

    #[derive(Debug, Deserialize, Serialize, Clone)]
    pub struct ServiceControlParams {
        pub action: ServiceAction,
    }

    #[post("/control/{id}")]
    #[protect(
        any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
        ty = "Role"
    )]
    pub async fn ytbot_control(
        id: web::Path<i32>,
        req: web::Json<ServiceControlParams>,
        controllers: web::Data<Mutex<ChannelController>>, // Adicionado como parâmetro
        _role: AuthDetails<Role>,
        _user: web::ReqData<UserMeta>,
    ) -> impl Responder {
        let action = req.action.clone();
        let channel_id = *id;
        let channel_name = match get_channel_name(channel_id, controllers.clone()).await {
            Ok(name) => name,
            Err(_) => return HttpResponse::InternalServerError().json("Erro ao acessar o canal"),
        };

        match action {
            ServiceAction::Start => {
                let mut processes = YTBOT_PROCESSES.lock().await;
                if processes.contains_key(&channel_id) {
                    info!("O ytbot já está em execução para o canal {}", channel_name);
                    return HttpResponse::BadRequest().json(format!(
                        "O ytbot já está em execução para o canal {}",
                        channel_name
                    ));
                }

                let ytbot_path = match get_ytbot_path().await {
                    Some(path) => path,
                    None => {
                        warn!("Nenhum executável do ytbot encontrado");
                        return HttpResponse::InternalServerError()
                            .json("Executável do ytbot não encontrado");
                    }
                };

                // Extraímos o rtmp_details via função Rust já existente
                let rtmp_details = match extract_rtmp_stream_details(channel_id, controllers.clone()).await {
                    Ok(details) => details,
                    Err(e) => {
                        error!("Erro ao extrair detalhes RTMP: {}", e);
                        return HttpResponse::InternalServerError().json(format!(
                            "Erro ao extrair detalhes RTMP para o canal {}",
                            channel_name
                        ));
                    }
                };

                // Montamos os argumentos para o ytbot com os parâmetros solicitados
                let args = vec![
                    format!("--monitor_channel={}", channel_id),
                    format!("--channel_name={}", channel_name),
                    format!("--rtmp_details={}", rtmp_details),
                ];

                let child = match Command::new(&ytbot_path)
                    .args(&args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                {
                    Ok(proc) => proc,
                    Err(e) => {
                        error!(
                            "Erro ao iniciar o ytbot para o canal {}: {}",
                            channel_name, e
                        );
                        return HttpResponse::InternalServerError().json(format!(
                            "Erro ao iniciar o ytbot para o canal {}",
                            channel_name
                        ));
                    }
                };

                let child = Arc::new(AsyncMutex::new(child));

                let stdout = {
                    let mut process_lock = child.lock().await;
                    match process_lock.stdout.take() {
                        Some(stdout) => stdout,
                        None => {
                            error!(
                                "Falha ao obter o stdout do ytbot para o canal {}",
                                channel_name
                            );
                            let _ = process_lock.kill().await;
                            return HttpResponse::InternalServerError().json(format!(
                                "Falha ao iniciar o ytbot para o canal {}",
                                channel_name
                            ));
                        }
                    }
                };

                let stderr = {
                    let mut process_lock = child.lock().await;
                    match process_lock.stderr.take() {
                        Some(stderr) => stderr,
                        None => {
                            error!(
                                "Falha ao obter o stderr do ytbot para o canal {}",
                                channel_name
                            );
                            let _ = process_lock.kill().await;
                            return HttpResponse::InternalServerError().json(format!(
                                "Falha ao iniciar o ytbot para o canal {}",
                                channel_name
                            ));
                        }
                    }
                };

                tokio::spawn(async move {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        debug!("ytbot stdout: {}", line);
                    }
                });

                tokio::spawn(async move {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        debug!("ytbot stderr: {}", line);
                    }
                });

                processes.insert(channel_id, child);
                info!(
                    "Processo do ytbot iniciado com sucesso para canal {}",
                    channel_name
                );
                HttpResponse::Ok().json(format!(
                    "ytbot iniciado com sucesso para o canal {}",
                    channel_name
                ))
            }
            ServiceAction::Stop => {
                let mut processes = YTBOT_PROCESSES.lock().await;
                if let Some(child) = processes.remove(&channel_id) {
                    async fn kill_and_wait_with_timeout(child: Arc<AsyncMutex<Child>>) -> Result<(), String> {
                        let mut child = child.lock().await;
                        child.kill().await.map_err(|e| e.to_string())?;
                        match timeout(Duration::from_secs(5), child.wait()).await {
                            Ok(Ok(_)) => Ok(()),
                            Ok(Err(e)) => Err(e.to_string()),
                            Err(_) => Err("Timeout ao encerrar o processo".to_string()),
                        }
                    }

                    match kill_and_wait_with_timeout(child).await {
                        Ok(()) => {
                            info!(
                                "Processo do ytbot interrompido com sucesso para canal {}",
                                channel_name
                            );
                            HttpResponse::Ok().json(format!(
                                "ytbot interrompido com sucesso para o canal {}",
                                channel_name
                            ))
                        }
                        Err(e) => {
                            error!(
                                "Erro ao interromper o ytbot para canal {}: {}",
                                channel_name, e
                            );
                            HttpResponse::InternalServerError().json(format!(
                                "Erro ao interromper o ytbot para o canal {}",
                                channel_name
                            ))
                        }
                    }
                } else {
                    info!(
                        "Nenhum processo do ytbot em execução para o canal {}",
                        channel_name
                    );
                    HttpResponse::BadRequest().json(format!(
                        "Nenhum processo do ytbot em execução para o canal {}",
                        channel_name
                    ))
                }
            }
        }
    }

    async fn get_channel_name(
        channel_id: i32,
        controllers: web::Data<Mutex<ChannelController>>
    ) -> Result<String, String> {
        let controller = match controllers.lock() {
            Ok(ctrl) => ctrl,
            Err(_) => return Err("Erro interno ao obter o controller".to_string()),
        };

        let manager = match controller.get(channel_id) {
            Some(mgr) => mgr,
            None => return Err(format!("Canal ({}) não existe!", channel_id)),
        };

        let channel_name = match manager.channel.lock() {
            Ok(ch) => ch.name.clone(),
            Err(_) => return Err("Erro ao acessar o canal".to_string()),
        };

        Ok(channel_name)
    }

    // Expondo as rotas para uso externo
    pub fn ytbot_routes() -> Scope {
        web::scope("/ytbot")
            .service(ytbot_service_status)
            .service(ytbot_control)
    }
}

// Módulo livestream
pub mod livestream {
    use super::*;

    #[derive(Error, Debug)]
    enum LivestreamError {
        #[error("Erro ao verificar o status do ffmpeg: {0}")]
        StatusError(String),
    }
    
    // Aqui definimos um mapa global de canal_id -> (streamlink_process, ffmpeg_process)
    static STREAM_PROCESSES: Lazy<AsyncMutex<HashMap<i32, (Arc<AsyncMutex<Child>>, Arc<AsyncMutex<Child>>)>>>
        = Lazy::new(|| AsyncMutex::new(HashMap::new()));
    
    async fn get_ffmpeg_path() -> Option<String> {
        if let Ok(path) = env::var("FFMPEG_PATH") {
            if metadata(&path).await.is_ok() {
                return Some(path);
            }
        }
    
        let paths = ["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"];
    
        for path in &paths {
            if metadata(path).await.is_ok() {
                return Some(path.to_string());
            }
        }
        None
    }
    
    /// Verifica se o processo `ffmpeg` do livestream está ativo para um determinado canal.
    async fn is_ffmpeg_livestream_active(channel_id: i32) -> Result<bool, LivestreamError> {
        let mut processes = STREAM_PROCESSES.lock().await;
    
        // Removemos do mapa primeiro
        if let Some((streamlink_process, ffmpeg_process)) = processes.remove(&channel_id) {
            let mut ffmpeg_child = ffmpeg_process.lock().await;
    
            match ffmpeg_child.try_wait() {
                Ok(Some(_status)) => {
                    // O processo terminou, não reinserimos no mapa
                    Ok(false)
                }
                Ok(None) => {
                    // O processo ainda está ativo
                    // Precisamos reinserir o par no mapa
                    drop(ffmpeg_child); // Solta o guard antes de reinserir
    
                    // Reinserir o mesmo tuple (streamlink_process, ffmpeg_process)
                    processes.insert(channel_id, (streamlink_process, ffmpeg_process));
    
                    Ok(true)
                }
                Err(e) => Err(LivestreamError::StatusError(e.to_string())),
            }
        } else {
            Ok(false) // Nenhum processo registrado para esse canal
        }
    }
    

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ServiceStatus {
        Active,
        Inactive,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ServiceStatusResponse {
        pub status: ServiceStatus,
    }

    #[get("/ffmpeg/status/{id}")]
    #[protect(
        any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
        ty = "Role"
    )]
    pub async fn livestream_ffmpeg_status(
        id: web::Path<i32>,
        _role: AuthDetails<Role>,
        _user: web::ReqData<UserMeta>,
        controllers: web::Data<Mutex<ChannelController>>, // Adicionado como parâmetro
    ) -> impl Responder {
        let channel_id = *id;
        let channel_name = match get_channel_name(channel_id, controllers.clone()).await {
            Ok(name) => name,
            Err(_) => return HttpResponse::InternalServerError().json("Erro ao acessar o canal"),
        };

        match is_ffmpeg_livestream_active(channel_id).await {
            Ok(active) => {
                let status = if active {
                    ServiceStatus::Active
                } else {
                    ServiceStatus::Inactive
                };
                let response = ServiceStatusResponse {
                    status,
                };
                HttpResponse::Ok().json(response)
            }
            Err(e) => {
                error!("Erro ao verificar o status do ffmpeg para o canal {}: {}", channel_name, e);
                HttpResponse::InternalServerError().json(format!("Erro ao verificar o status do ffmpeg para o canal {}", channel_name))
            }
        }
    }

    async fn get_streamlink_path() -> Option<String> {
        // Verifica se a variável de ambiente STREAMLINK_PATH está definida e se o caminho é válido
        if let Ok(path) = env::var("STREAMLINK_PATH") {
            if fs::metadata(&path).await.is_ok() {
                return Some(path);
            }
        }
    
        // Tenta encontrar o streamlink no diretório de instalação padrão do usuário
        if let Some(home_dir) = home_dir() {
            let default_path = home_dir.join("livebot/venv/bin/streamlink");
            if fs::metadata(&default_path).await.is_ok() {
                return Some(default_path.to_string_lossy().to_string());
            }
        }
    
        None
    }
    
    pub async fn extract_rtmp_stream_details(
        id: i32,
        controllers: web::Data<Mutex<ChannelController>>
    ) -> Result<String, ServiceError> {
        let controller = controllers.lock().map_err(|_| ServiceError::InternalServerError)?;
    
        let manager = controller
            .get(id)
            .ok_or(ServiceError::BadRequest(format!("Canal ({id}) não existe!")))?;
    
        let config = manager.config.lock().map_err(|_| ServiceError::InternalServerError)?;
        let input_param = &config.ingest.input_param;
    
        let re = Regex::new(r":(\d{1,5})(\S*)").map_err(|_| ServiceError::InternalServerError)?;
    
        if let Some(caps) = re.captures(input_param) {
            if let Some(port_str) = caps.get(1) {
                let port_str = port_str.as_str();
                if let Ok(port) = port_str.parse::<u16>() {
                    let stream_key = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                    return Ok(format!(":{}{}", port, stream_key));
                }
            }
        }
    
        Err(ServiceError::BadRequest("Nenhuma porta válida encontrada".to_string()))
    }
    
    #[derive(Debug, Deserialize, Serialize, Clone)]
    #[serde(rename_all = "snake_case")]
    pub enum StreamAction {
        Start,
        Stop,
    }

    #[derive(Debug, Deserialize)]
    pub struct StreamParams {
        pub action: StreamAction,
        pub url: Option<String>,
    }
    
    #[post("/control/{id}")]
    #[protect(
        any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
        ty = "Role"
    )]
    pub async fn livestream_control(
        id: web::Path<i32>,
        req: web::Json<StreamParams>,
        controllers: web::Data<Mutex<ChannelController>>, // Adicionado como parâmetro
        _role: AuthDetails<Role>,
        _user: web::ReqData<UserMeta>,
    ) -> impl Responder {
        let action = req.action.clone();
        let channel_id = *id;
        let channel_name = match get_channel_name(channel_id, controllers.clone()).await {
            Ok(name) => name,
            Err(_) => return HttpResponse::InternalServerError().json("Erro ao acessar o canal"),
        };
    
        match action {
            StreamAction::Start => {
                let mut processes = STREAM_PROCESSES.lock().await;
                if processes.contains_key(&channel_id) {
                    info!("Stream já está em execução para o canal {}", channel_name);
                    return HttpResponse::BadRequest().json(format!("Stream já está em execução para o canal {}", channel_name));
                }
    
                let url = match &req.url {
                    Some(u) => u,
                    None => {
                        info!("URL não fornecida");
                        return HttpResponse::BadRequest().json("URL não fornecida");
                    }
                };
    
                if let Ok(parsed_url) = Url::parse(url) {
                    // Verifica o caminho do executável do streamlink
                    let streamlink_path = match get_streamlink_path().await {
                        Some(path) => path,
                        None => {
                            error!("Executável do streamlink não encontrado");
                            return HttpResponse::InternalServerError()
                                .json("Executável do streamlink não encontrado");
                        }
                    };
    
                    let ffmpeg_path = match get_ffmpeg_path().await {
                        Some(path) => path,
                        None => {
                            error!("Executável do ffmpeg não encontrado");
                            return HttpResponse::InternalServerError()
                                .json("Executável do ffmpeg não encontrado");
                        }
                    };
    
                    // Define os argumentos do streamlink
                    let streamlink_args = vec![
                        "--hls-live-edge",
                        "6",
                        "--ringbuffer-size",
                        "128M",
                        "-4",
                        "--stream-sorting-excludes",
                        ">720p",
                        "--default-stream",
                        "best",
                        "--url",
                        parsed_url.as_str(),
                        "-o",
                        "-",
                    ];
    
                    // Inicia o processo do streamlink
                    let streamlink_process = match Command::new(&streamlink_path)
                        .args(&streamlink_args)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .stdin(Stdio::null())
                        .spawn()
                    {
                        Ok(process) => process,
                        Err(e) => {
                            error!("Erro ao iniciar o streamlink: {}", e);
                            return HttpResponse::InternalServerError()
                                .json("Erro ao iniciar o streaming");
                        }
                    };
    
                    let streamlink_process = Arc::new(AsyncMutex::new(streamlink_process));
    
                    let mut streamlink_stdout = {
                        let mut process_lock = streamlink_process.lock().await;
                        match process_lock.stdout.take() {
                            Some(stdout) => stdout,
                            None => {
                                error!("Falha ao obter o stdout do streamlink");
                                let _ = process_lock.kill().await;
                                return HttpResponse::InternalServerError()
                                    .json("Erro ao iniciar o streaming");
                            }
                        }
                    };
    
                    let streamlink_stderr = {
                        let mut process_lock = streamlink_process.lock().await;
                        match process_lock.stderr.take() {
                            Some(stderr) => stderr,
                            None => {
                                error!("Falha ao obter o stderr do streamlink");
                                let _ = process_lock.kill().await;
                                return HttpResponse::InternalServerError()
                                    .json("Erro ao iniciar o streaming");
                            }
                        }
                    };
    
                    let rtmp_details = match extract_rtmp_stream_details(channel_id, controllers.clone()).await {
                        Ok(details) => details,
                        Err(e) => {
                            error!("Erro ao extrair detalhes RTMP: {}", e);
                            let mut process_lock = streamlink_process.lock().await;
                            let _ = process_lock.kill().await;
                            return HttpResponse::InternalServerError().json("Erro ao extrair detalhes RTMP");
                        }
                    };
    
                    let ffmpeg_url = format!("rtmp://127.0.0.1{}", rtmp_details);
    
                    let ffmpeg_args = [
                        "-re",
                        "-hide_banner",
                        "-nostats",
                        "-v",
                        "level+error",
                        "-i",
                        "pipe:0",
                        "-vcodec",
                        "copy",
                        "-acodec",
                        "copy",
                        "-f",
                        "flv",
                        &ffmpeg_url,
                    ];
    
                    let ffmpeg_process = match Command::new(&ffmpeg_path)
                        .args(&ffmpeg_args)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                    {
                        Ok(process) => process,
                        Err(e) => {
                            error!("Erro ao iniciar o ffmpeg: {}", e);
                            let mut process_lock = streamlink_process.lock().await;
                            let _ = process_lock.kill().await;
                            return HttpResponse::InternalServerError()
                                .json("Erro ao iniciar o streaming");
                        }
                    };
    
                    let ffmpeg_process = Arc::new(AsyncMutex::new(ffmpeg_process));
    
                    let mut ffmpeg_stdin = {
                        let mut process_lock = ffmpeg_process.lock().await;
                        match process_lock.stdin.take() {
                            Some(stdin) => stdin,
                            None => {
                                error!("Falha ao obter o stdin do ffmpeg");
                                let mut streamlink_process_lock = streamlink_process.lock().await;
                                let _ = streamlink_process_lock.kill().await;
                                let _ = process_lock.kill().await;
                                return HttpResponse::InternalServerError()
                                    .json("Erro ao iniciar o streaming");
                            }
                        }
                    };
    
                    let ffmpeg_stdout = {
                        let mut process_lock = ffmpeg_process.lock().await;
                        match process_lock.stdout.take() {
                            Some(stdout) => stdout,
                            None => {
                                error!("Falha ao obter o stdout do ffmpeg");
                                let mut streamlink_process_lock = streamlink_process.lock().await;
                                let _ = streamlink_process_lock.kill().await;
                                let _ = process_lock.kill().await;
                                return HttpResponse::InternalServerError()
                                    .json("Erro ao iniciar o streaming");
                            }
                        }
                    };
    
                    let ffmpeg_stderr = {
                        let mut process_lock = ffmpeg_process.lock().await;
                        match process_lock.stderr.take() {
                            Some(stderr) => stderr,
                            None => {
                                error!("Falha ao obter o stderr do ffmpeg");
                                let mut streamlink_process_lock = streamlink_process.lock().await;
                                let _ = streamlink_process_lock.kill().await;
                                let _ = process_lock.kill().await;
                                return HttpResponse::InternalServerError()
                                    .json("Erro ao iniciar o streaming");
                            }
                        }
                    };
    
                    let streamlink_process_clone = Arc::clone(&streamlink_process);
                    let ffmpeg_process_clone = Arc::clone(&ffmpeg_process);
    
                    let copy_task = tokio::spawn(async move {
                        if let Err(e) = tokio::io::copy(&mut streamlink_stdout, &mut ffmpeg_stdin).await {
                            error!("Erro ao copiar dados do streamlink para o ffmpeg: {}", e);
                            HttpResponse::InternalServerError().json("Erro ao copiar dados do streamlink para o ffmpeg");
                            let mut streamlink_process = streamlink_process_clone.lock().await;
                            let mut ffmpeg_process = ffmpeg_process_clone.lock().await;
                            let _ = streamlink_process.kill().await;
                            let _ = ffmpeg_process.kill().await;
                        }
                    });
    
                    tokio::spawn(async move {
                        if let Err(e) = copy_task.await {
                            error!("Erro na tarefa de cópia: {}", e);
                        }
                    });
    
                    tokio::spawn(async move {
                        let reader = BufReader::new(streamlink_stderr);
                        let mut lines = reader.lines();
    
                        while let Ok(Some(line)) = lines.next_line().await {
                            debug!("streamlink: {}", line);
                        }
                    });
    
                    tokio::spawn(async move {
                        let reader = BufReader::new(ffmpeg_stdout);
                        let mut lines = reader.lines();
    
                        while let Ok(Some(line)) = lines.next_line().await {
                            debug!("ffmpeg stdout: {}", line);
                        }
                    });
    
                    tokio::spawn(async move {
                        let reader = BufReader::new(ffmpeg_stderr);
                        let mut lines = reader.lines();
    
                        while let Ok(Some(line)) = lines.next_line().await {
                            debug!("ffmpeg stderr: {}", line);
                        }
                    });
    
                    // Armazena ambos os processos no mapa
                    processes.insert(channel_id, (streamlink_process, ffmpeg_process));
                    drop(processes);
    
                    info!("Stream iniciado para canal {}", channel_name);
                    HttpResponse::Ok().json(format!("Stream iniciado para canal {}", channel_name))
                } else {
                    info!("URL inválida");
                    HttpResponse::BadRequest().json("URL inválida")
                }
            }
            StreamAction::Stop => {
                let mut processes = STREAM_PROCESSES.lock().await;
                if let Some((streamlink_child, ffmpeg_child)) = processes.remove(&channel_id) {
                    async fn kill_and_wait_with_timeout(child: Arc<AsyncMutex<Child>>) -> Result<(), String> {
                        let mut child = child.lock().await;
                        child.kill().await.map_err(|e| e.to_string())?;
                        match timeout(Duration::from_secs(5), child.wait()).await {
                            Ok(Ok(_)) => Ok(()),
                            Ok(Err(e)) => Err(e.to_string()),
                            Err(_) => Err("Timeout ao encerrar o processo".to_string()),
                        }
                    }
    
                    let streamlink_result = kill_and_wait_with_timeout(streamlink_child).await;
                    let ffmpeg_result = kill_and_wait_with_timeout(ffmpeg_child).await;
    
                    match (streamlink_result, ffmpeg_result) {
                        (Ok(()), Ok(())) => {
                            info!("Stream Encerrado para o canal {}", channel_name);
                            HttpResponse::Ok().json(format!("Stream Encerrado para o canal {}", channel_name))
                        }
                        (Err(e1), Err(e2)) => {
                            error!(
                                "Erro ao parar streaming do canal {}: streamlink: {}, ffmpeg: {}",
                                channel_name, e1, e2
                            );
                            HttpResponse::InternalServerError().json(format!("Erro ao parar streaming do canal {}",
                                channel_name))
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            error!("Erro ao parar um dos processos do streaming do canal {}: {}", channel_name, e);
                            HttpResponse::InternalServerError().json(format!("Erro ao parar um dos processos do streaming do canal {}", channel_name))
                        }
                    }
                } else {
                    info!("Nenhum stream está em execução para o canal {}", channel_name);
                    HttpResponse::BadRequest().json(format!("Nenhum stream está em execução para o canal {}", channel_name))
                }
            }
        }
    }

    async fn get_channel_name(channel_id: i32, controllers: web::Data<Mutex<ChannelController>>) -> Result<String, String> {
        let controller = match controllers.lock() {
            Ok(ctrl) => ctrl,
            Err(_) => return Err("Erro interno ao obter o controller".to_string()),
        };

        let manager = match controller.get(channel_id) {
            Some(mgr) => mgr,
            None => return Err(format!("Canal ({}) não existe!", channel_id)),
        };

        let channel_name = match manager.channel.lock() {
            Ok(ch) => ch.name.clone(),
            Err(_) => return Err("Erro ao acessar o canal".to_string()),
        };

        Ok(channel_name)
    }

    // Expondo as rotas para uso externo
    pub fn livestream_routes() -> Scope {
        web::scope("/livestream")
            .service(livestream_ffmpeg_status)
            .service(livestream_control)
    }
}

// Reexporte as funções para facilitar o acesso
pub use livestream::livestream_routes;
pub use ytbot::ytbot_routes;
