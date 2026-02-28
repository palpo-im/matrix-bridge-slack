use std::sync::Arc;

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use diesel::RunQueryDsl;
#[cfg(feature = "mysql")]
use diesel::mysql::MysqlConnection;
#[cfg(feature = "postgres")]
use diesel::pg::PgConnection;
#[cfg(any(feature = "postgres", feature = "mysql"))]
use diesel::r2d2::{self, ConnectionManager};

use crate::config::{DatabaseConfig as ConfigDatabaseConfig, DbType as ConfigDbType};
#[cfg(feature = "mysql")]
use crate::db::mysql::{MysqlEmojiStore, MysqlMessageStore, MysqlRoomStore, MysqlUserStore};
#[cfg(feature = "postgres")]
use crate::db::postgres::{
    PostgresEmojiStore, PostgresMessageStore, PostgresRoomStore, PostgresUserStore,
};
use crate::db::{DatabaseError, EmojiStore, MessageStore, RoomStore, UserStore};

#[cfg(feature = "postgres")]
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;
#[cfg(feature = "mysql")]
pub type MysqlPool = r2d2::Pool<ConnectionManager<MysqlConnection>>;

#[cfg(feature = "sqlite")]
use diesel::Connection;
#[cfg(feature = "sqlite")]
use diesel::sqlite::SqliteConnection;

#[cfg(feature = "sqlite")]
use crate::db::sqlite::{SqliteEmojiStore, SqliteMessageStore, SqliteRoomStore, SqliteUserStore};

#[derive(Clone)]
pub struct DatabaseManager {
    #[cfg(feature = "postgres")]
    postgres_pool: Option<Pool>,
    #[cfg(feature = "mysql")]
    mysql_pool: Option<MysqlPool>,
    #[cfg(feature = "sqlite")]
    sqlite_path: Option<String>,
    room_store: Arc<dyn RoomStore>,
    user_store: Arc<dyn UserStore>,
    message_store: Arc<dyn MessageStore>,
    emoji_store: Arc<dyn EmojiStore>,
    db_type: DbType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbType {
    Postgres,
    Sqlite,
    Mysql,
}

impl From<ConfigDbType> for DbType {
    fn from(value: ConfigDbType) -> Self {
        match value {
            ConfigDbType::Postgres => DbType::Postgres,
            ConfigDbType::Sqlite => DbType::Sqlite,
            ConfigDbType::Mysql => DbType::Mysql,
        }
    }
}

impl DatabaseManager {
    pub async fn new(config: &ConfigDatabaseConfig) -> Result<Self, DatabaseError> {
        let db_type = DbType::from(config.db_type());

        match db_type {
            #[cfg(feature = "postgres")]
            DbType::Postgres => {
                let connection_string = config.connection_string();
                let max_connections = config.max_connections();
                let min_connections = config.min_connections();

                let manager = ConnectionManager::<PgConnection>::new(connection_string);

                let builder = r2d2::Pool::builder()
                    .max_size(max_connections.unwrap_or(10))
                    .min_idle(Some(min_connections.unwrap_or(1)));

                let pool = builder
                    .build(manager)
                    .map_err(|e| DatabaseError::Connection(e.to_string()))?;

                let room_store = Arc::new(PostgresRoomStore::new(pool.clone()));
                let user_store = Arc::new(PostgresUserStore::new(pool.clone()));
                let message_store = Arc::new(PostgresMessageStore::new(pool.clone()));
                let emoji_store = Arc::new(PostgresEmojiStore::new(pool.clone()));

                Ok(Self {
                    postgres_pool: Some(pool),
                    #[cfg(feature = "mysql")]
                    mysql_pool: None,
                    #[cfg(feature = "sqlite")]
                    sqlite_path: None,
                    room_store,
                    user_store,
                    message_store,
                    emoji_store,
                    db_type,
                })
            }
            #[cfg(feature = "sqlite")]
            DbType::Sqlite => {
                let path = config.sqlite_path().unwrap();
                let path_arc = Arc::new(path.clone());

                let room_store = Arc::new(SqliteRoomStore::new(path_arc.clone()));
                let user_store = Arc::new(SqliteUserStore::new(path_arc.clone()));
                let message_store = Arc::new(SqliteMessageStore::new(Arc::new(path.clone())));
                let emoji_store = Arc::new(SqliteEmojiStore::new(path_arc));

                Ok(Self {
                    #[cfg(feature = "postgres")]
                    postgres_pool: None,
                    #[cfg(feature = "mysql")]
                    mysql_pool: None,
                    sqlite_path: Some(path),
                    room_store,
                    user_store,
                    message_store,
                    emoji_store,
                    db_type,
                })
            }
            #[cfg(feature = "mysql")]
            DbType::Mysql => {
                let connection_string = config.connection_string();
                let max_connections = config.max_connections();
                let min_connections = config.min_connections();

                let manager = ConnectionManager::<MysqlConnection>::new(connection_string);

                let builder = r2d2::Pool::builder()
                    .max_size(max_connections.unwrap_or(10))
                    .min_idle(Some(min_connections.unwrap_or(1)));

                let pool = builder
                    .build(manager)
                    .map_err(|e| DatabaseError::Connection(e.to_string()))?;

                let room_store = Arc::new(MysqlRoomStore::new(pool.clone()));
                let user_store = Arc::new(MysqlUserStore::new(pool.clone()));
                let message_store = Arc::new(MysqlMessageStore::new(pool.clone()));
                let emoji_store = Arc::new(MysqlEmojiStore::new(pool.clone()));

                Ok(Self {
                    #[cfg(feature = "postgres")]
                    postgres_pool: None,
                    mysql_pool: Some(pool),
                    #[cfg(feature = "sqlite")]
                    sqlite_path: None,
                    room_store,
                    user_store,
                    message_store,
                    emoji_store,
                    db_type,
                })
            }
            #[cfg(not(feature = "postgres"))]
            DbType::Postgres => {
                return Err(DatabaseError::Connection(
                    "PostgreSQL feature not enabled".to_string(),
                ));
            }
            #[cfg(not(feature = "sqlite"))]
            DbType::Sqlite => {
                return Err(DatabaseError::Connection(
                    "SQLite feature not enabled".to_string(),
                ));
            }
            #[cfg(not(feature = "mysql"))]
            DbType::Mysql => {
                Err(DatabaseError::Connection(
                    "MySQL feature not enabled".to_string(),
                ))
            }
        }
    }

