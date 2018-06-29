#![warn(rust_2018_idioms)]
#![feature(rust_2018_preview, use_extern_macros)]

pub mod builder;
pub mod codec;
pub mod congest_ctrl;
pub mod connected;
pub mod connection_settings;
pub mod default_congest_ctrl;
pub mod loss_compression;
pub mod packet;
pub mod pending_connection;
pub mod receiver;
pub mod sender;
pub mod srt_congest_ctrl;
pub mod srt_packet;
pub mod srt_version;
pub mod stats;
pub mod stats_printer;
#[macro_use]
pub mod modular_num;
pub mod msg_number;
pub mod recv_buffer;
pub mod seq_number;

pub use crate::builder::{ConnInitMethod, SrtSocket, SrtSocketBuilder};
pub use crate::congest_ctrl::{CCData, CongestCtrl};
pub use crate::connected::Connected;
pub use crate::connection_settings::ConnectionSettings;
pub use crate::default_congest_ctrl::DefaultCongestCtrl;
pub use crate::msg_number::MsgNumber;
pub use crate::packet::{ControlPacket, DataPacket, Packet};
pub use crate::pending_connection::PendingConnection;
pub use crate::receiver::Receiver;
pub use crate::sender::Sender;
pub use crate::seq_number::SeqNumber;
pub use crate::srt_congest_ctrl::SrtCongestCtrl;
pub use crate::srt_version::SrtVersion;
pub use crate::stats::Stats;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SocketID(pub u32);
