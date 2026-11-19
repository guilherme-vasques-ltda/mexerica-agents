//! Formas fechadas PL1, transcritas de §4 e §11.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Create,
    Begin,
    Attach,
    Intent,
    LocalTicket,
    AppConsent,
    LocalConsent,
    BoxChallenge,
    BoxProof,
    Approve,
    LocalConfirm,
    Consume,
    Grant,
    Challenge,
    Proof,
    Receipt,
    Activation,
    Publish,
    Revoke,
    Fence,
    FenceAck,
    Revocation,
    Cancel,
    Read,
    State,
    TimeRequest,
    Time,
    HostStateRequest,
    HostState,
    ChannelOpen,
    /// HOST-R1-01: testemunho Root do consumo, só no read do Host; não entra em pacote (sem `parse`).
    ConsumptionWitness,
}
impl Kind {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "create" => Some(Self::Create),
            "begin" => Some(Self::Begin),
            "attach" => Some(Self::Attach),
            "intent" => Some(Self::Intent),
            "local-ticket" => Some(Self::LocalTicket),
            "app-consent" => Some(Self::AppConsent),
            "local-consent" => Some(Self::LocalConsent),
            "box-challenge" => Some(Self::BoxChallenge),
            "box-proof" => Some(Self::BoxProof),
            "approve" => Some(Self::Approve),
            "local-confirm" => Some(Self::LocalConfirm),
            "consume" => Some(Self::Consume),
            "grant" => Some(Self::Grant),
            "challenge" => Some(Self::Challenge),
            "proof" => Some(Self::Proof),
            "receipt" => Some(Self::Receipt),
            "activation" => Some(Self::Activation),
            "publish" => Some(Self::Publish),
            "revoke" => Some(Self::Revoke),
            "fence" => Some(Self::Fence),
            "fence-ack" => Some(Self::FenceAck),
            "revocation" => Some(Self::Revocation),
            "cancel" => Some(Self::Cancel),
            "read" => Some(Self::Read),
            "state" => Some(Self::State),
            "time-request" => Some(Self::TimeRequest),
            "time" => Some(Self::Time),
            "host-state-request" => Some(Self::HostStateRequest),
            "host-state" => Some(Self::HostState),
            "channel-open" => Some(Self::ChannelOpen),
            _ => None,
        }
    }
    pub(super) fn shape(self) -> (&'static str, &'static str, &'static str, usize, usize, u64) {
        match self {
            Self::Create => ("create", "A", "id key =command int int", 5, 6, 600_000),
            Self::Begin => ("begin", "H", "id id hash key int int", 6, 7, 600_000),
            Self::Attach => ("attach", "A", "id hash key int int", 5, 6, 600_000),
            Self::Intent => ("intent", "E", "id mode id id key key int int int", 8, 9, 600_000),
            Self::LocalTicket => {
                ("local-ticket", "H", "id mode id key cid key key key =first-owner int int", 11, 12, 600_000)
            }
            Self::AppConsent => ("app-consent", "A", "hash =independent-identity-v1 =approve int int", 5, 6, 600_000),
            Self::LocalConsent => ("local-consent", "H", "hash hash =approve key int int", 6, 7, 600_000),
            Self::BoxChallenge => ("box-challenge", "A", "hash hash hash key key hash cipher int int", 9, 10, 60_000),
            Self::BoxProof => ("box-proof", "H", "hash hash key key int int", 6, 7, 60_000),
            Self::Approve => ("approve", "A", "hash hash hash =independent-identity-v1 =approve int int", 7, 8, 60_000),
            Self::LocalConfirm => ("local-confirm", "H", "hash hash hash =approve key int int", 7, 8, 60_000),
            Self::Consume => ("consume", "H", "hash hash hash int id int int", 7, 8, 60_000),
            Self::Grant => ("grant", "E", "T hash hash hash hash hash key int int caps", 9, 10, 300_000),
            Self::Challenge => ("challenge", "H", "hash hash key int int", 5, 6, 60_000),
            Self::Proof => ("proof", "A", "hash hash hash hash hash int int", 7, 8, 60_000),
            Self::Receipt => ("receipt", "H", "hash hash hash hash grant int int int hash hash", 0, 0, 0),
            Self::Activation => {
                ("activation", "H", "hash grant hash id id key cid key int int hash int int caps", 13, 14, 900_000)
            }
            Self::Publish => ("publish", "A", "id hash hash grant hash int id int int", 9, 10, 30_000),
            Self::Revoke => ("revoke", "A", "id hash hash id =ownerRevoked int int", 7, 8, 30_000),
            Self::Fence => ("fence", "E", "hash hash ?grant ?hash int reason int", 0, 0, 0),
            Self::FenceAck => ("fence-ack", "H", "hash hash ?grant ?hash int int hash int", 0, 0, 0),
            Self::Revocation => ("revocation", "H", "hash grant hash int int hash int reason", 0, 0, 0),
            Self::Cancel => ("cancel", "A", "id int key int int", 5, 6, 30_000),
            Self::Read => ("read", "reader", "id ?id ?hash key int int", 6, 7, 30_000),
            Self::State => {
                ("state", "E", "id ?id ?hash phase int ?grant ?hash ?hash ?hash ?hash ?hash ?hash key int int", 0, 0, 0)
            }
            Self::TimeRequest => ("time-request", "H", "key cid", 0, 0, 0),
            Self::Time => ("time", "E", "key cid int int", 0, 0, 0),
            Self::HostStateRequest => ("host-state-request", "H", "hash hash ?grant ?hash key int int", 7, 8, 30_000),
            Self::HostState => (
                "host-state",
                "E",
                "hash hash grant hash hash id id key cid key caps int zero key int int",
                16,
                17,
                900_000,
            ),
            Self::ConsumptionWitness => ("consumption-witness", "E", "hash hash cid key int", 0, 0, 0),
            Self::ChannelOpen => {
                ("channel-open", "channel", "role =use cid hash ?grant ?key nonce16 path key int int", 11, 12, 30_000)
            }
        }
    }
}
