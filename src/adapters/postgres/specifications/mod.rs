pub enum CompType<T> {
    Equals(T),
    Gte(T),
    Lte(T),
    Lt(T),
    Gt(T),
}

pub enum UsersSpecification {
    Id(CompType<i32>),
    Username(CompType<String>),
}

pub trait Specification {}

impl Specification for UsersSpecification {}
