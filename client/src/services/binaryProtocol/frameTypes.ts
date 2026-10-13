
import type { Vec3 } from '../../types/binaryProtocol';

// Protocol versions
// PROTOCOL_V2 (uint16 payload length, uint16 SSSP IDs) is rejected outright by
// the server (src/utils/binary_protocol.rs: "V2 protocol no longer supported.
// Please upgrade client to V3+."), so it is not advertised or named here
// (ADR-2057). Nothing in this module referenced the constant.
export const PROTOCOL_V3 = 3;  // LIVE: position broadcast (bare 0x03 lead byte, 52 bytes/node; ADR-031 D2 added centrality@48)
// PROTOCOL_V4 is NOT the position/agent wire format. The live server→client
// streams carry no version-tagged 6-byte header: positions lead with a bare
// PROTOCOL_V3 (0x03) byte and agent actions with a bare AGENT_ACTION (0x23) tag
// (see store/websocket/binaryProtocol.ts and commit 67503fb3). V4 only labels
// the 6-byte framed-message header ([type][version][uint32 len]) that
// createMessage/parseHeader/validateMessage use for GRAPH_UPDATE / VOICE /
// control / sync framing.
// V5 wraps a V3 body with an 8-byte little-endian broadcast sequence number:
// `[0x05][u64 broadcast_seq LE][V3 node records]`. Mirrors the server branch
// at src/utils/binary_protocol.rs (`match protocol_version { 5 => ... }`) and
// the Godot XR client's `decode_position_frame_with_sequence`
// (xr-client/rust/src/binary_protocol.rs). The V5 body carries no inner 0x03
// byte — node records start immediately after the sequence (ADR-2057).
export const PROTOCOL_V5 = 5;
// Bytes the V5 envelope inserts between the version byte and the V3 body.
export const V5_SEQ_BYTES = 8;
// NOTE: the 52-byte record size and the node-id mask are NOT redeclared here.
// They live once in client/src/types/binaryProtocol.ts (BINARY_NODE_SIZE_V3,
// NODE_ID_MASK), which is the canonical live decoder — a second copy is exactly
// how the V2/V5 drift arose (ADR-2057).
export const PROTOCOL_VERSION = PROTOCOL_V3;  // Default to the live V3 position protocol
// Versions the framed header's version byte may legitimately carry. `createMessage`
// writes PROTOCOL_VERSION (= PROTOCOL_V3) there, and a V5 envelope may be framed,
// so those are the two. ADR-2078: the old list also carried a `PROTOCOL_V4 = 4`
// declared in THIS file, which nothing ever wrote into the header byte and which
// collided by name with the retired-delta-encoding PROTOCOL_V4 in
// client/src/types/binaryProtocol.ts. That constant is gone from here: the name
// PROTOCOL_V4 means exactly one thing in the codebase — the RETIRED delta tag, which
// the decoder names only to refuse it (ADR-2139: position frames are full snapshots).
export const SUPPORTED_HEADER_VERSIONS = [PROTOCOL_V3, PROTOCOL_V5];

// Message types (1 byte header)
/**
 * Message-type tag space. ADR-2078 alignment note.
 *
 * The SERVER's `MessageType` (`src/utils/binary_protocol.rs:1705-1722`) defines only FIVE tags,
 * and those are the server-to-client registry:
 *   BinaryPositions = 0, VoiceData = 0x02, ControlFrame = 0x03, AgentAction = 0x23,
 *   BroadcastAck = 0x34.
 * Of those, only AGENT_ACTION (0x23) and BROADCAST_ACK (0x34) are decoded from the wire by this
 * client — live position frames are UNFRAMED and lead with a bare 0x03/0x05 version byte, so they
 * never carry a MessageType tag at all (see the framed-header note above).
 *
 * Every other member below is CLIENT-OUTBOUND ONLY: it names a type this client puts in the 6-byte
 * framed header it sends, and has no counterpart in the server enum. They are kept, not deleted,
 * because the framed-header path is a client-to-server surface whose server-side parsing lives
 * outside `binary_protocol.rs`; removing them from the client alone could silently break an
 * outbound encoder. They are NOT evidence that the server emits these tags.
 *
 * ADR-2078 deleted five members that had zero references anywhere in client/src, by name or by
 * literal value: VELOCITY_UPDATE (0x12), AGENT_STATE_DELTA (0x21), AGENT_HEALTH (0x22),
 * HANDSHAKE (0x32) and HEARTBEAT (0x33). Nothing encoded or decoded them. Naming them as
 * "deletion candidates" in a comment would have been deferred deletion, which this estate does
 * not do — dead code is deleted. Every member that remains has a live reference.
 */
export enum MessageType {

  GRAPH_UPDATE = 0x01,


  VOICE_DATA = 0x02,          // SERVER TAG (binary_protocol.rs:1711)


  POSITION_UPDATE = 0x10,
  AGENT_POSITIONS = 0x11,


  AGENT_STATE_FULL = 0x20,
  AGENT_ACTION = 0x23,        // SERVER TAG (binary_protocol.rs:1721) - decoded inbound


  CONTROL_BITS = 0x30,
  SSSP_DATA = 0x31,


  VOICE_CHUNK = 0x40,
  VOICE_START = 0x41,
  VOICE_END = 0x42,

  // Backpressure flow control (Phase 7)
  BROADCAST_ACK = 0x34,      // SERVER TAG (binary_protocol.rs:1717) - client ack of position broadcast

