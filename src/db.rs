pub use self::error::DatabaseError;
pub use self::manager::DatabaseManager;
pub use self::models::{
    EmojiMapping, MessageMapping, ProcessedEvent, RemoteRoomInfo, RemoteUserInfo, RoomMapping,
    UserMapping,
};
pub use self::stores::{EmojiStore, MessageStore, RoomStore, UserStore};

pub mod error;
pub mod manager;
pub mod models;
#[cfg(feature = "postgres")]
pub mod schema;
pub mod stores;

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "sqlite")]
pub mod schema_sqlite;

#[cfg(feature = "mysql")]
pub mod mysql;

#[cfg(feature = "mysql")]
pub mod schema_mysql;
