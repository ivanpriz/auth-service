mod repo_trait;
mod unit_of_work;
mod users;

trait UnitOfWorkInternal {
    fn get_conn(&mut self) -> &mut diesel_async::AsyncPgConnection;
}

pub use repo_trait::Repository;
pub use unit_of_work::{UnitOfWork, UnitOfWorkFactory, UnitOfWorkPublic};
pub use users::UsersRepo;
