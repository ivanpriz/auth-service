mod adapters;
mod dtos;
mod services;

use std::{env, sync::Arc};

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
    let app = create_app(users_service);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn create_app(users_service: UsersService) -> Router {
    Router::new()
        .route("/register", post(create_user))
        .route("/login", post(sign_in))
        .with_state(Arc::new(RwLock::new(users_service)))
}

async fn create_user(
    State(users_service): State<Arc<RwLock<UsersService>>>,
    Json(user_create_in): Json<UserCreateInDTO>,
) -> Json<UserOutDTO> {
    let mut users_service_ = users_service.write().await;
    let created_user = users_service_.create_user(&user_create_in).await;
    println!(
        "Created user {} with id {}",
        created_user.username, created_user.id
    );
    Json(UserOutDTO {
        username: created_user.username,
        id: created_user.id,
        email: created_user.email,
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
    let claim = Claims {
        iat,
        exp,
        username: username.to_string(),
    };

    encode(
        &Header::default(),
        &claim,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod tests {
    use crate::adapters::postgres::models::UserModel;

    use super::*;
    use crate::{
        create_app,
        dtos::users::{UserCreateDTO, UserCreateInDTO, UserOutDTO},
    };
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use chrono::NaiveDate;
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;
    use diesel_async::{
        pooled_connection::{deadpool::Pool, AsyncDieselConnectionManager},
        AsyncConnection, AsyncPgConnection,
    };
    use dotenvy::dotenv;
    use http_body_util::BodyExt;
    use pwhash::bcrypt;
    use rstest::{fixture, rstest};
    use serde::Serialize;
    use serde_json::{json, Serializer, Value};
    use serial_test::serial;
    use std::{collections::HashMap, env, process::Command};
    use tokio::runtime::{Builder, Runtime};
    use tower::{Service, ServiceExt};

    // As we need a way for fixtures to clean up stuff after a test has run,
    // we will use this structure to store the return value, and then run some code on drop.
    // Also found a way to test async code without using async tests, and with the ability to do
    // cleanup on drop - we can create runtime fixture and drill it through all the fixtures.
    // With such approach all the fixts and tests remains sync, and we can call async cleanups in
    // drop.
    struct WithCleanup<ValT> {
        pub closure: Box<dyn FnMut() -> ()>,
        pub val: ValT,
    }

    impl<ValT> Drop for WithCleanup<ValT> {
        fn drop(&mut self) {
            (*self.closure)();
        }
    }

    #[fixture]
    fn runtime() -> Runtime {
        Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[fixture]
    fn migrations() -> WithCleanup<()> {
        Command::new("diesel")
            .arg("migration")
            .arg("run")
            .arg("--locked-schema")
            .output()
            .expect("Error setting up diesel");

        WithCleanup {
            val: (),
            closure: Box::new(|| {
                Command::new("diesel")
                    .arg("migration")
                    .arg("revert")
                    .arg("--locked-schema")
                    .arg("--all")
                    .output()
                    .expect("Error reverting migrations");
            }),
        }
    }

    #[fixture]
    fn connection(runtime: Runtime) -> (AsyncPgConnection, Runtime) {
        dotenv().ok();

        let database_url = env::var("DATABASE_URL").expect("DB URL must be set");

        let connection = runtime
            .block_on(AsyncPgConnection::establish(&database_url))
            .unwrap_or_else(|_| panic!("Error connecting to {}", database_url));

        (connection, runtime)
    }

    #[fixture]
    fn conn_pool(runtime: Runtime) -> (Pool<AsyncPgConnection>, Runtime) {
        dotenv().ok();

        let database_url = env::var("DATABASE_URL").expect("DB URL must be set");
        let config =
            AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(database_url);
        let pool = Pool::builder(config).build().unwrap();

        println!("Pool connected to db");
        (pool, runtime)
    }

    #[fixture]
    fn uow_factory(conn_pool: (Pool<AsyncPgConnection>, Runtime)) -> (UnitOfWorkFactory, Runtime) {
        (UnitOfWorkFactory::new(conn_pool.0), conn_pool.1)
    }

    #[fixture]
    fn users_service(uow_factory: (UnitOfWorkFactory, Runtime)) -> (UsersService, Runtime) {
        (UsersService::new(uow_factory.0), uow_factory.1)
    }

    #[fixture]
    fn axum_app(users_service: (UsersService, Runtime)) -> (Router, Runtime) {
        (create_app(users_service.0), users_service.1)
    }

    #[fixture]
    fn default_users() -> HashMap<i32, UserDBDTO> {
        HashMap::from([(
            1,
            UserDBDTO {
                id: 1,
                username: "John".to_string(),
                hashed_pwd: bcrypt::hash("hashed_pwd##").unwrap(),
                registration_date: chrono::Utc::now().naive_utc(),
                email: "john@mail.com".to_string(),
            },
        )])
    }

    #[fixture]
    fn default_users_create() -> Vec<UserCreateDTO> {
        vec![UserCreateDTO {
            username: "John".to_string(),
            hashed_pwd: bcrypt::hash("hashed_pwd##").unwrap(),
            registration_date: chrono::Utc::now().naive_utc(),
            email: "john@mail.com".to_string(),
        }]
    }

    #[fixture]
    fn default_users_create_in() -> Vec<UserCreateInDTO> {
        vec![UserCreateInDTO {
            username: "John".to_string(),
            password: "hashed_pwd##".to_string(),
            email: "john@mail.com".to_string(),
        }]
    }

    #[fixture]
    fn existing_users(
        // todo: prob it's good to receive uow in transaction here
        // migrations: WithCleanup<()>,
        connection: (AsyncPgConnection, Runtime),
        default_users: HashMap<i32, UserDBDTO>,
    ) -> WithCleanup<HashMap<i32, UserDBDTO>> {
        use crate::adapters::postgres::schema::users;

        let (mut conn, runtime) = connection;
        // Here we are using connection to insert users
        let _ = runtime
            .block_on(
                diesel::insert_into(users::table)
                    .values(
                        default_users
                            .iter()
                            .map(|(_id, user)| UserModel::from_dto(user))
                            .collect::<Vec<UserModel>>(),
                    )
                    .returning(UserModel::as_returning())
                    .get_results(&mut conn),
            )
            .and_then(|users| {
                println!("Successfully created test users");
                Ok(users)
            })
            .unwrap_or_else(|error| {
                println!("Error creating default users {:?}", error);
                vec![]
            });

        let ids_to_delete = default_users.clone().into_keys().collect::<Vec<i32>>();

        WithCleanup {
            val: default_users,
            closure: Box::new(move || {
                use crate::adapters::postgres::schema::users::dsl::*;
                runtime
                    .block_on(
                        diesel::delete(users.filter(id.eq_any(ids_to_delete.clone())))
                            .execute(&mut conn),
                    )
                    .expect("Error deleting users");
                println!("Successfully cleaned up default users!");
            }),
        }
    }

    #[rstest]
    #[serial(existing_user)]
    #[serial(axum_app)]
    fn test_create_user_one_should_succeed(
        _migrations: WithCleanup<()>,
        axum_app: (Router, Runtime),
        // default_users_create: Vec<UserCreateDTO>,
    ) {
        let (mut app, runtime) = axum_app;
        let req_data = UserCreateInDTO {
            username: String::from("nagibator"),
            password: String::from("qwerty123"),
            email: String::from("vasya2003@mail.ru"),
        };
        let resp = runtime
            .block_on(
                app.oneshot(
                    Request::builder()
                        .method(http::Method::POST)
                        .header(http::header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
                        .uri("/register")
                        .body(Body::from(serde_json::to_string(&req_data).unwrap()))
                        .unwrap(),
                ),
            )
            .unwrap();
        println!("Status: {:?}", resp.status());
        let body = runtime
            .block_on(resp.into_body().collect())
            .unwrap()
            .to_bytes();
        let body: Value = serde_json::from_slice(&body).unwrap();
        println!("Body: {:?}", body);
        // assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            body,
            json!({
               "id": 1,
               "username": "nagibator",
               "email": "vasya2003@mail.ru"

            })
        );
    }
}
