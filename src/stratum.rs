use tokio::net::TcpStream;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};

pub struct StratumClient {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    job_id: String,
    blob: String,
    target: String,
}

impl StratumClient {
    pub async fn connect(pool_url: &str, wallet: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut stream = TcpStream::connect(pool_url).await?;
        let reader = BufReader::new(stream.try_clone().await?);
        let mut client = StratumClient { stream, reader, job_id: "".into(), blob: "".into(), target: "".into() };
        
        // Envia subscribe
        client.send_request("mining.subscribe", json!(["monero-miner/1.0"])).await?;
        // Envia authorize
        client.send_request("mining.authorize", json!([wallet, "x"])).await?;
        
        Ok(client)
    }
    
    async fn send_request(&mut self, method: &str, params: Value) -> Result<(), Box<dyn std::error::Error>> {
        let id = 1;
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params, "id": id });
        self.stream.write_all(msg.to_string().as_bytes()).await?;
        self.stream.write_all(b"\n").await?;
        Ok(())
    }
    
    pub async fn read_job(&mut self) -> Result<(String, String, String), Box<dyn std::error::Error>> {
        let mut line = String::new();
        self.reader.read_line(&mut line).await?;
        let resp: Value = serde_json::from_str(&line)?;
        if let Some(params) = resp.get("params") {
            let job_id = params[0].as_str().unwrap().to_string();
            let blob = params[1].as_str().unwrap().to_string();
            let target = params[2].as_str().unwrap().to_string();
            Ok((job_id, blob, target))
        } else {
            Err("Not a job notification".into())
        }
    }
}
