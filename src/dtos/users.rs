use chrono::prelude::*;
use utoipa::ToSchema;

#[derive(Debug, PartialEq, Clone)]
pub struct UserCreateDTO {
    pub username: String,
    pub hashed_pwd: String,
    pub registration_date: NaiveDateTime,
    pub interests: String,
}

#[derive(Debug, PartialEq, Clone)]
pub struct UserDBDTO {
    pub id: i32,
    pub username: String,
    pub hashed_pwd: String,
    pub registration_date: NaiveDateTime,
    pub interests: String,
}

#[derive(Debug, PartialEq, Clone, serde::Deserialize, serde::Serialize, ToSchema)]
pub struct UserCreateInDTO {
    pub username: String,
    pub password: String,
    pub interests: String,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize, ToSchema)]
pub struct UserOutDTO {
    pub id: i32,
    pub username: String,
    pub interests: String,
}

#[derive(serde::Deserialize, ToSchema)]
pub struct SignInData {
    pub username: String,
    pub password: String,
}
