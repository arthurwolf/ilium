# API gaps of ilium-ipc/tests/plugin_records (lane host-ipc, part 1)

- ilium-ipc: `pub const PLUGIN_PROTOCOL_VERSION: u16 = 1;` re-exported at the crate root (`ilium_ipc::PLUGIN_PROTOCOL_VERSION`). [HST-106] manual: constant lives in ilium-ipc.
- ilium-ipc `plugin` module: `pub struct PluginEnvelope { pub plugin: String, pub seq: u64, pub body: EnvelopeBody }` (derive Debug, Clone, PartialEq, Eq, Serialize, Deserialize). [HST-100] the envelope; plain string id because ilium-ipc has no plugin-crate edge.
- ilium-ipc `plugin`: `pub enum EnvelopeBody { Submit(Submission), Outcome(OutcomeRecord), Message(PluginMessage), ClientRequest(ClientRequestCall), ClientReply(ClientReplyBody), Event(SourcedEvent), Control(ControlBody) }` in exactly this order, no other variant. [HST-101] order fixes bincode variant indices.
- ilium-ipc `plugin`: `pub struct Submission { pub client_id: String, pub dedup_key: String, pub intent_json: String }`. [HST-112] shape chosen by the tests; the manual does not list fields.
- ilium-ipc `plugin`: `pub struct OutcomeRecord { pub seq: u64, pub dedup_key: String, pub outcome_json: String }`. [HST-115]
- ilium-ipc `plugin`: `pub struct PluginMessage { pub topic: String, pub payload: Vec<u8> }`, `pub struct ClientRequestCall { pub call_id: u64, pub name: String, pub payload: Vec<u8> }`, `pub struct ClientReplyBody { pub call_id: u64, pub payload: Vec<u8> }`, `pub struct SourcedEvent { pub topic: String, pub payload: Vec<u8> }`. [HST-100]
- ilium-ipc `plugin`: `pub enum ControlBody { Subscribe { topics: Vec<String> }, Resync, GrantsChanged, Stats }` in this order. [HST-108] manual lists "Subscribe, Resync, GrantsChanged, Stats".
- ilium-ipc `plugin`: `pub struct PluginHello { pub plugin_protocol: u16, pub contract: String, pub installed: Vec<InstalledPlugin> }` and `pub struct InstalledPlugin { pub id: String, pub version: String, pub content_hash: [u8; 32] }`. [HST-103] (id is String, not PluginId, as above).
- ilium-ipc `plugin`: `pub struct ContributionDelta { pub sequence: u64, pub kind: DeltaKind, pub ops: Vec<ContributionOp> }`, `pub enum DeltaKind { Snapshot, Increment }`, `pub enum ContributionOp { Put { id: String, entry: Vec<u8> }, Remove { id: String }, Clear { plugin: String } }`. [HST-107]
- ilium-ipc `plugin`: `pub struct Rejection { pub code: String, pub message: String }`. [HST-100, HST-103]
- ilium-ipc `plugin`: `pub const MAX_DELTA_BYTES: usize = 4 * 1024 * 1024;`. [HST-109]
- ilium-ipc protocol: `ClientRequest::Plugin(PluginEnvelope)` appended; `ServerEvent::Plugin(PluginEnvelope)`, `ServerEvent::Contributions(ContributionDelta)`, `ServerEvent::Rejected(Rejection)` appended (append-only). [HST-100]
- ilium-ipc protocol: `ClientRequest::Attach { session: String, hello: PluginHello }` and `ClientRequest::AttachInteractive { session: String, hello: PluginHello }` (field `hello` added). [HST-103]
