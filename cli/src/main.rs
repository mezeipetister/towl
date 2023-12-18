use proto::towl::{Entry, GetRequest};

#[tokio::main]
async fn main() -> Result<(), String> {
  let mut client = proto::towl::towl_server_client::TowlServerClient::connect("http://[::1]:50011")
    .await
    .unwrap();

  let mut log_stream = client
    .get_logs(LogRequest { logs_after: 0 })
    .await
    .map_err(|e| e.to_string())?
    .into_inner();

  let mut result: Vec<Entry> = Vec::new();
  while let Some(entry) = log_stream.message().await.map_err(|e| e.to_string())? {
    println!("{:?}", &entry);
    result.push(entry);
  }

  println!("Result count is {}", result.len());

  Ok(())
}
