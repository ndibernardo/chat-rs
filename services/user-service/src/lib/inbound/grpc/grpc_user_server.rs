use std::sync::Arc;

use tonic::Request;
use tonic::Response;
use tonic::Status;

use super::handlers::get_user;
use super::proto::GetUserRequest;
use super::proto::GetUserResponse;
use super::proto::user_service_server::UserService as UserServiceProto;
use crate::domain::user::service::Service;
use crate::outbound::argon2::PasswordHasher;
use crate::outbound::kafka::EventProducer;
use crate::outbound::postgres::UserRepository;

pub struct UserGrpcService {
    service: Arc<Service<UserRepository, EventProducer, PasswordHasher>>,
}

impl UserGrpcService {
    pub fn new(service: Arc<Service<UserRepository, EventProducer, PasswordHasher>>) -> Self {
        Self { service }
    }
}

#[tonic::async_trait]
impl UserServiceProto for UserGrpcService {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<GetUserResponse>, Status> {
        let response = get_user::get_user(Arc::clone(&self.service), request.into_inner()).await?;
        Ok(Response::new(response))
    }
}
