// Ideas:
// We take a mutable reference of uow into all repo methods
// On uow method we provide a connection from the db level (or whatever else),
// but to use this method you should import trait AllowedToImolort only on db level,
// it can also be not public for the module, but available for it's children.
// In this case we will be only able to use method that returns type from db level
// On the db level. On the higher level this type doesn't have to provide any methods,
// or only those without showing the db types - e.g. commit, rollback. Actually for db
// I should think if we can't close not the transaction we opened due to race condition.
// E.g. opened transaction 1, opened transaction 2, call commit from transaction 1 due to
// async runtime taking control of the thread and executing closing the innermost transaction (2),
// And then in transaction 2 we will actually commit what is present after changes in transaction
// 1. Other than that - perfect abstraction.
// There might be a problem with mutable reference escaping the method body, cuz we get uow by
// mutable reference, then get mutable reference to it's part, and then pass it to diesel method.
// If this is reference escaping method body, we can wrap pool into arcmutex, and get references to
// it whenever we want."

// use diesel_async::pooled_connection::deadpool::managed::{Manager, PoolError};
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::{AnsiTransactionManager, TransactionManager};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};

use super::UnitOfWorkInternal;

pub struct UnitOfWorkFactory {
    conn_pool: Pool<AsyncPgConnection>,
}

impl UnitOfWorkFactory {
    pub async fn create_uow(&mut self) -> UnitOfWork {
        let mut conn = self.conn_pool.get().await.unwrap();
        UnitOfWork::new(conn)
    }

    pub fn new(conn_pool: Pool<AsyncPgConnection>) -> Self {
        Self { conn_pool }
    }
}
pub struct UnitOfWork {
    // conn: AsyncPgConnection,
    conn: deadpool::managed::Object<AsyncDieselConnectionManager<AsyncPgConnection>>,
}

impl UnitOfWork {
    fn new(
        conn: deadpool::managed::Object<AsyncDieselConnectionManager<AsyncPgConnection>>,
    ) -> Self {
        Self { conn }
    }
}

impl UnitOfWorkInternal for UnitOfWork {
    fn get_conn(&mut self) -> &mut diesel_async::AsyncPgConnection {
        &mut self.conn
    }
}

pub trait UnitOfWorkPublic {
    async fn begin_transaction(&mut self);

    async fn commit(&mut self);

    async fn rollback(&mut self);
}

impl UnitOfWorkPublic for UnitOfWork {
    async fn begin_transaction(&mut self) {
        AnsiTransactionManager::begin_transaction(self.get_conn())
            .await
            .unwrap();
    }

    async fn commit(&mut self) {
        AnsiTransactionManager::commit_transaction(self.get_conn())
            .await
            .unwrap();
    }

    async fn rollback(&mut self) {
        AnsiTransactionManager::rollback_transaction(self.get_conn())
            .await
            .unwrap();
    }
}