    #[cfg(feature = "sqlite")]
    pub fn new_in_memory() -> Result<Self, DatabaseError> {
        use std::sync::Arc;

        let path_arc = Arc::new(":memory:".to_string());

        let room_store = Arc::new(SqliteRoomStore::new(path_arc.clone()));
        let user_store = Arc::new(SqliteUserStore::new(path_arc.clone()));
        let message_store = Arc::new(SqliteMessageStore::new(path_arc.clone()));
        let emoji_store = Arc::new(SqliteEmojiStore::new(path_arc));

        Ok(Self {
            #[cfg(feature = "postgres")]
            postgres_pool: None,
            #[cfg(feature = "mysql")]
            mysql_pool: None,
            sqlite_path: Some(":memory:".to_string()),
            room_store,
            user_store,
            message_store,
            emoji_store,
            db_type: DbType::Sqlite,
        })
    }

    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        match self.db_type {
            #[cfg(feature = "postgres")]
            DbType::Postgres => {
                let pool = self.postgres_pool.as_ref().unwrap();
                return Self::migrate_postgres(pool).await;
            }
            #[cfg(feature = "sqlite")]
            DbType::Sqlite => {
                let path = self.sqlite_path.as_ref().unwrap();
                return Self::migrate_sqlite(path).await;
            }
            #[cfg(feature = "mysql")]
            DbType::Mysql => {
                let pool = self.mysql_pool.as_ref().unwrap();
                return Self::migrate_mysql(pool).await;
            }
            #[cfg(not(feature = "postgres"))]
            DbType::Postgres => {
                return Err(DatabaseError::Migration(
                    "PostgreSQL feature not enabled".to_string(),
                ));
            }
            #[cfg(not(feature = "sqlite"))]
            DbType::Sqlite => {
                return Err(DatabaseError::Migration(
                    "SQLite feature not enabled".to_string(),
                ));
            }
            #[cfg(not(feature = "mysql"))]
            DbType::Mysql => {
                Err(DatabaseError::Migration(
                    "MySQL feature not enabled".to_string(),
                ))
            }
        }
    }

    #[cfg(feature = "postgres")]
    async fn migrate_postgres(pool: &Pool) -> Result<(), DatabaseError> {
        let pool = pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| DatabaseError::Connection(e.to_string()))?;

