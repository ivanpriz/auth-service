use crate::dtos::users::UserDBDTO;
use chrono::prelude::*;
use diesel::prelude::*;

#[derive(Queryable, Selectable, PartialEq, Insertable)]
#[diesel(table_name = super::schema::users)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct UserModel {
    pub id: i32,
    pub username: String,
    pub hashed_pwd: String,
    pub registration_date: NaiveDateTime,
    pub interests: String,
}

impl UserModel {
    pub fn from_dto(dto: &UserDBDTO) -> Self {
        Self {
            id: dto.id,
            username: dto.username.clone(),
            hashed_pwd: dto.hashed_pwd.clone(),
            registration_date: dto.registration_date,
            interests: dto.interests.clone(),
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = super::schema::users)]
pub struct NewUserModel<'a> {
    pub username: &'a str,
    pub hashed_pwd: &'a str,
    pub registration_date: &'a NaiveDateTime,
    pub interests: &'a str,
}
