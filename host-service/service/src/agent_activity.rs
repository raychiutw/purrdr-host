//! 以 herdr `session.snapshot` 判斷有沒有 `working` 的 Agent(raychiutw/purrdr.host#20)。

use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::herdr_client::HerdrClient;
use crate::herdr_socket::HerdrSocket;
use crate::update::AgentActivity;

/// 問 herdr 本機 socket 的 `AgentActivity`。
pub struct HerdrAgentActivity {
    client: HerdrClient,
    runtime: Handle,
}

impl HerdrAgentActivity {
    /// `runtime` 用來在同步的更新流程裡等 herdr 回應。
    pub fn new(socket: HerdrSocket, runtime: Handle) -> Self {
        HerdrAgentActivity {
            client: HerdrClient::new(socket),
            runtime,
        }
    }
}

impl AgentActivity for HerdrAgentActivity {
    /// herdr 連不上或回應看不懂時視為閒置。
    fn has_working_agent(&mut self) -> bool {
        match self.runtime.block_on(self.client.request("session.snapshot", json!({}))) {
            Ok(result) => snapshot_has_working_agent(&result),
            Err(_) => false,
        }
    }
}

fn snapshot_has_working_agent(result: &Value) -> bool {
    result["snapshot"]["agents"]
        .as_array()
        .is_some_and(|agents| agents.iter().any(|agent| agent["agent_status"] == "working"))
}
