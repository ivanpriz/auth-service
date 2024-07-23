mod adapters;
mod dtos;
mod services;

use std::{
    env,
    sync::Arc,
};

use adapters::postgres::repositories::UnitOfWorkFactory;
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::Duration;
use diesel_async::pooled_connection::{deadpool::Pool, AsyncDieselConnectionManager};
use dotenvy::dotenv;
use dtos::users::{SignInData, UserCreateInDTO, UserDBDTO, UserOutDTO};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use services::users::UsersService;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DB URL must be set");
    let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(database_url);
    let pool = Pool::builder(config).build().unwrap();
    let uow_factory = UnitOfWorkFactory::new(pool);

    let users_service = UsersService::new(uow_factory);

    let app = Router::new()
        .route("/register/", post(create_user))
        .route("/login/", post(sign_in))
        .with_state(Arc::new(RwLock::new(users_service)));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_user(
    State(users_service): State<Arc<RwLock<UsersService>>>,
    Json(user_create_in): Json<UserCreateInDTO>,
) -> Json<UserOutDTO> {
    let mut users_service_ = users_service.write().await;
    let created_user = users_service_.create_user(&user_create_in).await;
    println!("Created user {} with id {}", created_user.username, created_user.id);
    Json(UserOutDTO {
        username: created_user.username,
        id: created_user.id,
        interests: created_user.interests,
    })
}

async fn sign_in(
    State(users_service): State<Arc<RwLock<UsersService>>>,
    Json(user_data): Json<SignInData>,
) -> Result<Json<String>, StatusCode> {
    match users_service
        .write()
        .await
        .authenticate_user(user_data.username, &user_data.password)
        .await
    {
        Some(user_db) => {
            let token =
                encode_jwt(&user_db.username).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                println!("User {} (id {}) logged in", user_db.username, user_db.id);
            Ok(Json(token))
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

#[derive(serde::Serialize)]
pub struct Claims {
    pub exp: usize,       // Expiry time of the token
    pub iat: usize,       // Issued at time of the token
    pub username: String, // Email associated with the token
}

pub fn encode_jwt(username: &str) -> Result<String, StatusCode> {
    let secret: String = "random".to_string();
    let now = chrono::Utc::now();
    let expire: chrono::TimeDelta = Duration::hours(24);
    let exp: usize = (now + expire).timestamp() as usize;
    let iat: usize = now.timestamp() as usize;
    let claim = Claims { iat, exp, username: username.to_string() };

    encode(
        &Header::default(),
        &claim,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
