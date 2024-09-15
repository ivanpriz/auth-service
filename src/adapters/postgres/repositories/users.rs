use diesel::prelude::*;
use diesel_async::RunQueryDsl;

use super::super::specifications::{CompType, UsersSpecification};
use super::repo_trait::Repository;
use super::unit_of_work::UnitOfWork;
use super::UnitOfWorkInternal;
use crate::adapters::postgres::models::{NewUserModel, UserModel};
use crate::dtos::users::{UserCreateDTO, UserDBDTO};

pub struct UsersRepo {}

impl Repository<UserCreateDTO, UserDBDTO, UsersSpecification> for UsersRepo {
    async fn create_from_dto(user_create_data: &UserCreateDTO, uow: &mut UnitOfWork) -> UserDBDTO {
        use crate::adapters::postgres::schema::users;

        let new_post = NewUserModel {
            username: &user_create_data.username,
            hashed_pwd: &user_create_data.hashed_pwd,
            registration_date: &user_create_data.registration_date,
            email: &user_create_data.email,
        };

        let user = diesel::insert_into(users::table)
            .values(&new_post)
            .returning(UserModel::as_returning())
            .get_result(uow.get_conn())
            .await
            .expect("Error saving new post");

        UserDBDTO {
            id: user.id,
            username: user.username,
            hashed_pwd: user.hashed_pwd,
            registration_date: user.registration_date,
            email: user.email,
        }
    }

    async fn get_one_by(
        specification: UsersSpecification,
        uow: &mut UnitOfWork,
    ) -> Option<UserDBDTO> {
        use crate::adapters::postgres::schema::users::dsl::*;

        let user_db = match specification {
            UsersSpecification::Id(CompType::Equals(spec_id)) => users
                .find(spec_id)
                .select(UserModel::as_select())
                .first(uow.get_conn())
                .await
                .optional(),
            UsersSpecification::Username(CompType::Equals(spec_username)) => users
                .filter(username.eq(spec_username.as_str()))
                .select(UserModel::as_select())
                .first(uow.get_conn())
                .await
                .optional(),
            _ => {
                panic!("Unsupported specification: only equals specifications for id and email supported for users now.")
            }
        };

        match user_db {
            Ok(Some(user)) => Some(UserDBDTO {
                id: user.id,
                username: user.username,
                hashed_pwd: user.hashed_pwd,
                registration_date: user.registration_date,
                email: user.email,
            }),
            Ok(None) => None,
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::unit_of_work::UnitOfWorkFactory;

    use super::*;
    use chrono::NaiveDate;
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
        pub _val: ValT,
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
            _val: (),
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
    fn default_users() -> HashMap<i32, UserDBDTO> {
        HashMap::from([(
            1,
            UserDBDTO {
                id: 1,
                username: "John".to_string(),
                hashed_pwd: "hashed_pwd##".to_string(),
                registration_date: chrono::offset::Utc::now().naive_utc(),
                email: "john@mail.com".to_string(),
            },
        )])
    }

    #[fixture]
    fn default_users_create() -> Vec<UserCreateDTO> {
        vec![UserCreateDTO {
            username: "John".to_string(),
            hashed_pwd: "hashed_pwd##".to_string(),
            registration_date: chrono::Utc::now().naive_utc(),
            email: "john@mail.com".to_string(),
        }]
    }

    #[fixture]
    fn existing_users(
        // todo: prob it's good to receive uow in transaction here
        // migrations: WithCleanup<()>,
        connection: (AsyncPgConnection, Runtime),
        default_users: HashMap<i32, UserDBDTO>,
    ) -> WithCleanup<()> {
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

        let ids_to_delete = default_users.into_keys().collect::<Vec<i32>>();

        WithCleanup {
            _val: (),
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
    fn test_get_user_should_none(
        _migrations: WithCleanup<()>,
        uow_factory: (UnitOfWorkFactory, Runtime),
    ) {
        println!("Entered test_get_user_should_none");
        let (mut uow_factory, runtime) = uow_factory;
        {
            let mut uow = runtime.block_on(uow_factory.create_uow());
            let user = runtime.block_on(UsersRepo::get_one_by(
                UsersSpecification::Id(CompType::Equals(1)),
                &mut uow,
            ));
            println!("User received from repo");
            assert_eq!(user, None);
        }
    }

    #[rstest]
    #[serial(existing_user)]
    fn test_get_user_should_some(
        _migrations: WithCleanup<()>,
        uow_factory: (UnitOfWorkFactory, Runtime),
        _existing_users: WithCleanup<()>,
        default_users: HashMap<i32, UserDBDTO>,
    ) {
        let (mut uow_factory, runtime) = uow_factory;
        for (_, user) in default_users.into_iter() {
            {
                let mut uow = runtime.block_on(uow_factory.create_uow());
                let user_in_db = runtime
                    .block_on(UsersRepo::get_one_by(
                        UsersSpecification::Id(CompType::Equals(1)),
                        &mut uow,
                    ))
                    .unwrap();

                assert!(
                    user_in_db.username == user.username
                        && user_in_db.hashed_pwd == user.hashed_pwd
                        && user_in_db.registration_date.date() == user.registration_date.date()
                        && user_in_db.email == user.email
                );
            }
        }
    }

    #[rstest]
    #[serial(existing_user)]
    fn test_create_user_one_should_succeed(
        _migrations: WithCleanup<()>,
        uow_factory: (UnitOfWorkFactory, Runtime),
        default_users_create: Vec<UserCreateDTO>,
    ) {
        let (mut uow_factory, runtime) = uow_factory;
        {
            let mut uow = runtime.block_on(uow_factory.create_uow());
            let created_user = runtime.block_on(UsersRepo::create_from_dto(
                &default_users_create[0],
                &mut uow,
            ));
            assert_eq!(created_user.id, 1);
            assert!(
                created_user.username == default_users_create[0].username
                    && created_user.hashed_pwd == default_users_create[0].hashed_pwd
                    && created_user.registration_date.date()
                        == default_users_create[0].registration_date.date()
                    && created_user.email == default_users_create[0].email
            )
        }
    }
}
