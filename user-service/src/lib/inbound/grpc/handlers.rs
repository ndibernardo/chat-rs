use crate::domain::user::models::User;

pub mod get_user;

impl From<User> for crate::proto::User {
    fn from(user: User) -> Self {
        Self {
            id: user.id.to_string(),
            username: user.username.as_str().to_string(),
            email: user.email.as_str().to_string(),
            created_at: user.created_at.to_rfc3339(),
        }
    }
}
