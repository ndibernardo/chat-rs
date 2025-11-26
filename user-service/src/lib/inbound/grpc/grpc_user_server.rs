use std::sync::Arc;

use tonic::Request;
use tonic::Response;
use tonic::Status;

use super::handlers::get_user;
use crate::domain::user::service::UserService;
use crate::outbound::events::KafkaEventProducer;
use crate::outbound::repositories::PostgresUserRepository;
use crate::proto::user_service_server::UserService as UserServiceProto;
use crate::proto::GetUserRequest;
use crate::proto::GetUserResponse;

pub struct UserGrpcService {
    service: Arc<UserService<PostgresUserRepository, KafkaEventProducer>>,
}

impl UserGrpcService {
    pub fn new(service: Arc<UserService<PostgresUserRepository, KafkaEventProducer>>) -> Self {
        Self { service }
    }
}

#[tonic::async_trait]
impl UserServiceProto for UserGrpcService {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<GetUserResponse>, Status> {
        let response = get_user::get_user(self.service.clone(), request.into_inner()).await?;
        Ok(Response::new(response))
    }
}
