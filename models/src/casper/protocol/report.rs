//! Casper block-report protocol types (port of the `Report*` / `*EventData` messages in
//! `DeployServiceCommon.proto`).

use serde::{Deserialize, Serialize};

use crate::ast::Par;
use crate::block::state_hash::StateHash;
use crate::casper::protocol::casper_message::{Peek, SystemDeployData};
use crate::casper::protocol::deploy_service::{DeployInfo, LightBlockInfo};
use crate::runtime::{BindPattern, ListParWithRandom};

/// `ReportProduceProto` — a produce event (channel + data).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportProduceProto {
    pub channel: Par,
    pub data: ListParWithRandom,
}

/// `ReportConsumeProto` — a consume event (channels + patterns + peeks).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportConsumeProto {
    pub channels: Vec<Par>,
    pub patterns: Vec<BindPattern>,
    pub peeks: Vec<Peek>,
}

/// `ReportCommProto` — a comm event (one consume + many produces).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCommProto {
    pub consume: ReportConsumeProto,
    pub produces: Vec<ReportProduceProto>,
}

/// `ReportProto` — the `oneof report` event sum type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReportProto {
    Produce(ReportProduceProto),
    Consume(ReportConsumeProto),
    Comm(ReportCommProto),
}

/// `SingleReport` — the events produced by one deploy/soft-checkpoint segment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleReport {
    pub events: Vec<ReportProto>,
}

/// `DeployInfoWithEventData` — a user deploy plus its report.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployInfoWithEventData {
    pub deploy_info: DeployInfo,
    pub report: Vec<SingleReport>,
}

/// `SystemDeployInfoWithEventData` — a system deploy plus its report.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDeployInfoWithEventData {
    pub system_deploy: SystemDeployData,
    pub report: Vec<SingleReport>,
}

/// `BlockEventInfo` — the full per-block report.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockEventInfo {
    pub block_info: LightBlockInfo,
    pub deploys: Vec<DeployInfoWithEventData>,
    pub system_deploys: Vec<SystemDeployInfoWithEventData>,
    /// `StateHash`, not a hex-serde'd `Vec<u8>` (deferred item 1b). The JSON is unchanged — this
    /// type serializes as the same lowercase base16 — but the field now refuses a wrong-length value
    /// where the `Vec<u8>` accepted one.
    pub post_state_hash: StateHash,
}
