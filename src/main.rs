mod completion;
mod feishu;
use std::{
    env,
    str::FromStr,
    sync::{Arc, RwLock},
};

use axum::{
    extract,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Router, Server,
};
use dotenvy::dotenv;
use feishu::{
    auth::{get_access_token_periodically},
    events::{common::BotEvent, EventType},
};
use tokio::{task};

use crate::{
    completion::completion,
    feishu::{events::im_message::IMMessageReceiveEvent, message::reply_message},
};

pub struct BotState {
    pub openai_key: String,
    pub access_token: String,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    env::set_var("RUST_LOG", "info");
    tracing_subscriber::fmt().pretty().init();
    let host = env::var("HOST").unwrap_or("127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or("8000".to_string());
    let app_id = env::var("APP_ID").unwrap();
    let app_secret = env::var("APP_SECRET").unwrap();
    let openai_key = env::var("OPENAI_KEY").unwrap();
    let bot_state = Arc::new(RwLock::new(BotState {
        openai_key,
        access_token: "".to_string(), // 之后在定时任务中更新
    }));
    // 周期性地获取 access token
    let access_token_task = task::spawn(get_access_token_periodically(
        app_id.clone(),
        app_secret.clone(),
        bot_state.clone(),
    ));
    tokio::spawn(access_token_task);
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/bot", post(bot))
        .layer(Extension(bot_state));

    Server::bind(&format!("{}:{}", host, port).parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn bot(
    Extension(state): Extension<Arc<RwLock<BotState>>>,
    extract::Json(bot_event): extract::Json<BotEvent>,
) -> impl IntoResponse {
    let et = bot_event.header.event_type;
    if let Ok(event_type) = EventType::from_str(et.as_str()) {
        match event_type {
            EventType::IMMessageReceive => {
                let e: IMMessageReceiveEvent = serde_json::from_value(bot_event.event).unwrap();
                tracing::debug!("Chat message: {:?}", e.message.message_id);
                let bot_state = state.read().unwrap();
                let openai_key = bot_state.openai_key.clone();
                let access_token = bot_state.access_token.clone();
                let c_task = task::spawn(completion_chat(
                    e.message.message_id,
                    e.message.content.clone(),
                    openai_key,
                    access_token,
                ));
                tokio::spawn(c_task);
            }
        }
    } else {
        tracing::warn!("Unknown event type: {}", et);
    }
    (StatusCode::OK, "OK")
}

async fn completion_chat(
    message_id: String,
    message_content: String,
    openai_key: String,
    access_token: String,
) {
    let completion_result = completion(message_content.clone(), openai_key).await;
    tracing::info!("start completion task for {}", message_id);
    reply_message(message_id, completion_result, access_token).await;
}

// #[derive(Debug, Deserialize)]
// struct ChallengeRequest {
// challenge: String,
// token: String,
// #[serde(rename = "type")]
// type_: String,
// }

// #[derive(Debug, Serialize)]
// struct ChallengeResponse {
//     challenge: String,
// }
