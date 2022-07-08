use std::{path::Path, process::exit};

use actix_web::{dev::ServiceRequest, middleware, web, App, Error, HttpMessage, HttpServer};
use actix_web_grants::permissions::AttachPermissions;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use actix_web_httpauth::middleware::HttpAuthentication;

use clap::Parser;
use simplelog::*;

pub mod utils;

use utils::{
    args_parse::Args,
    auth, db_path, init_config,
    models::LoginUser,
    routes::{
        add_dir, add_preset, add_user, control_playout, del_playlist, delete_preset, file_browser,
        gen_playlist, get_all_settings, get_log, get_playlist, get_playout_config, get_presets,
        get_settings, get_user, login, media_current, media_last, media_next, move_rename,
        patch_settings, process_control, remove, save_file, save_playlist, send_text_message,
        update_playout_config, update_preset, update_user,
    },
    run_args, Role,
};

use ffplayout_lib::utils::{init_logging, PlayoutConfig};

async fn validator(req: ServiceRequest, credentials: BearerAuth) -> Result<ServiceRequest, Error> {
    // We just get permissions from JWT
    let claims = auth::decode_jwt(credentials.token()).await?;
    req.attach(vec![Role::set_role(&claims.role)]);

    req.extensions_mut()
        .insert(LoginUser::new(claims.id, claims.username));

    Ok(req)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let mut config = PlayoutConfig::new(None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    if let Err(c) = run_args(args.clone()).await {
        exit(c);
    }

    if let Some(conn) = args.listen {
        if let Ok(p) = db_path() {
            if !Path::new(&p).is_file() {
                error!("Database is not initialized! Init DB first and add admin user.");
                exit(1);
            }
        }
        init_config().await;
        let ip_port = conn.split(':').collect::<Vec<&str>>();
        let addr = ip_port[0];
        let port = ip_port[1].parse::<u16>().unwrap();

        info!("running ffplayout API, listen on {conn}");

        // TODO: add allow origin (or give it to the proxy)
        HttpServer::new(move || {
            let auth = HttpAuthentication::bearer(validator);
            App::new()
                .wrap(middleware::Logger::default())
                .service(login)
                .service(
                    web::scope("/api")
                        .wrap(auth)
                        .service(add_user)
                        .service(get_user)
                        .service(get_playout_config)
                        .service(update_playout_config)
                        .service(add_preset)
                        .service(get_presets)
                        .service(update_preset)
                        .service(delete_preset)
                        .service(get_settings)
                        .service(get_all_settings)
                        .service(patch_settings)
                        .service(update_user)
                        .service(send_text_message)
                        .service(control_playout)
                        .service(media_current)
                        .service(media_next)
                        .service(media_last)
                        .service(process_control)
                        .service(get_playlist)
                        .service(save_playlist)
                        .service(gen_playlist)
                        .service(del_playlist)
                        .service(get_log)
                        .service(file_browser)
                        .service(add_dir)
                        .service(move_rename)
                        .service(remove)
                        .service(save_file),
                )
        })
        .bind((addr, port))?
        .run()
        .await
    } else {
        error!("Run ffpapi with listen parameter!");

        Ok(())
    }
}