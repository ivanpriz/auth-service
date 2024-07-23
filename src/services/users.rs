use chrono::{NaiveDate, Utc};
use pwhash::bcrypt;

use crate::adapters::postgres::{
    repositories::{Repository, UnitOfWorkFactory, UsersRepo},
    specifications::{CompType, UsersSpecification},
};
use crate::dtos::users::{UserCreateDTO, UserCreateInDTO, UserDBDTO};

pub struct UsersService {
    uow_factory: UnitOfWorkFactory,
}

impl UsersService {
    pub fn new(uow_factory: UnitOfWorkFactory) -> Self {
        Self { uow_factory }
    }

    pub async fn create_user(&mut self, user: &UserCreateInDTO) -> UserDBDTO {
        let hashed_pwd = bcrypt::hash(user.password.clone()).unwrap();
        let registration_date = Utc::now().naive_utc();
        let user_create_db_dto = UserCreateDTO {
            username: user.username.clone(),
            hashed_pwd,
            registration_date,
            interests: user.interests.clone(),
        };
        let mut uow = self.uow_factory.create_uow().await;
        UsersRepo::create_from_dto(&user_create_db_dto, &mut uow).await
    }

    pub async fn find_by_username(&mut self, username: String) -> Option<UserDBDTO> {
        let mut uow = self.uow_factory.create_uow().await;
        UsersRepo::get_one_by(
            UsersSpecification::Username(CompType::Equals(username)),
            &mut uow,
        )
        .await
    }

    pub async fn authenticate_user(
        &mut self,
        username: String,
        password: &str,
    ) -> Option<UserDBDTO> {
        let mut uow = self.uow_factory.create_uow().await;
        let user = UsersRepo::get_one_by(
            UsersSpecification::Username(CompType::Equals(username)),
            &mut uow,
        )
        .await;

        match user {
            Some(user_db) => {
                if bcrypt::verify(password, user_db.hashed_pwd.as_str()) {
                    Some(user_db)
                } else {
                    None
                }
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::adapters::postgres::models::UserModel;

    use super::*;
    use chrono::NaiveDate;
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;
    use diesel_async::{
        pooled_connection::{deadpool::Pool, AsyncDieselConnectionManager},
        AsyncConnection, AsyncPgConnection,
    };
    use dotenvy::dotenv;
    use rstest::{fixture, rstest};
    use serial_test::serial;
    use std::{collections::HashMap, env, process::Command};
    use tokio::runtime::{Builder, Runtime};

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
    fn default_users() -> HashMap<i32, UserDBDTO> {
        HashMap::from([(
            1,
            UserDBDTO {
                id: 1,
                username: "John".to_string(),
                hashed_pwd: bcrypt::hash("hashed_pwd##").unwrap(),
                registration_date: chrono::Utc::now().naive_utc(),
                interests: "Programming, gaming".to_string(),
            },
        )])
    }

    #[fixture]
    fn default_users_create() -> Vec<UserCreateDTO> {
        vec![UserCreateDTO {
            username: "John".to_string(),
            hashed_pwd: bcrypt::hash("hashed_pwd##").unwrap(),
            registration_date: chrono::Utc::now().naive_utc(),
            interests: "Programming, gaming".to_string(),
        }]
    }

    #[fixture]
    fn default_users_create_in() -> Vec<UserCreateInDTO> {
        vec![UserCreateInDTO {
            username: "John".to_string(),
            password: "hashed_pwd##".to_string(),
            interests: "Programming, gaming".to_string(),
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
    fn test_create_user_one_should_succeed(
        _migrations: WithCleanup<()>,
        users_service: (UsersService, Runtime),
        default_users_create: Vec<UserCreateDTO>,
        default_users_create_in: Vec<UserCreateInDTO>,
    ) {
        let (mut users_service, runtime) = users_service;
        let created_user = runtime.block_on(users_service.create_user(&default_users_create_in[0]));
        assert_eq!(created_user.id, 1);
        // created_user.hashed_pwd == default_users_create[0].hashed_pwd because of salt we can't compare
        assert!(
            created_user.username == default_users_create[0].username
                && created_user.registration_date.date() == default_users_create[0].registration_date.date() // can only compare date here
                && created_user.interests == default_users_create[0].interests
        )
    }

    #[rstest]
    #[serial(existing_user)]
    fn test_find_by_username_should_succeed(
        _migrations: WithCleanup<()>,
        users_service: (UsersService, Runtime),
        existing_users: WithCleanup<HashMap<i32, UserDBDTO>>,
    ) {
        let (mut users_service, runtime) = users_service;
        for (_, user_db_dto) in existing_users.val.iter() {
            let user_found = runtime
                .block_on(users_service.find_by_username(user_db_dto.username.clone()))
                .unwrap();
            assert!(
                user_found.username == user_db_dto.username
                    && user_found.hashed_pwd == user_db_dto.hashed_pwd
                    && user_db_dto.registration_date.date() == user_db_dto.registration_date.date()
                    && user_db_dto.interests == user_db_dto.interests
            );
        }
    }

    #[rstest]
    #[serial(existing_user)]
    fn test_authenticate_should_succeed(
        _migrations: WithCleanup<()>,
        users_service: (UsersService, Runtime),
        existing_users: WithCleanup<HashMap<i32, UserDBDTO>>,
    ) {
        let (mut users_service, runtime) = users_service;
        for (_, user_db_dto) in existing_users.val.iter() {
            let user_found = runtime
                .block_on(
                    users_service.authenticate_user(user_db_dto.username.clone(), "hashed_pwd##"),
                )
                .unwrap();
            assert!(
                user_found.username == user_db_dto.username
                    && user_found.hashed_pwd == user_db_dto.hashed_pwd
                    && user_db_dto.registration_date.date() == user_db_dto.registration_date.date()
                    && user_db_dto.interests == user_db_dto.interests
            );
        }
    }
}
