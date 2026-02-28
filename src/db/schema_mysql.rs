// MySQL schema definitions
// Mirrors schema.rs with MySQL-compatible timestamp type.

diesel::table! {
    room_mappings (id) {
        id -> BigInt,
        matrix_room_id -> Text,
        slack_channel_id -> Text,
        slack_channel_name -> Text,
        slack_team_id -> Text,
        created_at -> Datetime,
        updated_at -> Datetime,
    }
}

diesel::table! {
    user_mappings (id) {
        id -> BigInt,
        matrix_user_id -> Text,
        slack_user_id -> Text,
        slack_username -> Text,
        slack_discriminator -> Text,
        slack_avatar -> Nullable<Text>,
        created_at -> Datetime,
        updated_at -> Datetime,
    }
}

diesel::table! {
    processed_events (id) {
        id -> BigInt,
        event_id -> Text,
        event_type -> Text,
        source -> Text,
        processed_at -> Datetime,
    }
}

diesel::table! {
    message_mappings (id) {
        id -> BigInt,
        slack_message_id -> Text,
        matrix_room_id -> Text,
        matrix_event_id -> Text,
        created_at -> Datetime,
        updated_at -> Datetime,
    }
}

diesel::table! {
    emoji_mappings (id) {
        id -> BigInt,
        slack_emoji_id -> Text,
        emoji_name -> Text,
        animated -> Bool,
        mxc_url -> Text,
        created_at -> Datetime,
        updated_at -> Datetime,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    room_mappings,
    user_mappings,
    processed_events,
    message_mappings,
    emoji_mappings,
);
