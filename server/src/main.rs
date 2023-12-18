use core::logger::{Config, Logger};
use std::{
    convert::Infallible,
    net::{SocketAddr, ToSocketAddrs},
};

use proto::towl::towl_server::Towl;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Response, Status};
use warp::Filter;

fn width_context(
    c: Context,
) -> impl Filter<Extract = (Context,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || c.clone())
}

#[derive(Clone)]
struct Context {
    logger: Logger,
}

impl Context {
    async fn init() -> Result<Self, String> {
        let config = Config::builder()
            .org("gz".into())
            .title("log".into())
            .archive_strategy(core::logger::Archive::Daily);
        let logger = Logger::init(config).await?;
        Ok(Self { logger })
    }
}

#[tonic::async_trait]
impl Towl for Context {
    type GetLogsStream = ReceiverStream<Result<proto::towl::Entry, Status>>;
    async fn get_logs(
        &self,
        request: tonic::Request<proto::towl::LogRequest>,
    ) -> Result<tonic::Response<Self::GetLogsStream>, tonic::Status> {
        // Create channel for stream response
        let (mut tx, rx) = tokio::sync::mpsc::channel(100);

        let db = self.db.clone();
        let logs_after = request.into_inner().logs_after;

        // Send the result items through the channel
        tokio::spawn(async move {
            let r = db.lock().await.clone();
            for item in r.into_iter().filter(|i| {
                if let Some(c) = i.created {
                    c.timestamp() > logs_after
                } else {
                    false
                }
            }) {
                let res: proto::towl::Entry = item.into();
                tx.send(Ok(res)).await.unwrap();
            }
        });

        // Send back the receiver
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn set_log(
    ip: Option<SocketAddr>,
    ctx: Context,
    e: Entry,
) -> Result<impl warp::Reply, Infallible> {
    let e = e.set_created().set_ip(ip);
    ctx.db.lock().await.as_mut().insert_entry(e.clone());
    Ok(warp::reply::json(&e))
}

#[tokio::main]
async fn main() {
    let context = Context::init();

    let ctx2 = context.clone();

    // Run REST API in background process
    let _ = tokio::spawn(async move {
        let log = warp::post()
            .and(warp::path("set_log"))
            .and(warp::filters::addr::remote())
            .and(width_context(context))
            .and(warp::body::json())
            .and_then(set_log);

        warp::serve(log).run(([127, 0, 0, 1], 3037)).await;
    });

    // Run GRPC service
    Server::builder()
        .add_service(proto::towl::towl_server::TowlServer::new(ctx2))
        .serve("[::1]:50011".to_socket_addrs().unwrap().next().unwrap())
        .await
        .unwrap();
}
