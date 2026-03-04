// LAN collaboration module (feature-gated behind `collab`).
//
// Provides a simple server-authoritative operational transform (OT)
// system for real-time collaborative editing over TCP on a local
// network.

#[cfg(feature = "collab")]
pub mod client;
#[cfg(feature = "collab")]
pub mod server;
