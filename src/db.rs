use diesel::prelude::*;
use rocket::Rocket;
use rocket_contrib::database;

// This macro from `diesel_migrations` defines an `embedded_migrations` module
// containing a function named `run`. This allows the example to be run and
// tested without any outside setup of the database.
#[cfg(feature = "postgres")]
embed_migrations!("migrations/postgres");

#[cfg(feature = "postgres")]
#[database("postgres_db")]
pub struct DbConn(PgConnection);

#[cfg(feature = "postgres")]
pub type ConnectionType = PgConnection;

#[cfg(feature = "sqlite")]
embed_migrations!("migrations/sqlite");

#[cfg(feature = "sqlite")]
#[database("sqlite_db")]
pub struct DbConn(SqliteConnection);

#[cfg(feature = "sqlite")]
pub type ConnectionType = SqliteConnection;

pub fn run_rocket_db_migrations(rocket: Rocket) -> Result<Rocket, Rocket> {
    let conn = DbConn::get_one(&rocket).expect("database connection");
    match run_db_migrations(&*conn) {
        Ok(()) => Ok(rocket),
        Err(e) => {
            error!("Failed to run database migrations: {:?}", e);
            Err(rocket)
        },
    }
}

pub fn run_db_migrations(conn: &ConnectionType) -> Result<(), diesel_migrations::RunMigrationsError> {
    embedded_migrations::run(conn)
}

impl DbConnPool {
    pub fn get_one(&self) -> Option<DbConn> {
        self.0.get().ok().map(DbConn)
    }

    pub fn new(pool: rocket_contrib::databases::r2d2::Pool<<ConnectionType as rocket_contrib::databases::Poolable>::Manager>) -> Self {
        DbConnPool(pool)
    }
}

impl Clone for DbConnPool {
    fn clone(&self) -> Self {
        DbConnPool(self.0.clone())
    }
}
