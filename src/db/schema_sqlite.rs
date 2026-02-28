// SQLite schema definitions
// This file mirrors schema.rs but uses SQLite-compatible types

diesel::table! {
    room_mappings (id) {
        id -> Integer,
        matrix_room_id -> Text,
        slack_channel_id -> Text,
        slack_channel_name -> Text,
        slack_team_id -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    user_mappings (id) {
        id -> Integer,
        matrix_user_id -> Text,
        slack_user_id -> Text,
        slack_username -> Text,
        slack_discriminator -> Text,
        slack_avatar -> Nullable<Text>,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    processed_events (id) {
        id -> Integer,
        event_id -> Text,
        event_type -> Text,
        source -> Text,
        processed_at -> Text,
    }
}

diesel::table! {
    message_mappings (id) {
        id -> Integer,
        slack_message_id -> Text,
        matrix_room_id -> Text,
        matrix_event_id -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::table! {
    emoji_mappings (id) {
        id -> Integer,
        slack_emoji_id -> Text,
        emoji_name -> Text,
        animated -> Bool,
        mxc_url -> Text,
        created_at -> Text,
        updated_at -> Text,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    room_mappings,
    user_mappings,
    processed_events,
    message_mappings,
    emoji_mappings,
);
