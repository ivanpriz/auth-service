// @generated automatically by Diesel CLI.

diesel::table! {
    users (id) {
        id -> Int4,
        username -> Varchar,
        hashed_pwd -> Varchar,
        registration_date -> Timestamp,
        interests -> Text,
    }
}
