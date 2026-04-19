use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Small,
    Medium,
    Large,
}

impl Tier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Small => "small",
            Tier::Medium => "medium",
            Tier::Large => "large",
        }
    }

    pub const ALL: [Tier; 3] = [Tier::Small, Tier::Medium, Tier::Large];
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Tier {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "small" => Ok(Tier::Small),
            "medium" => Ok(Tier::Medium),
            "large" => Ok(Tier::Large),
            _ => Err(format!("unknown tier: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignupStatus {
    Queued,
    Claimed,
    Committed,
    Expired,
    Skipped,
    Withdrawn,
}

impl SignupStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SignupStatus::Queued => "queued",
            SignupStatus::Claimed => "claimed",
            SignupStatus::Committed => "committed",
            SignupStatus::Expired => "expired",
            SignupStatus::Skipped => "skipped",
            SignupStatus::Withdrawn => "withdrawn",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signup {
    pub id: i64,
    pub pubkey: String,
    pub tier: Tier,
    pub joined_at: i64,
    pub status: SignupStatus,
    pub slot_claimed_at: Option<i64>,
    pub slot_deadline: Option<i64>,
    pub retry_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub tier: Tier,
    pub round: i64,
    pub contribution_id: String,
    pub circuit_id: String,
    pub srs_hash: String,
    pub state_txt_hash: String,
    pub receipt_hash: String,
    pub participant_pk: Option<String>,
    pub participant_label: Option<String>,
    pub nostr_event_id: Option<String>,
    pub prev_nostr_event_id: Option<String>,
    pub created_at: i64,
    pub verified_ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrentSlot {
    pub participant_pk: String,
    pub claimed_at: i64,
    pub deadline: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierStatus {
    pub tier: Tier,
    pub head_round: i64,
    pub queue_depth: i64,
    pub current_slot: Option<CurrentSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub tiers: Vec<TierStatus>,
    pub as_of: i64,
    pub limits: StatusLimits,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusLimits {
    pub slot_deadline_secs: i64,
    pub queue_idle_secs: i64,
}

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub tier: Tier,
    pub display_name: Option<String>,
    pub email_optional: Option<String>,
    pub pow: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SignupResponse {
    pub position: i64,
    pub signup_id: i64,
}

#[derive(Debug, Serialize)]
pub struct BlobRef {
    pub sha256: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct ClaimResponse {
    pub round: i64,
    pub deadline: i64,
    pub expected_before_srs_hash: String,
    pub download: ClaimDownload,
}

#[derive(Debug, Serialize)]
pub struct ClaimDownload {
    pub state_srs: BlobRef,
    pub state_txt: BlobRef,
    pub receipt: BlobRef,
}

#[derive(Debug, Serialize)]
pub struct ContributeResponse {
    pub round: i64,
    pub contribution_id: String,
    pub srs_hash: String,
    pub nostr_event_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyStateRequest {
    pub state_txt: String,
    pub srs_blob_sha256: String,
    pub receipt: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyStateResponse {
    pub ok: bool,
    pub round: Option<i64>,
    pub tier: Option<Tier>,
    pub srs_hash: Option<String>,
    pub error: Option<String>,
}

pub fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