            let statements = [
                r#"
                CREATE TABLE IF NOT EXISTS user_mappings (
                    id BIGSERIAL PRIMARY KEY,
                    matrix_user_id TEXT NOT NULL UNIQUE,
                    slack_user_id TEXT NOT NULL UNIQUE,
                    slack_username TEXT NOT NULL,
                    slack_discriminator TEXT NOT NULL,
                    slack_avatar TEXT,
                    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS room_mappings (
                    id BIGSERIAL PRIMARY KEY,
                    matrix_room_id TEXT NOT NULL UNIQUE,
                    slack_channel_id TEXT NOT NULL UNIQUE,
                    slack_channel_name TEXT NOT NULL,
                    slack_team_id TEXT NOT NULL,
                    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS processed_events (
                    id BIGSERIAL PRIMARY KEY,
                    event_id TEXT NOT NULL UNIQUE,
                    event_type TEXT NOT NULL,
                    source TEXT NOT NULL,
                    processed_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS message_mappings (
                    id BIGSERIAL PRIMARY KEY,
                    slack_message_id TEXT NOT NULL UNIQUE,
                    matrix_room_id TEXT NOT NULL,
                    matrix_event_id TEXT NOT NULL,
                    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS user_activity (
                    id BIGSERIAL PRIMARY KEY,
                    user_mapping_id BIGINT NOT NULL REFERENCES user_mappings(id) ON DELETE CASCADE,
                    activity_type TEXT NOT NULL,
                    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                    metadata JSONB
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS emoji_mappings (
                    id BIGSERIAL PRIMARY KEY,
                    slack_emoji_id TEXT NOT NULL UNIQUE,
                    emoji_name TEXT NOT NULL,
                    animated BOOLEAN NOT NULL DEFAULT FALSE,
                    mxc_url TEXT NOT NULL,
                    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
                )
                "#,
                "CREATE INDEX IF NOT EXISTS idx_user_mappings_matrix_id ON user_mappings(matrix_user_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_mappings_slack_id ON user_mappings(slack_user_id)",
                "CREATE INDEX IF NOT EXISTS idx_room_mappings_matrix_id ON room_mappings(matrix_room_id)",
                "CREATE INDEX IF NOT EXISTS idx_room_mappings_slack_id ON room_mappings(slack_channel_id)",
                "CREATE INDEX IF NOT EXISTS idx_processed_events_event_id ON processed_events(event_id)",
                "CREATE INDEX IF NOT EXISTS idx_message_mappings_slack_id ON message_mappings(slack_message_id)",
                "CREATE INDEX IF NOT EXISTS idx_message_mappings_matrix_event ON message_mappings(matrix_event_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_activity_user_mapping ON user_activity(user_mapping_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_activity_timestamp ON user_activity(timestamp)",
                "CREATE INDEX IF NOT EXISTS idx_emoji_mappings_slack_id ON emoji_mappings(slack_emoji_id)",
                "CREATE INDEX IF NOT EXISTS idx_emoji_mappings_mxc ON emoji_mappings(mxc_url)",
            ];

            for statement in statements {
                diesel::sql_query(statement)
                    .execute(&mut conn)
                    .map_err(|e| DatabaseError::Migration(e.to_string()))?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DatabaseError::Migration(format!("migration task failed: {e}")))?
    }

    #[cfg(feature = "mysql")]
    async fn migrate_mysql(pool: &MysqlPool) -> Result<(), DatabaseError> {
        let pool = pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| DatabaseError::Connection(e.to_string()))?;

            let statements = [
                r#"
                CREATE TABLE IF NOT EXISTS user_mappings (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    matrix_user_id VARCHAR(255) NOT NULL UNIQUE,
                    slack_user_id VARCHAR(64) NOT NULL UNIQUE,
                    slack_username VARCHAR(255) NOT NULL,
                    slack_discriminator VARCHAR(32) NOT NULL,
                    slack_avatar TEXT NULL,
                    created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS room_mappings (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    matrix_room_id VARCHAR(255) NOT NULL UNIQUE,
                    slack_channel_id VARCHAR(64) NOT NULL UNIQUE,
                    slack_channel_name VARCHAR(255) NOT NULL,
                    slack_team_id VARCHAR(64) NOT NULL,
                    created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
                    KEY idx_room_mappings_guild (slack_team_id)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS processed_events (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    event_id VARCHAR(255) NOT NULL UNIQUE,
                    event_type VARCHAR(128) NOT NULL,
                    source VARCHAR(128) NOT NULL,
                    processed_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS message_mappings (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    slack_message_id VARCHAR(64) NOT NULL UNIQUE,
                    matrix_room_id VARCHAR(255) NOT NULL,
                    matrix_event_id VARCHAR(255) NOT NULL,
                    created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
                    KEY idx_message_mappings_matrix_event (matrix_event_id)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS user_activity (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    user_mapping_id BIGINT NOT NULL,
                    activity_type VARCHAR(128) NOT NULL,
                    timestamp DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    metadata JSON NULL,
                    KEY idx_user_activity_user_mapping (user_mapping_id),
                    KEY idx_user_activity_timestamp (timestamp),
                    CONSTRAINT fk_user_activity_user_mapping
                        FOREIGN KEY (user_mapping_id) REFERENCES user_mappings(id) ON DELETE CASCADE
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS emoji_mappings (
                    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
                    slack_emoji_id VARCHAR(64) NOT NULL UNIQUE,
                    emoji_name VARCHAR(255) NOT NULL,
                    animated BOOLEAN NOT NULL DEFAULT FALSE,
                    mxc_url VARCHAR(1024) NOT NULL,
                    created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
                    updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
                    KEY idx_emoji_mappings_mxc (mxc_url)
                ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4
                "#,
            ];

            for statement in statements {
                diesel::sql_query(statement)
                    .execute(&mut conn)
                    .map_err(|e| DatabaseError::Migration(e.to_string()))?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DatabaseError::Migration(format!("migration task failed: {e}")))?
    }

    #[cfg(feature = "sqlite")]
    async fn migrate_sqlite(path: &str) -> Result<(), DatabaseError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let conn_string = format!("sqlite://{}", path);
            let mut conn = SqliteConnection::establish(&conn_string)
                .map_err(|e| DatabaseError::Connection(e.to_string()))?;

            let statements = [
                r#"
                CREATE TABLE IF NOT EXISTS user_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    matrix_user_id TEXT NOT NULL UNIQUE,
                    slack_user_id TEXT NOT NULL UNIQUE,
                    slack_username TEXT NOT NULL,
                    slack_discriminator TEXT NOT NULL,
                    slack_avatar TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS room_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    matrix_room_id TEXT NOT NULL UNIQUE,
                    slack_channel_id TEXT NOT NULL UNIQUE,
                    slack_channel_name TEXT NOT NULL,
                    slack_team_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS processed_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_id TEXT NOT NULL UNIQUE,
                    event_type TEXT NOT NULL,
                    source TEXT NOT NULL,
                    processed_at TEXT NOT NULL DEFAULT (datetime('now'))
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS message_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    slack_message_id TEXT NOT NULL UNIQUE,
                    matrix_room_id TEXT NOT NULL,
                    matrix_event_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS user_activity (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_mapping_id INTEGER NOT NULL REFERENCES user_mappings(id) ON DELETE CASCADE,
                    activity_type TEXT NOT NULL,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                    metadata TEXT
                )
                "#,
                r#"
                CREATE TABLE IF NOT EXISTS emoji_mappings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    slack_emoji_id TEXT NOT NULL UNIQUE,
                    emoji_name TEXT NOT NULL,
                    animated INTEGER NOT NULL DEFAULT 0,
                    mxc_url TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                )
                "#,
                "CREATE INDEX IF NOT EXISTS idx_user_mappings_matrix_id ON user_mappings(matrix_user_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_mappings_slack_id ON user_mappings(slack_user_id)",
                "CREATE INDEX IF NOT EXISTS idx_room_mappings_matrix_id ON room_mappings(matrix_room_id)",
                "CREATE INDEX IF NOT EXISTS idx_room_mappings_slack_id ON room_mappings(slack_channel_id)",
                "CREATE INDEX IF NOT EXISTS idx_processed_events_event_id ON processed_events(event_id)",
                "CREATE INDEX IF NOT EXISTS idx_message_mappings_slack_id ON message_mappings(slack_message_id)",
                "CREATE INDEX IF NOT EXISTS idx_message_mappings_matrix_event ON message_mappings(matrix_event_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_activity_user_mapping ON user_activity(user_mapping_id)",
                "CREATE INDEX IF NOT EXISTS idx_user_activity_timestamp ON user_activity(timestamp)",
                "CREATE INDEX IF NOT EXISTS idx_emoji_mappings_slack_id ON emoji_mappings(slack_emoji_id)",
                "CREATE INDEX IF NOT EXISTS idx_emoji_mappings_mxc ON emoji_mappings(mxc_url)",
            ];

            for statement in statements {
                diesel::sql_query(statement)
                    .execute(&mut conn)
                    .map_err(|e| DatabaseError::Migration(e.to_string()))?;
            }

            Ok(())
        })
        .await
        .map_err(|e| DatabaseError::Migration(format!("migration task failed: {e}")))?
    }

    pub fn room_store(&self) -> Arc<dyn RoomStore> {
        self.room_store.clone()
    }

    pub fn user_store(&self) -> Arc<dyn UserStore> {
        self.user_store.clone()
    }

    pub fn message_store(&self) -> Arc<dyn MessageStore> {
        self.message_store.clone()
    }

    pub fn emoji_store(&self) -> Arc<dyn EmojiStore> {
        self.emoji_store.clone()
    }

    #[cfg(feature = "postgres")]
    pub fn pool(&self) -> Option<&Pool> {
        self.postgres_pool.as_ref()
    }

    pub fn db_type(&self) -> DbType {
        self.db_type
    }
}

