// Node and graph constants
pub const NODE_SIZE: f32 = 1.0;
pub const EDGE_WIDTH: f32 = 0.1;
pub const MIN_DISTANCE: f32 = 0.75;
pub const MAX_DISTANCE: f32 = 10.0;

// The /wss heartbeat lives in `system.websocket.heartbeat{Interval,Timeout}`
// (handlers/socket_flow_handler/heartbeat.rs) and the position-stream rate in
// `physics.broadcastFps`; the unused constants that shadowed them are gone.

// Binary message constants
pub const NODE_POSITION_SIZE: usize = 24;
pub const BINARY_HEADER_SIZE: usize = 4;

// Compression constants
pub const COMPRESSION_THRESHOLD: usize = 1024;
pub const ENABLE_COMPRESSION: bool = true;