  // Multi-user sync messages (Phase 6)
  SYNC_UPDATE = 0x50,        // Graph operation sync
  ANNOTATION_UPDATE = 0x51,  // Annotation sync
  SELECTION_UPDATE = 0x52,   // Selection sync
  USER_POSITION = 0x53,      // User cursor/avatar position
  VR_PRESENCE = 0x54,        // VR head + hand tracking


  ERROR = 0xFF
}

// Graph type flags for GRAPH_UPDATE messages
// Values must match server: src/utils/binary_protocol.rs GraphType enum
export enum GraphTypeFlag {
  KNOWLEDGE_GRAPH = 0x00,
  ONTOLOGY = 0x01
}

// Agent state flags (bit field)
export enum AgentStateFlags {
  ACTIVE = 1 << 0,
  IDLE = 1 << 1,
  ERROR = 1 << 2,
  VOICE_ACTIVE = 1 << 3,
  HIGH_PRIORITY = 1 << 4,
  POSITION_CHANGED = 1 << 5,
  METADATA_CHANGED = 1 << 6,
  RESERVED = 1 << 7
}

// Control bit flags
export enum ControlFlags {
  PAUSE_UPDATES = 1 << 0,
  HIGH_FREQUENCY = 1 << 1,
  LOW_BANDWIDTH = 1 << 2,
  VOICE_ENABLED = 1 << 3,
  DEBUG_MODE = 1 << 4,
  FORCE_FULL_UPDATE = 1 << 5,
  USER_INTERACTING = 1 << 6,
  BACKGROUND_MODE = 1 << 7
}

// Binary data structures

export interface AgentPositionUpdate {
  agentId: number;
  position: Vec3;
  timestamp: number;
  flags: number;
}

export interface AgentStateData {
  agentId: number;
  position: Vec3;
  velocity: Vec3;
  health: number;
  cpuUsage: number;
  memoryUsage: number;
  workload: number;
  tokens: number;
  flags: number;
}

export interface SSSPData {
  nodeId: number;
  distance: number;
  parentId: number;
  flags: number;
}

export interface VoiceChunk {
  agentId: number;
  chunkId: number;
  format: number;
  dataLength: number;
  audioData: ArrayBuffer;
}

export interface MessageHeader {
  type: MessageType;
  version: number;
  payloadLength: number;
  graphTypeFlag?: GraphTypeFlag;
}

// Broadcast ACK data for backpressure flow control
export interface BroadcastAckData {
  sequenceId: number;     // 8 bytes - correlates with server broadcast sequence
  nodesReceived: number;  // 4 bytes - number of nodes client processed
  timestamp: number;      // 8 bytes - client receive timestamp (ms)
}

// Agent action types for ephemeral connection visualization
export enum AgentActionType {
  Query = 0,      // Agent querying data node (blue)
  Update = 1,     // Agent updating data node (yellow)
  Create = 2,     // Agent creating data node (green)
  Delete = 3,     // Agent deleting data node (red)
  Link = 4,       // Agent linking nodes (purple)
  Transform = 5,  // Agent transforming data (cyan)
}

// Agent action event for visualization
export interface AgentActionEvent {
  sourceAgentId: number;    // 4 bytes - ID of the acting agent
  targetNodeId: number;     // 4 bytes - ID of the target data node
  actionType: AgentActionType; // 1 byte
  timestamp: number;        // 4 bytes - Event timestamp (ms)
  durationMs: number;       // 2 bytes - Animation duration hint
  payload?: Uint8Array;     // Variable - Optional metadata
}

// Color mapping for action types (used by visualization layer)
export const AGENT_ACTION_COLORS: Record<AgentActionType, string> = {
  [AgentActionType.Query]: '#3b82f6',     // Blue
  [AgentActionType.Update]: '#eab308',    // Yellow
  [AgentActionType.Create]: '#22c55e',    // Green
  [AgentActionType.Delete]: '#ef4444',    // Red
  [AgentActionType.Link]: '#a855f7',      // Purple
  [AgentActionType.Transform]: '#06b6d4', // Cyan
};

// Wire format size for agent action header
export const AGENT_ACTION_HEADER_SIZE = 15;

export interface GraphUpdateHeader extends MessageHeader {
  graphTypeFlag: GraphTypeFlag;
}

// Constants for binary layout
// Framed-message header: [1-byte type][1-byte version][4-byte payloadLength] = 6 bytes
// (GRAPH_UPDATE / VOICE / control / sync framing; the live position + agent-action
// streams are unframed — see the framed-header note above)
export const MESSAGE_HEADER_SIZE = 6;
export const GRAPH_UPDATE_HEADER_SIZE = 7;  // MESSAGE_HEADER_SIZE + 1-byte graphTypeFlag
export const AGENT_POSITION_SIZE_V2 = 21;  // 4 (u32 id) + 12 (pos) + 4 (timestamp) + 1 (flags)
export const AGENT_STATE_SIZE_V2 = 49;     // Full agent state with u32 IDs
// SSSP layout: 4 (u32 nodeId) + 4 (f32 distance) + 4 (u32 parentId) + 2 (u16 flags) = 14 bytes
export const SSSP_DATA_SIZE_V2 = 14;       // SSSP with u32 IDs

// Canonical sizes
export const AGENT_POSITION_SIZE = AGENT_POSITION_SIZE_V2;
export const AGENT_STATE_SIZE = AGENT_STATE_SIZE_V2;
export const SSSP_DATA_SIZE = SSSP_DATA_SIZE_V2;

export const VOICE_HEADER_SIZE = 7;
